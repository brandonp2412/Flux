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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: Option<String>,
    pub entry: PathBuf,
    pub path: PathBuf,
}

pub fn load(entry: &Path) -> Result<(Program, Vec<ProjectSource>), Vec<Diagnostic>> {
    let entry = resolve_entry(entry)?;
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

pub fn resolve_entry(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    if target.is_dir() {
        return read_manifest(&target.join("flux.toml")).map(|manifest| manifest.entry);
    }
    if target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        return read_manifest(target).map(|manifest| manifest.entry);
    }
    canonical_source(target, "entry source")
}

pub fn read_manifest(path: &Path) -> Result<PackageManifest, Vec<Diagnostic>> {
    let canonical_manifest = fs::canonicalize(path).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to read package manifest '{}': {error}",
                path.display()
            ),
        )]
    })?;
    let source = fs::read_to_string(&canonical_manifest).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to read package manifest '{}': {error}",
                canonical_manifest.display()
            ),
        )]
    })?;
    let source_id = SourceId::from_name(canonical_manifest.to_string_lossy().as_ref());
    let mut section = None::<String>;
    let mut name = None::<String>;
    let mut version = None::<String>;
    let mut entry = None::<String>;
    let mut diagnostics = Vec::new();

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line.len() < 3 {
                diagnostics.push(manifest_diagnostic(
                    source_id,
                    line_number,
                    "invalid manifest table declaration",
                ));
                continue;
            }
            let table = line[1..line.len() - 1].trim();
            if table != "package" {
                diagnostics.push(manifest_diagnostic(
                    source_id,
                    line_number,
                    format!("unsupported manifest table '[{table}]'"),
                ));
            }
            section = Some(table.to_string());
            continue;
        }
        if section.as_deref() != Some("package") {
            diagnostics.push(manifest_diagnostic(
                source_id,
                line_number,
                "manifest fields must be declared inside [package]",
            ));
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            diagnostics.push(manifest_diagnostic(
                source_id,
                line_number,
                "manifest fields use 'key = \"value\"' syntax",
            ));
            continue;
        };
        let key = key.trim();
        let value = match parse_manifest_string(raw_value.trim()) {
            Ok(value) => value,
            Err(message) => {
                diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                continue;
            }
        };
        let slot = match key {
            "name" => &mut name,
            "version" => &mut version,
            "entry" => &mut entry,
            _ => {
                diagnostics.push(manifest_diagnostic(
                    source_id,
                    line_number,
                    format!("unknown [package] field '{key}'"),
                ));
                continue;
            }
        };
        if slot.replace(value).is_some() {
            diagnostics.push(manifest_diagnostic(
                source_id,
                line_number,
                format!("duplicate [package] field '{key}'"),
            ));
        }
    }

    if name.as_deref().is_none_or(str::is_empty) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "package manifest requires a non-empty [package].name",
        ));
    }
    if version.as_deref().is_some_and(str::is_empty) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[package].version cannot be empty",
        ));
    }
    let Some(entry_value) = entry.filter(|entry| !entry.is_empty()) else {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "package manifest requires a non-empty [package].entry",
        ));
        return Err(diagnostics);
    };
    let entry_path = Path::new(&entry_value);
    if entry_path.is_absolute() {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[package].entry must be a relative path",
        ));
    }
    if entry_path.extension().and_then(|value| value.to_str()) != Some("flux") {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[package].entry must end in '.flux'",
        ));
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let package_root = canonical_manifest
        .parent()
        .expect("canonical manifest path has a parent");
    let canonical_entry = canonical_source(&package_root.join(entry_path), "package entry")?;
    if !canonical_entry.starts_with(package_root) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            "[package].entry must remain inside the package root",
        )]);
    }

    Ok(PackageManifest {
        name: name.expect("validated package name"),
        version,
        entry: canonical_entry,
        path: canonical_manifest,
    })
}

fn canonical_source(path: &Path, kind: &str) -> Result<PathBuf, Vec<Diagnostic>> {
    fs::canonicalize(path).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("failed to read {kind} '{}': {error}", path.display()),
        )]
    })
}

fn parse_manifest_string(text: &str) -> Result<String, String> {
    if !text.starts_with('"') {
        return Err("manifest values must be quoted strings".to_string());
    }
    let mut value = String::new();
    let mut escaped = false;
    let mut closing = None;
    for (index, ch) in text[1..].char_indices() {
        if escaped {
            match ch {
                '"' | '\\' => value.push(ch),
                'n' => value.push('\n'),
                't' => value.push('\t'),
                _ => return Err(format!("unsupported manifest string escape '\\{ch}'")),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                closing = Some(index + 2);
                break;
            }
            _ => value.push(ch),
        }
    }
    if escaped || closing.is_none() {
        return Err("unterminated manifest string".to_string());
    }
    let rest = text[closing.expect("checked closing quote")..].trim();
    if !rest.is_empty() && !rest.starts_with('#') {
        return Err("unexpected text after manifest string".to_string());
    }
    Ok(value)
}

fn manifest_diagnostic(source_id: SourceId, line: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        DiagnosticStage::Parse,
        SourceSpan::line(line).with_source(source_id),
        message,
    )
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
