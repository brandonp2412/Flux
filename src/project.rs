use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};
use crate::{codegen, parser, typecheck};

#[derive(Debug, Clone)]
pub struct ProjectSource {
    pub path: PathBuf,
    pub source_id: SourceId,
}

#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    pub program: Program,
    pub signatures: typecheck::Signatures,
    pub sources: Vec<ProjectSource>,
}

pub fn load(entry: &Path) -> Result<(Program, Vec<ProjectSource>), Vec<Diagnostic>> {
    let entry = canonical_entry(entry)?;
    let mut loader = Loader {
        loaded: HashSet::new(),
        stack: Vec::new(),
        program: Program::default(),
        sources: Vec::new(),
        diagnostics: Vec::new(),
    };
    loader.load_file(&entry, None);
    if loader.diagnostics.is_empty() {
        Ok((loader.program, loader.sources))
    } else {
        Err(loader.diagnostics)
    }
}

pub fn analyze(entry: &Path) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    let (program, sources) = load(entry)?;
    let signatures = typecheck::check_all(&program)?;
    Ok(ProjectAnalysis {
        program,
        signatures,
        sources,
    })
}

pub fn check(entry: &Path) -> Result<(), Vec<Diagnostic>> {
    analyze(entry).map(|_| ())
}

pub fn compile_to_c(entry: &Path) -> Result<String, Diagnostic> {
    let analysis = analyze(entry).map_err(first_diagnostic)?;
    codegen::emit_c(&analysis.program, &analysis.signatures)
}

fn canonical_entry(entry: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    fs::canonicalize(entry).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("failed to read entry source '{}': {error}", entry.display()),
        )]
    })
}

struct Loader {
    loaded: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
    program: Program,
    sources: Vec<ProjectSource>,
    diagnostics: Vec<Diagnostic>,
}

impl Loader {
    fn load_file(&mut self, path: &Path, via: Option<SourceSpan>) {
        let canonical = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) => {
                self.diagnostics.push(import_diagnostic(
                    via,
                    format!(
                        "failed to read imported source '{}': {error}",
                        path.display()
                    ),
                ));
                return;
            }
        };
        if self.loaded.contains(&canonical) {
            return;
        }
        if let Some(index) = self.stack.iter().position(|entry| entry == &canonical) {
            let mut cycle = self.stack[index..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(canonical.display().to_string());
            self.diagnostics.push(
                import_diagnostic(via, "cyclic import detected".to_string())
                    .with_note(format!("import cycle: {}", cycle.join(" -> "))),
            );
            return;
        }

        let source = match fs::read_to_string(&canonical) {
            Ok(source) => source,
            Err(error) => {
                self.diagnostics.push(import_diagnostic(
                    via,
                    format!(
                        "failed to read imported source '{}': {error}",
                        canonical.display()
                    ),
                ));
                return;
            }
        };
        let source_id = SourceId::from_name(canonical.to_string_lossy().as_ref());
        let parsed = match parser::parse_all_with_source(&source, source_id) {
            Ok(program) => program,
            Err(mut diagnostics) => {
                self.diagnostics.append(&mut diagnostics);
                return;
            }
        };

        self.stack.push(canonical.clone());
        for import in &parsed.imports {
            let import_path = Path::new(&import.path);
            if import_path.is_absolute() {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    import.path_span,
                    "imports must use relative paths",
                ));
                continue;
            }
            if import_path.extension().and_then(|value| value.to_str()) != Some("flux") {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    import.path_span,
                    "import paths must end in '.flux'",
                ));
                continue;
            }
            let parent = canonical.parent().unwrap_or_else(|| Path::new("."));
            self.load_file(&parent.join(import_path), Some(import.path_span));
        }
        self.stack.pop();

        if self.diagnostics.is_empty() || !self.loaded.contains(&canonical) {
            self.merge_program(parsed);
            self.loaded.insert(canonical.clone());
            self.sources.push(ProjectSource {
                path: canonical,
                source_id,
            });
        }
    }

    fn merge_program(&mut self, mut program: Program) {
        self.program.imports.append(&mut program.imports);
        self.program.aliases.append(&mut program.aliases);
        self.program.interfaces.append(&mut program.interfaces);
        self.program
            .implementations
            .append(&mut program.implementations);
        self.program.structs.append(&mut program.structs);
        self.program.enums.append(&mut program.enums);
        self.program.constants.append(&mut program.constants);
        self.program.functions.append(&mut program.functions);
    }
}

fn import_diagnostic(span: Option<SourceSpan>, message: String) -> Diagnostic {
    match span {
        Some(span) => Diagnostic::new(DiagnosticStage::Parse, span, message),
        None => Diagnostic::global(DiagnosticStage::Parse, message),
    }
}

fn first_diagnostic(diagnostics: Vec<Diagnostic>) -> Diagnostic {
    diagnostics
        .into_iter()
        .next()
        .expect("project analysis returns diagnostics on failure")
}
