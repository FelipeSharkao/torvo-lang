mod registry;
mod types;

use self::registry::ValueTypeDeps;
use self::types::{ambig, binop_sig, fn_type, merge_types, primitive, types_iter};
use self::{registry::Registry, types::num_type};
use crate::proto::{ast, lex};

impl From<&ast::Module> for lex::Module {
    fn from(value: &ast::Module) -> Self {
        let mut lexer = Lexer::new(value.name.clone());

        for stmt in value.body.iter() {
            lexer.add_symbol(&stmt);
        }

        lexer.finish()
    }
}

pub struct Lexer {
    pub name: String,
    registry: Registry,
    symbols: Vec<lex::Symbol>,
}

impl Lexer {
    pub fn new(name: String) -> Self {
        Lexer {
            name,
            symbols: Vec::new(),
            registry: Registry::new(),
        }
    }

    pub fn add_symbol(&mut self, node: &ast::Stmt) {
        match &node.stmt {
            Some(ast::stmt::Stmt::Fn(func)) => {
                let name = self.registry.use_name(Some(&func.name));

                let mut builder = InstrBuilder::new(&mut self.registry);

                let mut args = Vec::new();
                let mut args_ty = Vec::new();
                for arg in func.args.iter() {
                    let ast::pat::Pat::Name(name_pat) = arg.pat.pat.as_ref().unwrap();
                    args.push(builder.registry.use_name(Some(name_pat)).to_string());

                    let ty = arg.r#type.as_ref().map_or(
                        lex::Type {
                            r#type: Some(lex::r#type::Type::Unknown(true)),
                        },
                        |ty| builder.parse_type(&ty),
                    );

                    args_ty.push(ty.clone());
                    builder.registry.insert_value_type(
                        &name_pat,
                        ty.clone(),
                        ValueTypeDeps::default(),
                    );
                }

                let mut ret_ty = func.ret_type.as_ref().map_or(
                    lex::Type {
                        r#type: Some(lex::r#type::Type::Unknown(true)),
                    },
                    |ty| builder.parse_type(&ty),
                );

                let ty = fn_type(args_ty.clone(), [ret_ty.clone()]);

                self.registry
                    .insert_value_type(&name, ty.clone(), ValueTypeDeps::default());
                builder
                    .registry
                    .insert_value_type(&name, ty.clone(), ValueTypeDeps::default());

                if let Some(ret) = &func.ret {
                    let (ret, _) = builder.add_expr(&ret, None);
                    builder.body.push(lex::Instr {
                        instr: Some(lex::instr::Instr::FnReturn(ret.clone())),
                    });

                    if let lex::value::Value::Ident(ident) = ret.value.as_ref().unwrap() {
                        builder.registry.set_value_type(ident, ret_ty, Some(&name));
                    }

                    ret_ty = builder.registry.value_type(&ret);
                }

                for (i, arg) in args.iter().enumerate() {
                    let arg_value = lex::Value {
                        value: Some(lex::value::Value::Ident(arg.to_string())),
                    };
                    let parsed_ty = builder.registry.value_type(&arg_value);
                    args_ty[i] = parsed_ty;
                }

                let ty = fn_type(args_ty.clone(), [ret_ty.clone()]);

                self.registry.set_value_type(&name, ty.clone(), None);
                builder.registry.set_value_type(&name, ty, None);

                self.symbols.push(lex::Symbol {
                    symbol: Some(lex::symbol::Symbol::FnDecl(lex::FnDecl {
                        name,
                        r#type: lex::FnType {
                            ret: vec![ret_ty],
                            args: args_ty,
                        },
                        args,
                        body: builder.finish(),
                    })),
                });
            }
            Some(ast::stmt::Stmt::Var(var)) => {
                let ast::pat::Pat::Name(original_name) = var.pat.pat.as_ref().unwrap();
                let name = self.registry.use_name(Some(original_name));

                let mut builder = InstrBuilder::new(&mut self.registry);

                let ty = var.r#type.as_ref().map_or(
                    lex::Type {
                        r#type: Some(lex::r#type::Type::Unknown(true)),
                    },
                    |ty| builder.parse_type(&ty),
                );

                builder
                    .registry
                    .insert_value_type(&name, ty.clone(), ValueTypeDeps::default());
                self.registry
                    .insert_value_type(&name, ty.clone(), ValueTypeDeps::default());

                let (value, _) = builder.add_expr(&var.value, None);
                builder.body.push(lex::Instr {
                    instr: Some(lex::instr::Instr::BodyReturn(value.clone())),
                });

