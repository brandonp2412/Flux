use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};
use crate::{codegen, parser, typecheck};

#[derive(Debug, Clone)]
pub struct ProjectSource {
    pub path: PathBuf,
    pub source_id: SourceId,
    pub module_name: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    pub program: Program,
    pub signatures: typecheck::Signatures,
    pub sources: Vec<ProjectSource>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectAnalysisCacheStats {
    pub hits: usize,
    pub misses: usize,
}

#[derive(Debug, Clone)]
struct CachedProjectAnalysis {
    analysis: ProjectAnalysis,
    manifest_text: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModuleParseCacheStats {
    pub hits: usize,
    pub misses: usize,
}

#[derive(Debug, Clone)]
struct CachedModuleParse {
    text: String,
    program: Program,
}

#[derive(Debug, Default)]
struct ModuleParseCache {
    entries: HashMap<PathBuf, CachedModuleParse>,
    hits: usize,
    misses: usize,
}

impl ModuleParseCache {
    fn parse(
        &mut self,
        path: &Path,
        source: &str,
        source_id: SourceId,
    ) -> Result<Program, Vec<Diagnostic>> {
        if let Some(entry) = self.entries.get(path)
            && entry.text == source
        {
            self.hits += 1;
            return Ok(entry.program.clone());
        }
        self.misses += 1;
        let program = parser::parse_all_with_source(source, source_id)?;
        self.entries.insert(
            path.to_path_buf(),
            CachedModuleParse {
                text: source.to_string(),
                program: program.clone(),
            },
        );
        Ok(program)
    }

    const fn stats(&self) -> ModuleParseCacheStats {
        ModuleParseCacheStats {
            hits: self.hits,
            misses: self.misses,
        }
    }
}

#[derive(Debug, Default)]
pub struct ProjectAnalysisCache {
    entries: HashMap<PathBuf, CachedProjectAnalysis>,
    module_parses: ModuleParseCache,
    hits: usize,
    misses: usize,
}

impl ProjectAnalysisCache {
    pub fn analyze_with_overlays(
        &mut self,
        target: &Path,
        overlays: &HashMap<PathBuf, String>,
    ) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
        let key = cache_target_key(target)?;
        if let Some(entry) = self.entries.get(&key)
            && cached_analysis_is_current(&key, entry, overlays)
        {
            self.hits += 1;
            return Ok(entry.analysis.clone());
        }

        self.misses += 1;
        let analysis =
            analyze_with_overlays_and_parse_cache(target, overlays, &mut self.module_parses)?;
        self.entries.insert(
            key.clone(),
            CachedProjectAnalysis {
                analysis: analysis.clone(),
                manifest_text: manifest_snapshot(&key),
            },
        );
        Ok(analysis)
    }

    pub fn invalidate_path(&mut self, path: &Path) {
        let Ok(path) = fs::canonicalize(path) else {
            return;
        };
        self.entries.retain(|target, entry| {
            target != &path
                && !entry
                    .analysis
                    .sources
                    .iter()
                    .any(|source| source.path == path)
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub const fn stats(&self) -> ProjectAnalysisCacheStats {
        ProjectAnalysisCacheStats {
            hits: self.hits,
            misses: self.misses,
        }
    }

    pub const fn module_parse_stats(&self) -> ModuleParseCacheStats {
        self.module_parses.stats()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: Option<String>,
    pub entry: PathBuf,
    pub path: PathBuf,
}

fn cache_target_key(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    let candidate = if target.is_dir() {
        target.join("flux.toml")
    } else {
        target.to_path_buf()
    };
    fs::canonicalize(&candidate).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to resolve project target '{}': {error}",
                target.display()
            ),
        )]
    })
}

fn manifest_snapshot(target: &Path) -> Option<String> {
    (target.file_name().and_then(|name| name.to_str()) == Some("flux.toml"))
        .then(|| fs::read_to_string(target).ok())
        .flatten()
}

