mod binary;
mod traits;

use target_lexicon::Triple;
use traits::Codegen;

use crate::proto::lex;

pub fn compile_program(program: &lex::Module) {
    // TODO: get the target from some kind of configuration
    let triple = Triple::host();
    let mut codegen = binary::BinaryCodegen::new(triple, program.name.clone());

    for symbol in program.symbols.iter() {
        match symbol.symbol.as_ref() {
            Some(lex::symbol::Symbol::FnDecl(fn_decl)) => {
                codegen.declare_function(fn_decl);
            }
            Some(lex::symbol::Symbol::DataDecl(data_decl)) => {
                codegen.declare_data(data_decl);
            }
            _ => {
                unreachable!();
            }
        }
    }

    for symbol in program.symbols.iter() {
        if let Some(lex::symbol::Symbol::FnDecl(fn_decl)) = symbol.symbol.as_ref() {
            codegen.build_function(fn_decl);
        }
    }

    codegen.write_to_file(&program.name);

    println!("Compiled program to {}", &program.name);
}
