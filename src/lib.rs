pub mod android_bindings;
pub mod ast;
pub mod builtin_names;
pub mod cli;
pub mod codegen;
pub mod diagnostic;
pub mod formatter;
pub mod ir;
pub mod linux_bindings;
pub mod lsp;
pub mod package_ecosystem;
pub mod parser;
pub mod project;
pub mod semantic;
pub mod terminal;
pub mod typecheck;
pub mod web;
pub mod windows_bindings;

pub use codegen::{C_ABI_VERSION_PACKAGE, NATIVE_ABI_POLICY_VERSION};
pub use diagnostic::{
    DIAGNOSTIC_JSON_SCHEMA_VERSION, Diagnostic, DiagnosticFix, DiagnosticLabel, DiagnosticStage,
    SourceId, SourceSpan, diagnostics_envelope_to_json, diagnostics_to_json,
};
pub use parser::GRAMMAR_VERSION;
pub use terminal::{DiagnosticSource, TerminalRenderOptions, render_diagnostics};
pub use typecheck::UI_API_VERSION;

/// Compatibility version for Flux's source-level type and memory model.
///
/// The version covers the meaning of `Copy`, `Send`, moves, immutable borrows,
/// and the explicitly rejected ownership-sensitive shapes.  It is independent
/// of the parser, formatter, package, UI, and native ABI versions.
pub const TYPE_MEMORY_MODEL_VERSION: u32 = 1;

pub fn compile_to_c(source: &str) -> Result<String, Diagnostic> {
    compile_to_c_with_source(source, SourceId::UNKNOWN)
}

pub fn compile_to_c_with_source(source: &str, source_id: SourceId) -> Result<String, Diagnostic> {
    let database =
        semantic::SemanticDatabase::analyze(source, source_id).map_err(first_diagnostic)?;
    codegen::emit_c(database.program(), database.signatures())
}

pub fn compile_to_c_header(source: &str) -> Result<String, Diagnostic> {
    let database =
        semantic::SemanticDatabase::analyze(source, SourceId::UNKNOWN).map_err(first_diagnostic)?;
    codegen::emit_c_header(database.program(), database.signatures())
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

#[cfg(test)]
mod tests {
    use super::compile_to_c;

    #[test]
    fn directory_listing_validates_borrowed_entry_names() {
        let source = r#"
fn visit(_name: str) -> void {
}

fn main() -> i64 {
    print(directory.list(".", visit))
    return 0
}
"#;
        let generated = compile_to_c(source).expect("directory.list should compile");
        assert!(generated.contains("flux__fs_list_directory"));
        assert!(generated.contains("flux__fs_valid_utf8_name"));
        assert!(generated.contains("directory entry name is not valid UTF-8"));
    }

    #[test]
    fn batched_accept_helpers_validate_callbacks_at_the_native_boundary() {
        let source = r#"
fn accepted(_socket: i64) -> void {
}

fn main() -> i64 {
    let (_count, _failure) = net.acceptMany(1, 4, accepted)
    let (_timedCount, _ready, _timedFailure) = net.acceptManyTimeout(1, 4, 0, accepted)
    return 0
}
"#;
        let generated = compile_to_c(source).expect("batched accept helpers should compile");
        assert!(generated.contains("if (callback == NULL) return flux__net_result"));
        assert!(generated.contains("invalid acceptMany callback"));
        assert!(generated.contains("if (callback == NULL) return flux__net_progress_result"));
        assert!(generated.contains("invalid acceptManyTimeout callback"));
    }
}