fn cached_analysis_is_current(
    target: &Path,
    entry: &CachedProjectAnalysis,
    overlays: &HashMap<PathBuf, String>,
) -> bool {
    if manifest_snapshot(target) != entry.manifest_text {
        return false;
    }
    entry.analysis.sources.iter().all(|source| {
        overlays.get(&source.path).map_or_else(
            || fs::read_to_string(&source.path).is_ok_and(|text| text == source.text),
            |overlay| overlay == &source.text,
        )
    })
}

pub fn load(target: &Path) -> Result<(Program, Vec<ProjectSource>), Vec<Diagnostic>> {
    load_with_overlays(target, &HashMap::new())
}

pub fn load_with_overlays(
    target: &Path,
    overlays: &HashMap<PathBuf, String>,
) -> Result<(Program, Vec<ProjectSource>), Vec<Diagnostic>> {
    let report = load_report_with_overlays(target, overlays)?;
    if report.diagnostics.is_empty() {
        Ok((report.program, report.sources))
    } else {
        Err(report.diagnostics)
    }
}

struct ProjectLoadReport {
    program: Program,
    sources: Vec<ProjectSource>,
    diagnostics: Vec<Diagnostic>,
}

fn load_report_with_overlays(
    target: &Path,
    overlays: &HashMap<PathBuf, String>,
) -> Result<ProjectLoadReport, Vec<Diagnostic>> {
    load_report_with_overlays_and_parse_cache(target, overlays, None)
}

fn load_report_with_overlays_and_parse_cache(
    target: &Path,
    overlays: &HashMap<PathBuf, String>,
    parse_cache: Option<&mut ModuleParseCache>,
) -> Result<ProjectLoadReport, Vec<Diagnostic>> {
    let (entry, module_root, package_name) = resolve_project_target(target)?;
    let mut loader = Loader {
        loaded: HashSet::new(),
        stack: Vec::new(),
        program: Program::default(),
        sources: Vec::new(),
        diagnostics: Vec::new(),
        module_root,
        package_name,
        overlays: overlays.clone(),
        parse_cache,
    };
    loader.load_file(&entry, None);
    Ok(ProjectLoadReport {
        program: loader.program,
        sources: loader.sources,
        diagnostics: loader.diagnostics,
    })
}

pub fn analyze(entry: &Path) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    analyze_with_overlays(entry, &HashMap::new())
}

pub fn analyze_with_overlays(
    entry: &Path,
    overlays: &HashMap<PathBuf, String>,
) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    analyze_with_overlays_report(load_report_with_overlays(entry, overlays)?)
}

fn analyze_with_overlays_and_parse_cache(
    entry: &Path,
    overlays: &HashMap<PathBuf, String>,
    parse_cache: &mut ModuleParseCache,
) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    analyze_with_overlays_report(load_report_with_overlays_and_parse_cache(
        entry,
        overlays,
        Some(parse_cache),
    )?)
}

fn analyze_with_overlays_report(
    report: ProjectLoadReport,
) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    if !report.diagnostics.is_empty() {
        return Err(report.diagnostics);
    }
    let signatures = typecheck::check_all(&report.program)?;
    Ok(ProjectAnalysis {
        program: report.program,
        signatures,
        sources: report.sources,
    })
}

pub fn check(entry: &Path) -> Result<(), Vec<Diagnostic>> {
    analyze(entry).map(|_| ())
}

pub fn check_with_sources(entry: &Path) -> (Vec<Diagnostic>, Vec<ProjectSource>) {
    check_with_overlays(entry, &HashMap::new())
}

pub fn check_with_overlays(
    entry: &Path,
    overlays: &HashMap<PathBuf, String>,
) -> (Vec<Diagnostic>, Vec<ProjectSource>) {
    let (program, sources) = match load_report_with_overlays(entry, overlays) {
        Ok(report) => {
            if !report.diagnostics.is_empty() {
                return (report.diagnostics, report.sources);
            }
            (report.program, report.sources)
        }
        Err(diagnostics) => return (diagnostics, Vec::new()),
    };
    match typecheck::check_all(&program) {
        Ok(_) => (Vec::new(), sources),
        Err(diagnostics) => (diagnostics, sources),
    }
}

