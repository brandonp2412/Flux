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
    check_source_all(source).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("check_source_all always returns at least one diagnostic on failure")
    })
}

pub fn check_source_all(source: &str) -> Result<(), Vec<Diagnostic>> {
    let program = parser::parse_all(source)?;
    typecheck::check_all(&program)?;
    Ok(())
}
