pub mod ast;
pub mod codegen;
pub mod diagnostic;
pub mod formatter;
pub mod parser;
pub mod semantic;
pub mod typecheck;

pub use diagnostic::{
    Diagnostic, DiagnosticFix, DiagnosticLabel, DiagnosticStage, SourceId, SourceSpan,
    diagnostics_to_json,
};

pub fn compile_to_c(source: &str) -> Result<String, Diagnostic> {
    compile_to_c_with_source(source, SourceId::UNKNOWN)
}

pub fn compile_to_c_with_source(source: &str, source_id: SourceId) -> Result<String, Diagnostic> {
    let database =
        semantic::SemanticDatabase::analyze(source, source_id).map_err(first_diagnostic)?;
    codegen::emit_c(database.program(), database.signatures())
}

pub fn check_source(source: &str) -> Result<(), Diagnostic> {
    check_source_all(source).map_err(first_diagnostic)
}

pub fn check_source_with_id(source: &str, source_id: SourceId) -> Result<(), Diagnostic> {
    check_source_all_with_id(source, source_id).map_err(first_diagnostic)
}

pub fn check_source_all(source: &str) -> Result<(), Vec<Diagnostic>> {
    check_source_all_with_id(source, SourceId::UNKNOWN)
}

pub fn check_source_all_with_id(source: &str, source_id: SourceId) -> Result<(), Vec<Diagnostic>> {
    semantic::SemanticDatabase::analyze(source, source_id).map(|_| ())
}

fn first_diagnostic(diagnostics: Vec<Diagnostic>) -> Diagnostic {
    diagnostics
        .into_iter()
        .next()
        .expect("batch checking always returns at least one diagnostic on failure")
}