                if let lex::value::Value::Ident(ident) = value.value.as_ref().unwrap() {
                    builder.registry.set_value_type(ident, ty, Some(&name));
                }

                let ty = builder.registry.value_type(&value);

                builder.registry.set_value_type(&name, ty.clone(), None);
                self.registry.set_value_type(&name, ty.clone(), None);

                // Names can't repeat between data implementations, because they will probably be
                // all implemented in the same scope
                self.registry.append_names(&builder.registry);

                self.symbols.push(lex::Symbol {
                    symbol: Some(lex::symbol::Symbol::DataDecl(lex::DataDecl {
                        name,
                        r#type: ty,
                        body: builder.finish(),
                    })),
                });
            }
            _ => {
                // FIXME: melhorar tratamento de erro
                unreachable!()
            }
        };
    }

    pub fn finish(self) -> lex::Module {
        lex::Module {
            name: self.name,
            symbols: self.symbols,
        }
    }
}

struct InstrBuilder {
    registry: Registry,
    body: Vec<lex::Instr>,
}

impl InstrBuilder {
    pub fn new(parent_registry: &Registry) -> Self {
        InstrBuilder {
            registry: Registry::with_parent(parent_registry),
            body: Vec::new(),
        }
    }

    pub fn finish(mut self) -> Vec<lex::Instr> {
        let name_value = |name: &str| lex::Value {
            value: Some(lex::value::Value::Ident(name.to_string())),
        };

        for inst in self.body.iter_mut() {
            // Instructions with a type should have their type loaded from the registry
            match inst.instr.as_mut() {
                Some(lex::instr::Instr::BinOp(binop)) => {
                    let value = name_value(&binop.name);
                    binop.r#type = self.registry.value_type(&value);
                }
                Some(lex::instr::Instr::FnCall(fncall)) => {
                    let value = name_value(&fncall.name);
                    fncall.r#type = self.registry.value_type(&value);
                }
                Some(lex::instr::Instr::Assign(assign)) => {
                    let value = name_value(&assign.name);
                    assign.r#type = self.registry.value_type(&value);
                }
                _ => {}
            }
        }

        self.body
    }

