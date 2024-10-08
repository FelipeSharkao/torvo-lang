use std::fmt;

use derive_more::{Display, From};
use derive_new::new;
use thiserror::Error;

use crate::{bytecode as b, context, utils};

#[derive(Debug, Clone, Error, new)]
#[error("{}:{}:{}: error: {detail}", loc.source_idx, loc.start_line, loc.start_col)]
pub struct Error {
    detail: ErrorDetail,
    loc: b::Loc,
}

#[derive(Debug, Clone, Error, new)]
pub struct DisplayError<'a>(&'a context::BuildContext, &'a Error);
impl fmt::Display for DisplayError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let DisplayError(ctx, err) = self;

        let idx = err.loc.source_idx;
        let src = ctx.source(idx);

        let line = err.loc.start_line;
        let col = err.loc.start_col;
        writeln!(f, "{}:{line}:{col}", src.path.display())?;

        let num = format!("{line}");
        let line_content = src.content().line(line).expect("line should be valid");
        writeln!(f, "{} |", " ".repeat(num.len()))?;
        writeln!(f, "{num} | {line_content}",)?;
        writeln!(f, "{} | {}^", " ".repeat(num.len()), " ".repeat(col - 1))?;
        writeln!(f, "error: {}", err.detail)?;

        Ok(())
    }
}

#[derive(Debug, Clone, Display, From)]
pub enum ErrorDetail {
    ValueNotFound(ValueNotFound),
    TypeNotFound(TypeNotFound),
    UnexpectedType(UnexpectedType),
    TypeMisatch(TypeMisatch),
    #[display("Type should be known at this point")]
    TypeNotFinal,
    Todo(Todo),
}

#[derive(Debug, Clone, Display, new)]
#[display("Cannot find value `{ident}` on the current scope")]
pub struct ValueNotFound {
    pub ident: String,
}

#[derive(Debug, Clone, Display, new)]
#[display("Cannot find type `{ident}` on the current scope")]
pub struct TypeNotFound {
    pub ident: String,
}

#[derive(Debug, Clone, Display, new)]
#[display("Expected type {}, found {}", &expected.body, &actual.body)]
pub struct UnexpectedType {
    pub expected: b::Type,
    pub actual: b::Type,
}

#[derive(Debug, Clone, Display, new)]
#[display(
    "All results of the expression should have the same type\n{}",
    utils::indented(2, types.iter().map(|t| format!("- found {t}"))),
)]
pub struct TypeMisatch {
    pub types: Vec<b::Type>,
}

#[derive(Debug, Clone, Display, new)]
#[display("Feature is not implemented yet: {feature}")]
pub struct Todo {
    pub feature: String,
}
