pub mod ast;
pub mod codegen;
pub mod diagnostic;
pub mod parser;
pub mod typecheck;

pub use diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};

pub fn compile_to_c(source: &str) -> Result<String, Diagnostic> {
    let program = parser::parse(source)?;
    let signatures = typecheck::check(&program)?;
    codegen::emit_c(&program, &signatures)
}

pub fn compile_to_c_with_source(source: &str, source_id: SourceId) -> Result<String, Diagnostic> {
    let program = parser::parse_with_source(source, source_id)?;
    let signatures = typecheck::check(&program)?;
    codegen::emit_c(&program, &signatures)
}

pub fn check_source(source: &str) -> Result<(), Diagnostic> {
    check_source_all(source).map_err(first_diagnostic)
}

pub fn check_source_with_id(source: &str, source_id: SourceId) -> Result<(), Diagnostic> {
    check_source_all_with_id(source, source_id).map_err(first_diagnostic)
}

pub fn check_source_all(source: &str) -> Result<(), Vec<Diagnostic>> {
    let program = parser::parse_all(source)?;
    typecheck::check_all(&program)?;
    Ok(())
}

pub fn check_source_all_with_id(source: &str, source_id: SourceId) -> Result<(), Vec<Diagnostic>> {
    let program = parser::parse_all_with_source(source, source_id)?;
    typecheck::check_all(&program)?;
    Ok(())
}

fn first_diagnostic(diagnostics: Vec<Diagnostic>) -> Diagnostic {
    diagnostics
        .into_iter()
        .next()
        .expect("batch checking always returns at least one diagnostic on failure")
}