pub fn compile_to_c(entry: &Path) -> Result<String, Diagnostic> {
    let analysis = analyze(entry).map_err(first_diagnostic)?;
    codegen::emit_c(&analysis.program, &analysis.signatures)
}

pub fn resolve_entry(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    resolve_project_target(target).map(|(entry, _, _)| entry)
}

fn resolve_project_target(
    target: &Path,
) -> Result<(PathBuf, PathBuf, Option<String>), Vec<Diagnostic>> {
    if target.is_dir() || target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        let manifest_path = if target.is_dir() {
            target.join("flux.toml")
        } else {
            target.to_path_buf()
        };
        let manifest = read_manifest(&manifest_path)?;
        let root = manifest
            .path
            .parent()
            .expect("canonical manifest path has a parent")
            .to_path_buf();
        return Ok((manifest.entry, root, Some(manifest.name)));
    }
    let entry = canonical_source(target, "entry source")?;
    let root = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok((entry, root, None))
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

struct Loader<'a> {
    loaded: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
    program: Program,
    sources: Vec<ProjectSource>,
    diagnostics: Vec<Diagnostic>,
    module_root: PathBuf,
    package_name: Option<String>,
    overlays: HashMap<PathBuf, String>,
    parse_cache: Option<&'a mut ModuleParseCache>,
}

impl Loader<'_> {
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
        if self.sources.iter().any(|entry| entry.path == canonical) {
            return;
        }

        let source = if let Some(source) = self.overlays.get(&canonical) {
            source.clone()
        } else {
            match fs::read_to_string(&canonical) {
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
            }
        };
        let source_id = SourceId::from_name(canonical.to_string_lossy().as_ref());
        let module_name = self.module_name(&canonical);
        self.sources.push(ProjectSource {
            path: canonical.clone(),
            source_id,
            module_name,
            text: source.clone(),
        });
        let parsed = if let Some(cache) = self.parse_cache.as_deref_mut() {
            cache.parse(&canonical, &source, source_id)
        } else {
            parser::parse_all_with_source(&source, source_id)
        };
        let mut parsed = match parsed {
            Ok(program) => program,
            Err(mut diagnostics) => {
                self.diagnostics.append(&mut diagnostics);
                return;
            }
        };

        self.stack.push(canonical.clone());
        for import in &mut parsed.imports {
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
            if import_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            }) {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    import.path_span,
                    "import paths must be normalized and cannot contain '.' or '..' segments",
                ));
                continue;
            }
            let parent = canonical.parent().unwrap_or_else(|| Path::new("."));
            let resolved = parent.join(import_path);
            let resolved_canonical = fs::canonicalize(&resolved).ok();
            if self.package_name.is_some()
                && resolved_canonical
                    .as_ref()
                    .is_some_and(|path| !path.starts_with(&self.module_root))
            {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    import.path_span,
                    "package imports must remain inside the package root",
                ));
                continue;
            }
            if let Some(target) = &resolved_canonical {
                import.resolved_source_id =
                    Some(SourceId::from_name(target.to_string_lossy().as_ref()));
            }
            self.load_file(&resolved, Some(import.path_span));
        }
        self.stack.pop();

        if self.diagnostics.is_empty() || !self.loaded.contains(&canonical) {
            self.merge_program(parsed);
            self.loaded.insert(canonical);
        }
    }

    fn module_name(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.module_root).unwrap_or(path);
        let mut segments = relative
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if let Some(last) = segments.last_mut()
            && let Some(stem) = last.strip_suffix(".flux")
        {
            *last = stem.to_string();
        }
        let local = segments.join("::");
        match &self.package_name {
            Some(package) if !local.is_empty() => format!("{package}::{local}"),
            Some(package) => package.clone(),
            None => local,
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
        self.program.views.append(&mut program.views);
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