    pub fn add_stmt(&mut self, node: &ast::Stmt) -> (lex::Value, lex::Type) {
        match &node.stmt {
            Some(ast::stmt::Stmt::Var(var)) => {
                let ast::pat::Pat::Name(name_pat) = var.pat.pat.as_ref().unwrap();
                let name = self.registry.use_name(Some(name_pat));

                let (value, ty) = self.add_expr(&var.value, Some(&name));

                if let Some(ty) = var.r#type.as_ref() {
                    let ty = self.parse_type(ty);
                    self.registry.set_value_type(&name, ty, None);
                }

                (value, ty)
            }
            Some(ast::stmt::Stmt::Fn(_)) => {
                todo!()
            }
            None => {
                // FIXME: melhorar tratamento de erro
                unreachable!()
            }
        }
    }

    pub fn add_expr(&mut self, expr: &ast::Expr, name: Option<&str>) -> (lex::Value, lex::Type) {
        match expr.expr.as_ref().unwrap() {
            ast::expr::Expr::Num(num) => {
                let mut value = lex::Value {
                    value: Some(lex::value::Value::Num(num.clone())),
                };
                // TODO: improve type handling
                let ty = self.registry.value_type(&value);

                if let Some(name) = name {
                    value = self.assign(name, value, ty.clone());
                }

                (value, ty)
            }
            ast::expr::Expr::BinOp(op) => {
                let (left, left_ty) = self.add_expr(&op.left, None);
                let (right, right_ty) = self.add_expr(&op.right, None);

                // This will be implemented with typeclasses and generics
                // so + will be like `for T: Sum fn(T, T): T`
                // but none of this is implemented yet so we will use the number types instead, with
                // one signature for each type
                let num_ty = num_type("0");
                let ty = merge_types([&num_ty, &left_ty, &right_ty]).unwrap();
                let sigs = types_iter(&ty).map(binop_sig);

                let name = name.map_or_else(|| self.registry.use_name(None), |v| v.to_string());

                self.registry.insert_value_type(
                    &name,
                    ty.clone(),
                    ValueTypeDeps {
                        refs: vec![left.clone(), right.clone()],
                        sig: sigs.collect(),
                    },
                );

                self.body.push(lex::Instr {
                    instr: Some(lex::instr::Instr::BinOp(lex::BinOp {
                        name: name.clone(),
                        r#type: ty.clone(),
                        op: match op.op() {
                            ast::BinOpType::Add => lex::BinOpType::Add,
                            ast::BinOpType::Sub => lex::BinOpType::Sub,
                            ast::BinOpType::Mod => lex::BinOpType::Mod,
                            ast::BinOpType::Mul => lex::BinOpType::Mul,
                            ast::BinOpType::Div => lex::BinOpType::Div,
                            ast::BinOpType::Pow => lex::BinOpType::Pow,
                        }
                        .into(),
                        left,
                        right,
                    })),
                });

                let value = lex::Value {
                    value: Some(lex::value::Value::Ident(name)),
                };

                (value, ty)
            }
            ast::expr::Expr::Ident(ident) => {
                let internal_name = self.registry.get_internal_name(ident).unwrap();

                let mut value = lex::Value {
                    value: Some(lex::value::Value::Ident(internal_name.to_string())),
                };

                let ty = self.registry.value_type(&value);

                if let Some(name) = name {
                    value = self.assign(name, value, ty.clone());
                }

                (value, ty)
            }
            ast::expr::Expr::FnCall(call) => {
                let mut args = Vec::new();
                for arg in call.args.iter() {
                    let (value, _) = self.add_expr(arg, None);
                    args.push(value);
                }

                let (callee, callee_ty) = self.add_expr(&call.callee, None);
                let callee_name = match callee.value {
                    Some(lex::value::Value::Ident(name)) => {
                        self.registry.get_internal_name(&name).unwrap().to_string()
                    }
                    _ => {
                        // TODO: improve error handling
                        unreachable!()
                    }
                };

                let fn_sigs: Vec<_> = types_iter(&callee_ty)
                    .filter_map(|ty| {
                        if let Some(lex::r#type::Type::Fn(fn_ty)) = ty.r#type.as_ref() {
                            Some(fn_ty.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                let ret_ty = ambig(
                    fn_sigs
                        .clone()
                        .into_iter()
                        // TODO: many return values
                        .map(|mut fn_ty| fn_ty.ret.remove(0)),
                );

                let name = name.map_or_else(|| self.registry.use_name(None), |v| v.to_string());

                self.registry.insert_value_type(
                    &name,
                    ret_ty.clone(),
                    ValueTypeDeps {
                        refs: args.clone(),
                        sig: fn_sigs,
                    },
                );

                self.body.push(lex::Instr {
                    instr: Some(lex::instr::Instr::FnCall(lex::FnCall {
                        name: name.clone(),
                        r#type: ret_ty.clone(),
                        callee: callee_name,
                        args,
                    })),
                });

                let value = lex::Value {
                    value: Some(lex::value::Value::Ident(name)),
                };

                (value, ret_ty.clone())
            }
            ast::expr::Expr::Block(block) => {
                for stmt in block.body.iter() {
                    self.add_stmt(stmt);
                }

                self.add_expr(&block.ret, name)
            }
            ast::expr::Expr::FnExpr(_) => {
                todo!()
            }
        }
    }

    pub fn parse_type(&mut self, ty: &ast::Type) -> lex::Type {
        match ty.r#type.as_ref() {
            Some(ast::r#type::Type::Ident(indent)) => {
                match indent.as_str() {
                    "i8" => primitive!(I8),
                    "i16" => primitive!(I16),
                    "i32" => primitive!(I32),
                    "i64" => primitive!(I64),
                    "u8" => primitive!(U8),
                    "u16" => primitive!(U16),
                    "u32" => primitive!(U32),
                    "u64" => primitive!(U64),
                    "usize" => primitive!(USize),
                    "f32" => primitive!(F32),
                    "f64" => primitive!(F64),
                    _ => {
                        // TODO: improve error handling
                        panic!("{} is not a type, dummy", indent);
                    }
                }
            }
            None => {
                unreachable!()
            }
        }
    }

    pub fn assign(&mut self, name: &str, value: lex::Value, ty: lex::Type) -> lex::Value {
        self.body.push(lex::Instr {
            instr: Some(lex::instr::Instr::Assign(lex::Assign {
                name: name.to_string(),
                r#type: ty.clone(),
                value,
            })),
        });

        self.registry
            .insert_value_type(&name, ty.clone(), ValueTypeDeps::default());

        lex::Value {
            value: Some(lex::value::Value::Ident(name.to_string())),
        }
    }
}