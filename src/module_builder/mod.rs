mod instr_builder;
mod registry;

use tree_sitter as ts;

use self::instr_builder::InstrBuilder;
use self::registry::ModuleRegistry;
use crate::mir;
use crate::module_builder::registry::{
    FuncRegistry, Registry, ValueTypeDeps, VirtualValue,
};
use crate::tree_sitter_utils::TreeSitterUtils;

pub struct ModuleBuilder<'a> {
    pub name: String,
    pub source: &'a str,
    registry: ModuleRegistry,
    globals: Vec<mir::Global>,
    funcs: Vec<mir::Func>,
    init_body: Vec<mir::Instr>,
}

impl<'a> ModuleBuilder<'a> {
    pub fn new(name: &str, source: &'a str) -> Self {
        let mut registry = ModuleRegistry::new();

        // TODO: detect which intrinsic functions are needed and declare only those
        // TODO: use syscalls instead of libc functions, maybe we will have to implement a
        //       wrapper around syscall in C to be able to call it from Cranelift

        let funcs = [
            // void exit(int status);
            (
                "exit",
                mir::Func {
                    extern_: Some(mir::Extern {
                        name: "exit".to_string(),
                    }),
                    params: ext_params([mir::Type::I32]),
                    ret: vec![],
                    ..Default::default()
                },
            ),
            // ssize_t write(int fildes, const void *buf, size_t nbyte);
            (
                "write",
                mir::Func {
                    extern_: Some(mir::Extern {
                        name: "write".to_string(),
                    }),
                    params: ext_params([
                        mir::Type::I32,
                        mir::Type::String(mir::StringType { len: None }),
                        mir::Type::USize,
                    ]),
                    ret: vec![mir::Type::USize],
                    ..Default::default()
                },
            ),
        ];

        for (i, (name, func)) in funcs.iter().enumerate() {
            let params_ty: Vec<_> = func.params.iter().map(|p| p.ty.clone()).collect();
            registry.register_func(name, params_ty.clone());
            registry.set_value_type(
                VirtualValue::Func(i as u32),
                mir::Type::func_type(params_ty, func.ret.clone()),
                None,
            );
        }

        ModuleBuilder {
            name: name.to_string(),
            source,
            globals: Vec::new(),
            funcs: funcs.into_iter().map(|(_, f)| f).collect(),
            registry,
            init_body: Vec::new(),
        }
    }

    pub fn parse(mut self, node: &'a ts::Node<'a>) -> mir::Module {
        node.of_kind("root");

        for sym_node in node.iter_children() {
            let ident_node = sym_node.required_field("name").of_kind("ident");
            let ident = ident_node.get_text(self.source).to_string();

            match sym_node.kind() {
                "fn_decl" => self.add_func(ident, sym_node),
                "global_var_decl" => self.add_global(ident, sym_node),
                _ => panic!("Unexpected symbol kind: {}", sym_node.kind()),
            }
        }

        self.finish()
    }

    pub fn add_func(&mut self, name: String, node: ts::Node<'a>) {
        assert_eq!(node.kind(), "fn_decl");

        let mut local_registry = FuncRegistry::new(&mut self.registry);
        let mut local_builder = InstrBuilder::new(&mut local_registry, self.source);

        for param_node in node.iter_field("params") {
            let param_name_node = param_node.required_field("pat").of_kind("ident");
            let param_name = param_name_node.get_text(self.source);

            let ty = param_node
                .field("type")
                .as_ref()
                .map_or(mir::Type::Unknown, |ty_node| {
                    local_builder.parse_type(ty_node)
                });

            local_builder.registry.register_param(param_name, ty);
        }

        let func_idx = local_builder
            .registry
            .module_registry
            .register_func(&name, local_builder.registry.get_params().map(|p| p.ty));
        let func_value = VirtualValue::Func(func_idx);
        local_builder
            .registry
            .idents
            .insert(&name, func_value.clone());

        let ret = node
            .field("ret_type")
            .map(|ty_node| local_builder.parse_type(&ty_node));
        let ret = local_builder.add_return(&node.required_field("return"), ret);

        let locals: Vec<_> = local_builder.registry.get_locals().collect();
        let params: Vec<_> = local_builder.registry.get_params().collect();

        if ret.is_ambig() {
            panic!("Type should be known for function return: {}", name);
        }

        let ty = mir::Type::func_type(params.iter().map(|p| p.ty.clone()), [ret.clone()]);

        local_builder.registry.module_registry.set_value_type(
            func_value.clone(),
            ty.clone(),
            None,
        );

        let body = local_builder.finish();

        self.funcs.push(mir::Func {
            export: Some(mir::Export { name }),
            locals,
            params,
            ret: vec![ret],
            body,
            ..Default::default()
        });
    }

