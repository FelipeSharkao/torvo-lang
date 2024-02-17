use std::collections::HashMap;
use std::iter::zip;
use std::pin::Pin;

use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::ir::{types, Block, Function, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncOrDataId, Module};
use cranelift_object::ObjectModule;

use crate::proto::lex;

pub struct FnCodegen<'a> {
    pub module: &'a mut ObjectModule,
    pub builder: FunctionBuilder<'a>,
    variables: HashMap<String, Variable>,
    #[allow(dead_code)]
    // This is a hack to get the borrow checker to allow `builder` to borrow `builder_ctx` wile
    // moving `builder_ctx` to `FnCodegen`
    // I tried using using the ouroboros crate to do this, but it was really buggy due to the `'a`
    // The risk doing this is that it will break if `builder` or `builder_ctx` is moved out of sync
    // with each other, since the borrow checker won't be able to catch it ahead of time
    builder_ctx: Pin<Box<FunctionBuilderContext>>,
}

impl<'a> FnCodegen<'a> {
    pub fn new(module: &'a mut ObjectModule, func: &'a mut Function) -> Self {
        let mut builder_ctx = Box::pin(FunctionBuilderContext::new());
        let ptr: *mut FunctionBuilderContext = &mut *builder_ctx;

        FnCodegen {
            module,
            builder_ctx,
            variables: HashMap::new(),
            builder: unsafe { FunctionBuilder::new(func, &mut *ptr) },
        }
    }

    pub fn build(module: &'a mut ObjectModule, func: &'a mut Function, decl: &lex::FnDecl) {
        let mut this = Self::new(module, func);

        this.create_entry_block(&decl.args);

        for instr in decl.body.iter() {
            if let Some(instr) = instr.instr.as_ref() {
                this.instr(instr);
            }
        }

        println!("<{}>: {}", decl.name, &this.builder.func);

        this.finalize();
    }

    pub fn use_var(&mut self, lex_value: &lex::Value) -> Value {
        match lex_value.value.as_ref() {
            Some(lex::value::Value::Num(num)) => {
                let ptr_ty = self.module.target_config().pointer_type();
                // maybe this parsing should be handled by the lexer
                self.builder
                    .ins()
                    .iconst(ptr_ty, num.value.parse::<i64>().unwrap())
            }
            Some(lex::value::Value::Ident(ident)) => {
                let var = self
                    .variables
                    .get(ident)
                    .expect(&format!("Variable {} not found", ident));
                self.builder.use_var(*var)
            }
            None => {
                unreachable!();
            }
        }
    }

    pub fn define_var(&mut self, name: &str, ty: types::Type, value: Value) {
        let variables = &mut self.variables;

        variables.insert(name.to_string(), Variable::new(variables.len()));
        let var = variables.get(name).unwrap();
        self.builder.declare_var(*var, ty);
        self.builder.def_var(*var, value);
    }

    pub fn create_entry_block(&mut self, args: &[String]) -> Block {
        let entry_block = self.builder.create_block();
        self.builder
            .append_block_params_for_function_params(entry_block);
        self.builder.switch_to_block(entry_block);

        let entry_block_params = self.builder.block_params(entry_block).to_vec();
        for (arg, block_param) in zip(args.iter(), entry_block_params.iter()) {
            // FIXME: hardcoded type
            let ptr_ty = self.module.target_config().pointer_type();
            self.define_var(&arg, ptr_ty, *block_param);
        }

        entry_block
    }

    pub fn instr(&mut self, instr: &lex::instr::Instr) {
        match instr {
            lex::instr::Instr::Assign(assign) => {
                let value = self.use_var(&assign.value);
                // FIXME: hardcoded type
                let ptr_ty = self.module.target_config().pointer_type();
                self.define_var(&assign.name, ptr_ty, value);
            }
            lex::instr::Instr::BinOp(bin_op) => {
                // Different instructions for different types, might want to use some kind of
                // abstraction for this
                let left = self.use_var(&bin_op.left);
                let right = self.use_var(&bin_op.right);

                let tmp = match bin_op.op() {
                    lex::BinOpType::Add => self.builder.ins().iadd(left, right),
                    lex::BinOpType::Sub => self.builder.ins().isub(left, right),
                    lex::BinOpType::Mul => self.builder.ins().imul(left, right),
                    lex::BinOpType::Div => self.builder.ins().sdiv(left, right),
                    lex::BinOpType::Mod => self.builder.ins().srem(left, right),
                    lex::BinOpType::Pow => {
                        // TODO: exponentiation by squaring
                        // https://stackoverflow.com/a/101613
                        self.builder.ins().imul(left, right)
                    }
                };

                // FIXME: hardcoded type
                let ptr_ty = self.module.target_config().pointer_type();
                self.define_var(&bin_op.name, ptr_ty, tmp);
            }
            lex::instr::Instr::FnCall(fn_call) => {
                let mut args = Vec::new();
                for arg in fn_call.args.iter() {
                    args.push(self.use_var(arg));
                }

                let results = self.call(&fn_call.callee, &args);
                if let &[tmp] = results {
                    // FIXME: hardcoded type
                    let ptr_ty = self.module.target_config().pointer_type();
                    self.define_var(&fn_call.name, ptr_ty, tmp);
                }
            }
            lex::instr::Instr::FnReturn(fn_return) => {
                if fn_return.value.value.is_none() {
                    self.builder.ins().return_(&[]);
                } else {
                    let value = self.use_var(&fn_return.value);
                    self.builder.ins().return_(&[value]);
                }
            }
            lex::instr::Instr::FnDecl(_) => {
                panic!("Found function declaration in function body");
            }
        }
    }

    pub fn call(&mut self, func_name: &str, args: &[Value]) -> &[Value] {
        let Some(FuncOrDataId::Func(func_id)) = self.module.get_name(func_name) else {
            // TODO: better error handling
            panic!("Function {} not found", func_name);
        };

        let func = &mut self.builder.func;
        let func_ref = self.module.declare_func_in_func(func_id, func);

        let instr = self.builder.ins().call(func_ref, args);
        self.builder.inst_results(instr)
    }

    pub fn finalize(mut self) {
        // Sealing the block means that everything before it is done and won't change. Creanelift's
        // documentation recomends sealing each block as soon as possible, but since we're doing a
        // lot of back patching in the function, sealing all the blocks at the end of the function
        // is the only way to go. Maybe the lex typing could be changed to provide all the
        // information we need, like variables names, types and so on, so we can avoid this
        self.builder.seal_all_blocks();

        self.builder.finalize();
    }
}
