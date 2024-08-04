use std::collections::HashMap;

use cranelift_shim::{self as cl};
use derive_new::new;
use itertools::repeat_n;

use super::func::FuncCodegen;
use super::types;
use crate::{bytecode as b, utils};

#[derive(Debug, Clone)]
pub struct GlobalBinding {
    pub symbol_name: String,
    pub value: types::RuntimeValue,
    pub ty: b::Type,
    pub init: Option<Vec<b::Instr>>,
    pub entry: bool,
}

/// Describe all static data that is present in the module and which values they represent
#[derive(Debug, new)]
pub struct Globals {
    #[new(default)]
    pub data: HashMap<cl::DataId, cl::DataDescription>,
    #[new(default)]
    strings: HashMap<String, cl::DataId>,
    #[new(default)]
    tuples: HashMap<Vec<types::RuntimeValue>, cl::DataId>,
    #[new(default)]
    pub globals: Vec<GlobalBinding>,
}
impl Globals {
    pub fn get_global(&self, idx: usize) -> Option<&GlobalBinding> {
        self.globals.get(idx)
    }

    pub fn insert_global<M: cl::Module>(
        &mut self,
        idx: usize,
        global: &b::Global,
        module: M,
        typedefs: Vec<b::TypeDef>,
    ) -> M {
        assert!(idx == self.globals.len());

        // TODO: improve name mangling
        let symbol_name = format!("$global{idx}");

        let (value, is_const, module) = utils::replace_with(self, |s| {
            let mut codegen = FuncCodegen::new(None, module, s, vec![], typedefs.clone());

            for instr in &global.body {
                if let Some(value) = codegen.value_from_instr(instr) {
                    codegen.stack.push(value);
                } else {
                    let (data_id, module) = codegen.globals.create_writable_for_type(
                        &global.ty,
                        &typedefs,
                        codegen.module,
                    );
                    let value =
                        types::RuntimeValue::new(global.ty.clone(), data_id.into());
                    return (codegen.globals, (value, false, module));
                }
            }

            assert!(codegen.stack.len() >= 1);
            (codegen.globals, (codegen.stack.pop(), true, codegen.module))
        });

        self.globals.push(GlobalBinding {
            symbol_name,
            value,
            ty: global.ty.clone(),
            init: if is_const {
                None
            } else {
                Some(global.body.clone())
            },
            entry: global.entry,
        });

        module
    }

    pub fn data_for_string<M: cl::Module>(
        &mut self,
        value: &str,
        mut module: M,
    ) -> (cl::DataId, M) {
        if let Some(id) = self.strings.get(value) {
            return (*id, module);
        }

        let data_id = module.declare_anonymous_data(false, false).unwrap();
        let mut desc = cl::DataDescription::new();

        let mut bytes = value.as_bytes().to_vec();
        // Append a null terminator to avoid problems if used as a C string
        bytes.extend([0]);

        desc.define(bytes.into());
        module.define_data(data_id, &desc).unwrap();

        self.data.insert(data_id, desc);
        self.strings.insert(value.to_string(), data_id);
        (data_id, module)
    }

    pub fn data_for_tuple<M: cl::Module>(
        &mut self,
        values: Vec<types::RuntimeValue>,
        mut module: M,
    ) -> (Option<cl::DataId>, M) {
        if let Some(id) = self.tuples.get(&values) {
            return (Some(*id), module);
        }

        let data_id = module.declare_anonymous_data(false, false).unwrap();
        let mut desc = cl::DataDescription::new();

        let mut bytes = vec![];
        let mut included_datas = HashMap::new();

        for item in &values {
            if let types::ValueSource::Data(field_data_id) = item.src {
                let offset = bytes.len();
                bytes.extend(repeat_n(0u8, module.isa().pointer_bytes() as usize));

                let field_gv = included_datas.entry(field_data_id).or_insert_with(|| {
                    module.declare_data_in_data(field_data_id, &mut desc)
                });
                desc.write_data_addr(offset as u32, field_gv.clone(), 0);
            } else {
                if let Err(()) = item.serialize(&mut bytes, module.isa().endianness()) {
                    return (None, module);
                }
            }
        }

        desc.define(bytes.into());
        module.define_data(data_id, &desc).unwrap();

        self.data.insert(data_id, desc);
        self.tuples.insert(values, data_id);
        (Some(data_id), module)
    }

    pub fn create_writable_for_type<M: cl::Module>(
        &mut self,
        ty: &b::Type,
        typedefs: &[b::TypeDef],
        mut module: M,
    ) -> (cl::DataId, M) {
        let ptr = module.isa().pointer_bytes() as usize;

        let size = match ty {
            b::Type::String(s) => s.len.map_or(ptr, |len| len + 1),
            b::Type::Array(a) => a.len.map_or(ptr, |len| {
                len * types::get_type(&a.item, typedefs, &module).bytes() as usize
            }),
            b::Type::TypeRef(i) => match &typedefs[*i as usize].body {
                b::TypeDefBody::Record(rec) => rec
                    .fields
                    .values()
                    .map(|field| {
                        types::get_type(&field.ty, typedefs, &module).bytes() as usize
                    })
                    .sum(),
            },
            b::Type::Bool
            | b::Type::I8
            | b::Type::U8
            | b::Type::I16
            | b::Type::U16
            | b::Type::I32
            | b::Type::U32
            | b::Type::I64
            | b::Type::U64
            | b::Type::USize
            | b::Type::F32
            | b::Type::F64 => types::get_type(ty, typedefs, &module).bytes() as usize,
            b::Type::AnyNumber
            | b::Type::AnySignedNumber
            | b::Type::AnyFloat
            | b::Type::Inferred(_) => unreachable!(),
        };

        let data_id = module.declare_anonymous_data(false, false).unwrap();
        let mut desc = cl::DataDescription::new();
        desc.define_zeroinit(size);
        module.define_data(data_id, &desc).unwrap();

        self.data.insert(data_id, desc);
        (data_id, module)
    }
}
