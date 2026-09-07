pub mod ast;
pub mod codegen;
pub mod diagnostic;
pub mod parser;
pub mod typecheck;

pub use diagnostic::{Diagnostic, DiagnosticStage, SourceSpan};

pub fn compile_to_c(source: &str) -> Result<String, Diagnostic> {
    let program = parser::parse(source)?;
    let signatures = typecheck::check(&program)?;
    codegen::emit_c(&program, &signatures)
}

pub fn check_source(source: &str) -> Result<(), Diagnostic> {
    let program = parser::parse(source)?;
    typecheck::check(&program)?;
    Ok(())
}