    pub fn add_global(&mut self, name: String, node: ts::Node<'a>) {
        assert_eq!(node.kind(), "global_var_decl");

        let mut instr_builder = InstrBuilder::new(&mut self.registry, self.source);

        let (v_value, ty) = instr_builder.add_expr(&node.required_field("value"));

        let ty = match &node.field("type") {
            Some(ty_node) => {
                let manual_ty = instr_builder.parse_type(ty_node);
                manual_ty.merge_with(&ty).expect(&format!(
                    "Type mismatch: expected {}, got {}",
                    manual_ty, ty
                ))
            }
            None => ty,
        };

        if ty.is_ambig() {
            // FIXME: allow local to constrain the type of the global
            panic!("Type should be known for global value: {}", name);
        }

        let deps = match &v_value {
            VirtualValue::Array(items) => {
                let mir::Type::Array(array_ty) = &ty else {
                    panic!("Expected array type, got {}", ty);
                };

                if array_ty.len.is_none() {
                    panic!("Array length should be known for global array: {}", name);
                }

                if array_ty.len.unwrap() != items.len() {
                    panic!(
                        "Array length mismatch: expected {}, got {}",
                        array_ty.len.unwrap(),
                        items.len()
                    );
                }

                ValueTypeDeps {
                    refs: items.clone(),
                    sig: array_ty
                        .item
                        .possible_types()
                        .into_iter()
                        .map(|t| mir::FuncType::array_sig(&t, items.len()))
                        .collect(),
                }
            }
            _ => ValueTypeDeps {
                sig: vec![mir::FuncType::new(vec![ty.clone()], vec![ty.clone()])],
                refs: vec![v_value.clone()],
            },
        };

        let global_idx = instr_builder
            .registry
            .register_global(&name, ty.clone(), deps);

        let const_value = match (mir::ConstValue::try_from(v_value.clone()), &v_value) {
            (Ok(v), _) => Some(v),
            (_, VirtualValue::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let value = instr_builder.use_virtual_value(item);
                    instr_builder.body.push(mir::Instr::StoreGlobal(
                        mir::StoreGlobalInstr {
                            global_idx,
                            field_idx: Some(i as u32),
                            value,
                        },
                    ));
                }

                None
            }
            (_, _) => {
                let value = instr_builder.use_virtual_value(&v_value);
                instr_builder
                    .body
                    .push(mir::Instr::StoreGlobal(mir::StoreGlobalInstr {
                        global_idx,
                        field_idx: None,
                        value,
                    }));

                // Shadow the global value with the local result so next time we use the
                // global we get the local value instead of loading the global again.
                instr_builder
                    .registry
                    .init_idents
                    .insert(&name, v_value.clone());

                None
            }
        };

        self.globals.push(mir::Global {
            // FIXME: read export info from the source
            export: if name == "main" {
                Some(mir::Export { name })
            } else {
                None
            },
            ty,
            value: const_value,
        });

        self.init_body.extend(instr_builder.finish());
    }

    pub fn finish(self) -> mir::Module {
        mir::Module {
            name: self.name,
            globals: self.globals,
            funcs: self.funcs,
            init: if self.init_body.len() > 0 {
                Some(mir::ModuleInit {
                    locals: self.registry.get_locals().collect(),
                    body: self.init_body,
                })
            } else {
                None
            },
        }
    }
}

fn ext_params(params: impl IntoIterator<Item = mir::Type>) -> Vec<mir::Param> {
    params.into_iter().map(|ty| mir::Param { ty }).collect()
}
