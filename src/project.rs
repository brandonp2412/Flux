use std::collections::{BTreeMap, HashMap, HashSet};
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
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
}

impl ProjectAnalysis {
    pub fn emit_c(&self) -> Result<String, Diagnostic> {
        self.emit_c_for_target(codegen::NativeTarget::Linux)
    }

    pub fn emit_c_header(&self) -> Result<String, Diagnostic> {
        let source_modules = self
            .sources
            .iter()
            .map(|source| (source.source_id, source.module_name.clone()))
            .collect::<HashMap<_, _>>();
        codegen::emit_c_header_with_module_names(&self.program, &self.signatures, &source_modules)
    }

    pub fn emit_c_for_target(&self, target: codegen::NativeTarget) -> Result<String, Diagnostic> {
        let source_paths = self
            .sources
            .iter()
            .map(|source| (source.source_id, source.path.to_string_lossy().into_owned()))
            .collect::<HashMap<_, _>>();
        let source_modules = self
            .sources
            .iter()
            .map(|source| (source.source_id, source.module_name.clone()))
            .collect::<HashMap<_, _>>();
        codegen::emit_c_for_target_with_source_metadata(
            &self.program,
            &self.signatures,
            &source_paths,
            &source_modules,
            &self.translations,
            target,
        )
    }
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
pub struct AndroidPackageConfig {
    pub application_id: String,
    pub version_code: u32,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub permissions: Vec<String>,
    pub deep_links: Vec<String>,
    pub keystore: Option<PathBuf>,
    pub key_alias: Option<String>,
}

pub const PACKAGE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageDependency {
    Registry {
        requirement: String,
    },
    Path {
        path: PathBuf,
        requirement: Option<String>,
    },
    Git {
        url: String,
        rev: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub format_version: u32,
    pub name: String,
    pub version: Option<String>,
    pub entry: PathBuf,
    pub path: PathBuf,
    pub dependencies: BTreeMap<String, PackageDependency>,
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
    pub android: AndroidPackageConfig,
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
    translations: BTreeMap<String, BTreeMap<String, String>>,
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
    let (entry, module_root, package_name, dependencies, translations) =
        resolve_project_target(target)?;
    let package_scopes = package_name
        .as_ref()
        .map(|name| {
            vec![PackageScope {
                name: name.clone(),
                root: module_root.clone(),
                dependencies,
            }]
        })
        .unwrap_or_default();
    let mut loader = Loader {
        loaded: HashSet::new(),
        stack: Vec::new(),
        program: Program::default(),
        sources: Vec::new(),
        diagnostics: Vec::new(),
        module_root,
        package_scopes,
        overlays: overlays.clone(),
        parse_cache,
    };
    loader.load_file(&entry, None);
    Ok(ProjectLoadReport {
        program: loader.program,
        sources: loader.sources,
        diagnostics: loader.diagnostics,
        translations,
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
        translations: report.translations,
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
    analysis.emit_c()
}

pub fn compile_to_c_header(entry: &Path) -> Result<String, Diagnostic> {
    let analysis = analyze(entry).map_err(first_diagnostic)?;
    analysis.emit_c_header()
}

pub fn resolve_entry(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    resolve_project_target(target).map(|(entry, _, _, _, _)| entry)
}

pub fn development_status_path(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    let entry = resolve_entry(target)?;
    let source_id = SourceId::from_name(entry.to_string_lossy().as_ref());
    Ok(std::env::temp_dir().join(format!("fluxc-run-status-{}.json", source_id.value())))
}

fn resolve_project_target(
    target: &Path,
) -> Result<
    (
        PathBuf,
        PathBuf,
        Option<String>,
        BTreeMap<String, PackageDependency>,
        BTreeMap<String, BTreeMap<String, String>>,
    ),
    Vec<Diagnostic>,
> {
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
        return Ok((
            manifest.entry,
            root,
            Some(manifest.name),
            manifest.dependencies,
            manifest.translations,
        ));
    }
    let entry = canonical_source(target, "entry source")?;
    let root = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok((entry, root, None, BTreeMap::new(), BTreeMap::new()))
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
    let mut format_version = None::<u32>;
    let mut name = None::<String>;
    let mut version = None::<String>;
    let mut entry = None::<String>;
    let mut android_application_id = None::<String>;
    let mut android_version_code = None::<u32>;
    let mut android_min_sdk = None::<u32>;
    let mut android_target_sdk = None::<u32>;
    let mut android_permissions = None::<Vec<String>>;
    let mut android_deep_links = None::<Vec<String>>;
    let mut android_keystore = None::<String>;
    let mut android_key_alias = None::<String>;
    let mut dependencies = BTreeMap::<String, PackageDependency>::new();
    let mut translations = BTreeMap::<String, BTreeMap<String, String>>::new();
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
            if !matches!(
                table,
                "package" | "android" | "dependencies" | "translations"
            ) {
                diagnostics.push(manifest_diagnostic(
                    source_id,
                    line_number,
                    format!("unsupported manifest table '[{table}]'"),
                ));
            }
            section = Some(table.to_string());
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            diagnostics.push(manifest_diagnostic(
                source_id,
                line_number,
                "manifest fields use 'key = value' syntax",
            ));
            continue;
        };
        let key = key.trim();
        let raw_value = raw_value.trim();
        match section.as_deref() {
            Some("package") => {
                if key == "format_version" {
                    let value = match parse_manifest_u32(raw_value) {
                        Ok(value) => value,
                        Err(message) => {
                            diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                            continue;
                        }
                    };
                    if format_version.replace(value).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            "duplicate [package] field 'format_version'",
                        ));
                    }
                    continue;
                }
                let value = match parse_manifest_string(raw_value) {
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
            Some("dependencies") => {
                if !valid_dependency_name(key) {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!(
                            "invalid dependency name '{key}'; names use ASCII letters, digits, '.', '-', or '_' and must start with a letter or digit"
                        ),
                    ));
                    continue;
                }
                let dependency = match parse_package_dependency(raw_value) {
                    Ok(dependency) => dependency,
                    Err(message) => {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("invalid dependency '{key}': {message}"),
                        ));
                        continue;
                    }
                };
                if dependencies.insert(key.to_string(), dependency).is_some() {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!("duplicate [dependencies] entry '{key}'"),
                    ));
                }
            }
            Some("translations") => {
                if !valid_dependency_name(key) {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!(
                            "invalid translation key '{key}'; keys use ASCII letters, digits, '.', '-', or '_' and must start with a letter or digit"
                        ),
                    ));
                    continue;
                }
                let entries = match parse_manifest_string_array(raw_value) {
                    Ok(value) => value,
                    Err(message) => {
                        diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                        continue;
                    }
                };
                if translations.contains_key(key) {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!("duplicate [translations] entry '{key}'"),
                    ));
                    continue;
                }
                let mut localized = BTreeMap::new();
                for entry in entries {
                    let Some((locale, text)) = entry.split_once('=') else {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!(
                                "[translations].{key} entries use 'locale=text' strings; invalid value '{entry}'"
                            ),
                        ));
                        continue;
                    };
                    if !valid_translation_locale(locale) {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!(
                                "[translations].{key} has invalid locale '{locale}'; use a language tag such as 'en' or 'fr-CA'"
                            ),
                        ));
                        continue;
                    }
                    let locale_key = canonical_translation_locale(locale);
                    if localized.insert(locale_key, text.to_string()).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("[translations].{key} repeats locale '{locale}'"),
                        ));
                    }
                }
                translations.insert(key.to_string(), localized);
            }
            Some("android") => match key {
                "application_id" | "keystore" | "key_alias" => {
                    let value = match parse_manifest_string(raw_value) {
                        Ok(value) => value,
                        Err(message) => {
                            diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                            continue;
                        }
                    };
                    let slot = match key {
                        "application_id" => &mut android_application_id,
                        "keystore" => &mut android_keystore,
                        "key_alias" => &mut android_key_alias,
                        _ => unreachable!(),
                    };
                    if slot.replace(value).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("duplicate [android] field '{key}'"),
                        ));
                    }
                }
                "permissions" | "deep_links" => {
                    let value = match parse_manifest_string_array(raw_value) {
                        Ok(value) => value,
                        Err(message) => {
                            diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                            continue;
                        }
                    };
                    let slot = if key == "permissions" {
                        &mut android_permissions
                    } else {
                        &mut android_deep_links
                    };
                    if slot.replace(value).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("duplicate [android] field '{key}'"),
                        ));
                    }
                }
                "version_code" | "min_sdk" | "target_sdk" => {
                    let value = match parse_manifest_u32(raw_value) {
                        Ok(value) => value,
                        Err(message) => {
                            diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                            continue;
                        }
                    };
                    let slot = match key {
                        "version_code" => &mut android_version_code,
                        "min_sdk" => &mut android_min_sdk,
                        "target_sdk" => &mut android_target_sdk,
                        _ => unreachable!(),
                    };
                    if slot.replace(value).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("duplicate [android] field '{key}'"),
                        ));
                    }
                }
                _ => diagnostics.push(manifest_diagnostic(
                    source_id,
                    line_number,
                    format!("unknown [android] field '{key}'"),
                )),
            },
            _ => diagnostics.push(manifest_diagnostic(
                source_id,
                line_number,
                "manifest fields must be declared inside [package], [dependencies], [translations], or [android]",
            )),
        }
    }

    if format_version.is_some_and(|value| value != PACKAGE_FORMAT_VERSION) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "unsupported [package].format_version {}; this compiler supports version {}",
                format_version.expect("format version was checked"),
                PACKAGE_FORMAT_VERSION
            ),
        ));
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
    if let Some(application_id) = android_application_id.as_deref()
        && !valid_android_application_id(application_id)
    {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].application_id must be a lowercase reverse-DNS identifier such as 'nz.example.app'",
        ));
    }
    if android_version_code.is_some_and(|value| value == 0 || value > 2_100_000_000) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].version_code must be between 1 and 2100000000 for Google Play",
        ));
    }
    if android_min_sdk.is_some_and(|value| value < 21) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].min_sdk must be at least 21",
        ));
    }
    if android_target_sdk.is_some_and(|value| value < 21) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].target_sdk must be at least 21",
        ));
    }
    if let (Some(min_sdk), Some(target_sdk)) = (android_min_sdk, android_target_sdk)
        && target_sdk < min_sdk
    {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].target_sdk must be greater than or equal to min_sdk",
        ));
    }
    if let Some(permissions) = android_permissions.as_ref() {
        for permission in permissions {
            if !valid_android_permission(permission) {
                diagnostics.push(Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!(
                        "[android].permissions entries must be Android-style permission names such as 'android.permission.CAMERA'; invalid value '{permission}'"
                    ),
                ));
            }
        }
    }
    if let Some(deep_links) = android_deep_links.as_ref() {
        for deep_link in deep_links {
            if !valid_android_deep_link(deep_link) {
                diagnostics.push(Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!(
                        "[android].deep_links entries must be absolute URI prefixes such as 'https://example.com/app' or 'flux://open'; invalid value '{deep_link}'"
                    ),
                ));
            }
        }
    }
    if android_keystore.as_deref().is_some_and(str::is_empty) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].keystore cannot be empty",
        ));
    }
    if android_key_alias.as_deref().is_some_and(str::is_empty) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].key_alias cannot be empty",
        ));
    }
    if android_keystore.is_some() != android_key_alias.is_some() {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[android].keystore and [android].key_alias must be configured together",
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

    let name = name.expect("validated package name");
    let version_code = android_version_code.unwrap_or(1);
    let min_sdk = android_min_sdk.unwrap_or(23);
    let target_sdk = android_target_sdk.unwrap_or(36);
    let keystore = android_keystore.map(|path| {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            package_root.join(path)
        }
    });
    Ok(PackageManifest {
        format_version: format_version.unwrap_or(PACKAGE_FORMAT_VERSION),
        android: AndroidPackageConfig {
            application_id: android_application_id
                .unwrap_or_else(|| default_android_application_id(&name)),
            version_code,
            min_sdk,
            target_sdk,
            permissions: {
                let mut permissions = android_permissions.unwrap_or_default();
                permissions.sort();
                permissions.dedup();
                permissions
            },
            deep_links: {
                let mut deep_links = android_deep_links.unwrap_or_default();
                deep_links.sort();
                deep_links.dedup();
                deep_links
            },
            keystore,
            key_alias: android_key_alias,
        },
        name,
        version,
        entry: canonical_entry,
        path: canonical_manifest,
        dependencies,
        translations,
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

fn parse_package_dependency(text: &str) -> Result<PackageDependency, String> {
    let text = text.trim();
    if text.starts_with('"') {
        let requirement = parse_manifest_string(text)?;
        if !valid_semver_requirement(&requirement) {
            return Err(
                "registry dependencies require a SemVer requirement such as \"1.2.3\", \"^1.2.3\", \"~1.2.3\", or \"*\""
                    .to_string(),
            );
        }
        return Ok(PackageDependency::Registry { requirement });
    }

    let fields = parse_manifest_inline_string_table(text)?;
    if let Some(path) = fields.get("path") {
        if fields
            .keys()
            .any(|field| field != "path" && field != "version")
        {
            return Err(
                "path dependencies accept only 'path' and optional 'version' fields".to_string(),
            );
        }
        if path.is_empty() {
            return Err("path dependencies require a non-empty path".to_string());
        }
        let requirement = fields.get("version").cloned();
        if requirement
            .as_deref()
            .is_some_and(|requirement| !valid_semver_requirement(requirement))
        {
            return Err("path dependency 'version' must be a SemVer requirement".to_string());
        }
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Err("path dependencies must use a relative path".to_string());
        }
        return Ok(PackageDependency::Path { path, requirement });
    }

    if let Some(url) = fields.get("git") {
        if fields.len() != 2 || !fields.contains_key("rev") {
            return Err("Git dependencies require exactly 'git' and 'rev' fields".to_string());
        }
        let rev = fields.get("rev").expect("Git dependency rev was checked");
        if url.is_empty() || rev.is_empty() {
            return Err("Git dependency 'git' and 'rev' values cannot be empty".to_string());
        }
        return Ok(PackageDependency::Git {
            url: url.clone(),
            rev: rev.clone(),
        });
    }

    Err("dependencies must be a SemVer string, { path = \"...\" }, or { git = \"...\", rev = \"...\" }".to_string())
}

fn parse_manifest_inline_string_table(text: &str) -> Result<BTreeMap<String, String>, String> {
    let text = text.trim();
    if !text.starts_with('{') || !text.ends_with('}') {
        return Err("dependency tables must use { key = \"value\", ... } syntax".to_string());
    }
    let inner = text[1..text.len() - 1].trim();
    if inner.is_empty() {
        return Err("dependency tables cannot be empty".to_string());
    }

    let bytes = inner.as_bytes();
    let mut index = 0usize;
    let mut fields = BTreeMap::new();
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let key_start = index;
        while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
        {
            index += 1;
        }
        if key_start == index {
            return Err("dependency table fields require an identifier key".to_string());
        }
        let key = &inner[key_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            return Err(format!("dependency table field '{key}' requires '='"));
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) != Some(&b'"') {
            return Err(format!(
                "dependency table field '{key}' must be a quoted string"
            ));
        }
        let value_start = index;
        index += 1;
        let mut escaped = false;
        while index < bytes.len() {
            let byte = bytes[index];
            index += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if index > bytes.len() || bytes.get(index.saturating_sub(1)) != Some(&b'"') {
            return Err(format!("unterminated dependency table string for '{key}'"));
        }
        let value = parse_manifest_string(&inner[value_start..index])?;
        if fields.insert(key.to_string(), value).is_some() {
            return Err(format!("duplicate dependency table field '{key}'"));
        }
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        if bytes[index] != b',' {
            return Err("dependency table fields must be separated by commas".to_string());
        }
        index += 1;
        if inner[index..].trim().is_empty() {
            return Err("dependency tables may not end with a trailing comma".to_string());
        }
    }
    Ok(fields)
}

fn valid_dependency_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

fn valid_translation_locale(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    matches!(parts.len(), 1 | 2)
        && parts[0].len() >= 2
        && parts[0].len() <= 8
        && parts[0].chars().all(|ch| ch.is_ascii_alphabetic())
        && parts.get(1).is_none_or(|region| {
            (region.len() == 2 && region.chars().all(|ch| ch.is_ascii_alphabetic()))
                || (region.len() == 3 && region.chars().all(|ch| ch.is_ascii_digit()))
        })
}

fn canonical_translation_locale(value: &str) -> String {
    let mut parts = value.split('-');
    let language = parts
        .next()
        .expect("validated locale has a language")
        .to_ascii_lowercase();
    parts.next().map_or(language.clone(), |region| {
        format!("{language}-{}", region.to_ascii_uppercase())
    })
}

fn valid_semver_requirement(value: &str) -> bool {
    if value == "*" {
        return true;
    }
    let version = value
        .strip_prefix('^')
        .or_else(|| value.strip_prefix('~'))
        .unwrap_or(value);
    valid_semver_version(version)
}

fn valid_semver_version(value: &str) -> bool {
    let (without_build, build) = value
        .split_once('+')
        .map_or((value, None), |(version, build)| (version, Some(build)));
    if build.is_some_and(|build| !valid_semver_identifiers(build, false)) {
        return false;
    }
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(core, prerelease)| {
            (core, Some(prerelease))
        });
    if prerelease.is_some_and(|prerelease| !valid_semver_identifiers(prerelease, true)) {
        return false;
    }

    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    [major, minor, patch].into_iter().all(|part| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    })
}

fn valid_semver_identifiers(value: &str, reject_numeric_leading_zero: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (!reject_numeric_leading_zero
                    || !identifier.bytes().all(|byte| byte.is_ascii_digit())
                    || identifier == "0"
                    || !identifier.starts_with('0'))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticVersion {
    major: String,
    minor: String,
    patch: String,
    prerelease: Vec<String>,
}

impl SemanticVersion {
    fn parse(value: &str) -> Option<Self> {
        if !valid_semver_version(value) {
            return None;
        }
        let without_build = value.split_once('+').map_or(value, |(version, _)| version);
        let (core, prerelease) = without_build
            .split_once('-')
            .map_or((without_build, ""), |(core, prerelease)| (core, prerelease));
        let mut parts = core.split('.');
        Some(Self {
            major: parts.next()?.to_string(),
            minor: parts.next()?.to_string(),
            patch: parts.next()?.to_string(),
            prerelease: if prerelease.is_empty() {
                Vec::new()
            } else {
                prerelease.split('.').map(str::to_string).collect()
            },
        })
    }

    fn core_eq(&self, other: &Self) -> bool {
        self.major == other.major && self.minor == other.minor && self.patch == other.patch
    }

    fn precedence_cmp(&self, other: &Self) -> std::cmp::Ordering {
        for (left, right) in [
            (&self.major, &other.major),
            (&self.minor, &other.minor),
            (&self.patch, &other.patch),
        ] {
            let ordering = semver_numeric_cmp(left, right);
            if ordering != std::cmp::Ordering::Equal {
                return ordering;
            }
        }
        match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => {
                for (left, right) in self.prerelease.iter().zip(&other.prerelease) {
                    let left_numeric = left.bytes().all(|byte| byte.is_ascii_digit());
                    let right_numeric = right.bytes().all(|byte| byte.is_ascii_digit());
                    let ordering = match (left_numeric, right_numeric) {
                        (true, true) => semver_numeric_cmp(left, right),
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        (false, false) => left.cmp(right),
                    };
                    if ordering != std::cmp::Ordering::Equal {
                        return ordering;
                    }
                }
                self.prerelease.len().cmp(&other.prerelease.len())
            }
        }
    }
}

fn semver_numeric_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn increment_semver_number(value: &str) -> String {
    let mut digits = value.bytes().collect::<Vec<_>>();
    let mut carry = true;
    for digit in digits.iter_mut().rev() {
        if !carry {
            break;
        }
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            carry = false;
        }
    }
    if carry {
        digits.insert(0, b'1');
    }
    String::from_utf8(digits).expect("SemVer numeric components are ASCII digits")
}

fn semver_requirement_matches(requirement: &str, version: &str) -> bool {
    let Some(candidate) = SemanticVersion::parse(version) else {
        return false;
    };
    if requirement == "*" {
        return candidate.prerelease.is_empty();
    }
    let (kind, base_text) = if let Some(base) = requirement.strip_prefix('^') {
        ('^', base)
    } else if let Some(base) = requirement.strip_prefix('~') {
        ('~', base)
    } else {
        ('=', requirement)
    };
    let Some(base) = SemanticVersion::parse(base_text) else {
        return false;
    };
    if !candidate.prerelease.is_empty() && (base.prerelease.is_empty() || !candidate.core_eq(&base))
    {
        return false;
    }
    if kind == '=' {
        return candidate.precedence_cmp(&base) == std::cmp::Ordering::Equal;
    }
    if candidate.precedence_cmp(&base) == std::cmp::Ordering::Less {
        return false;
    }
    let upper = if kind == '~' {
        SemanticVersion {
            major: base.major.clone(),
            minor: increment_semver_number(&base.minor),
            patch: "0".to_string(),
            prerelease: Vec::new(),
        }
    } else if base.major != "0" {
        SemanticVersion {
            major: increment_semver_number(&base.major),
            minor: "0".to_string(),
            patch: "0".to_string(),
            prerelease: Vec::new(),
        }
    } else if base.minor != "0" {
        SemanticVersion {
            major: "0".to_string(),
            minor: increment_semver_number(&base.minor),
            patch: "0".to_string(),
            prerelease: Vec::new(),
        }
    } else {
        SemanticVersion {
            major: "0".to_string(),
            minor: "0".to_string(),
            patch: increment_semver_number(&base.patch),
            prerelease: Vec::new(),
        }
    };
    candidate.precedence_cmp(&upper) == std::cmp::Ordering::Less
}

pub fn resolve_semver_requirement<'a>(
    requirement: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Result<Option<String>, String> {
    if !valid_semver_requirement(requirement) {
        return Err(format!("invalid SemVer requirement '{requirement}'"));
    }
    let mut best = None::<(SemanticVersion, String)>;
    for candidate in candidates {
        let parsed = SemanticVersion::parse(candidate)
            .ok_or_else(|| format!("invalid SemVer candidate '{candidate}'"))?;
        if !semver_requirement_matches(requirement, candidate) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(current, _)| parsed.precedence_cmp(current).is_gt())
        {
            best = Some((parsed, candidate.to_string()));
        }
    }
    Ok(best.map(|(_, version)| version))
}

fn parse_manifest_string_array(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim();
    if !text.starts_with('[') || !text.ends_with(']') {
        return Err("manifest string arrays must use [\"value\", ...] syntax".to_string());
    }
    let bytes = text.as_bytes();
    let mut index = 1usize;
    let end = bytes.len() - 1;
    let mut values = Vec::new();
    while index < end {
        while index < end && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == end {
            break;
        }
        if bytes[index] != b'"' {
            return Err("manifest string array entries must be quoted strings".to_string());
        }
        let start = index;
        index += 1;
        let mut escaped = false;
        while index < end {
            let byte = bytes[index];
            index += 1;
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                break;
            }
        }
        if index > end || bytes[index - 1] != b'"' {
            return Err("unterminated manifest string array entry".to_string());
        }
        values.push(parse_manifest_string(&text[start..index])?);
        while index < end && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == end {
            break;
        }
        if bytes[index] != b',' {
            return Err("manifest string array entries must be separated by commas".to_string());
        }
        index += 1;
        let mut lookahead = index;
        while lookahead < end && bytes[lookahead].is_ascii_whitespace() {
            lookahead += 1;
        }
        if lookahead == end {
            return Err("manifest string arrays may not end with a trailing comma".to_string());
        }
    }
    Ok(values)
}

fn parse_manifest_u32(text: &str) -> Result<u32, String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("manifest integer values must contain only decimal digits".to_string());
    }
    text.parse::<u32>()
        .map_err(|_| "manifest integer value is out of range".to_string())
}

fn valid_android_deep_link(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    let mut scheme_chars = scheme.chars();
    if !scheme_chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
        || !scheme_chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return false;
    }
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.contains(['?', '#'])
        || value.chars().any(char::is_whitespace)
    {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    !authority.is_empty() && !authority.contains('@')
}

fn valid_android_permission(value: &str) -> bool {
    if value.is_empty() || value.starts_with('.') || value.ends_with('.') || value.contains("..") {
        return false;
    }
    let mut saw_dot = false;
    for ch in value.chars() {
        if ch == '.' {
            saw_dot = true;
        } else if !(ch.is_ascii_alphanumeric() || ch == '_') {
            return false;
        }
    }
    saw_dot
}

fn valid_android_application_id(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    let Some(second) = parts.next() else {
        return false;
    };
    valid_android_id_segment(first)
        && valid_android_id_segment(second)
        && parts.all(valid_android_id_segment)
}

fn valid_android_id_segment(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_lowercase())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn default_android_application_id(package_name: &str) -> String {
    let mut segment = package_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' {
                ch
            } else if ch.is_ascii_uppercase() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while segment
        .chars()
        .next()
        .is_some_and(|ch| !ch.is_ascii_lowercase())
    {
        segment.remove(0);
    }
    if segment.is_empty() {
        segment.push_str("app");
    }
    format!("app.flux.{segment}")
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

#[derive(Debug, Clone)]
struct PackageScope {
    name: String,
    root: PathBuf,
    dependencies: BTreeMap<String, PackageDependency>,
}

struct Loader<'a> {
    loaded: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
    program: Program,
    sources: Vec<ProjectSource>,
    diagnostics: Vec<Diagnostic>,
    module_root: PathBuf,
    package_scopes: Vec<PackageScope>,
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
        let current_package = self.package_scope_for_path(&canonical).cloned();
        for import in &mut parsed.imports {
            if import.path.starts_with("pkg:") {
                let Some(current_package) = current_package.as_ref() else {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        import.path_span,
                        "package imports require a flux.toml package manifest",
                    ));
                    continue;
                };
                let Some(resolved) =
                    self.resolve_package_import(current_package, &import.path, import.path_span)
                else {
                    continue;
                };
                if let Ok(target) = fs::canonicalize(&resolved) {
                    import.resolved_source_id =
                        Some(SourceId::from_name(target.to_string_lossy().as_ref()));
                }
                self.load_file(&resolved, Some(import.path_span));
                continue;
            }

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
            let package_root = current_package
                .as_ref()
                .map(|package| package.root.as_path())
                .unwrap_or(self.module_root.as_path());
            if current_package.is_some()
                && resolved_canonical
                    .as_ref()
                    .is_some_and(|path| !path.starts_with(package_root))
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

    fn package_scope_for_path(&self, path: &Path) -> Option<&PackageScope> {
        self.package_scopes
            .iter()
            .filter(|package| path.starts_with(&package.root))
            .max_by_key(|package| package.root.components().count())
    }

    fn resolve_package_import(
        &mut self,
        current_package: &PackageScope,
        import_path: &str,
        span: SourceSpan,
    ) -> Option<PathBuf> {
        let rest = import_path
            .strip_prefix("pkg:")
            .expect("package import prefix was checked");
        let Some((dependency_name, module)) = rest.split_once('/') else {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticStage::Parse,
                span,
                "package imports use 'pkg:<dependency>/<module.flux>'",
            ));
            return None;
        };
        if dependency_name.is_empty() || module.is_empty() {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticStage::Parse,
                span,
                "package imports require both a dependency name and module path",
            ));
            return None;
        }
        let module_path = Path::new(module);
        if module_path.is_absolute()
            || module_path.extension().and_then(|value| value.to_str()) != Some("flux")
            || module_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
        {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticStage::Parse,
                span,
                "package module paths must be normalized relative '.flux' paths",
            ));
            return None;
        }
        let Some(dependency) = current_package.dependencies.get(dependency_name) else {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticStage::Parse,
                span,
                format!("package dependency '{dependency_name}' is not declared in [dependencies]"),
            ));
            return None;
        };
        let PackageDependency::Path { path, requirement } = dependency else {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticStage::Parse,
                span,
                format!(
                    "package dependency '{dependency_name}' requires dependency resolution before it can be imported"
                ),
            ));
            return None;
        };

        let dependency_manifest = current_package.root.join(path).join("flux.toml");
        let manifest = match read_manifest(&dependency_manifest) {
            Ok(manifest) => manifest,
            Err(diagnostics) => {
                let detail = diagnostics
                    .first()
                    .map(|diagnostic| diagnostic.message.as_str())
                    .unwrap_or("invalid dependency manifest");
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    span,
                    format!("failed to load package dependency '{dependency_name}': {detail}"),
                ));
                return None;
            }
        };
        if let Some(requirement) = requirement {
            let Some(version) = manifest.version.as_deref() else {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    span,
                    format!(
                        "package dependency '{dependency_name}' requires version '{requirement}', but the path package has no [package].version"
                    ),
                ));
                return None;
            };
            if !semver_requirement_matches(requirement, version) {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    span,
                    format!(
                        "package dependency '{dependency_name}' requires version '{requirement}', but path package '{}' is version '{version}'",
                        manifest.name
                    ),
                ));
                return None;
            }
        }
        let dependency_root = manifest
            .path
            .parent()
            .expect("canonical manifest path has a parent")
            .to_path_buf();
        if let Some(existing) = self
            .package_scopes
            .iter()
            .find(|package| package.name == manifest.name)
        {
            if existing.root != dependency_root {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    span,
                    format!(
                        "package '{}' resolved to multiple source roots",
                        manifest.name
                    ),
                ));
                return None;
            }
        } else {
            self.package_scopes.push(PackageScope {
                name: manifest.name,
                root: dependency_root.clone(),
                dependencies: manifest.dependencies,
            });
        }
        let resolved = dependency_root.join(module_path);
        if fs::canonicalize(&resolved)
            .ok()
            .is_some_and(|path| !path.starts_with(&dependency_root))
        {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticStage::Parse,
                span,
                "package imports must remain inside the dependency package root",
            ));
            return None;
        }
        Some(resolved)
    }

    fn module_name(&self, path: &Path) -> String {
        let package = self.package_scope_for_path(path);
        let module_root = package
            .map(|package| package.root.as_path())
            .unwrap_or(self.module_root.as_path());
        let relative = path.strip_prefix(module_root).unwrap_or(path);
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
        match package.map(|package| package.name.as_str()) {
            Some(package) if !local.is_empty() => format!("{package}::{local}"),
            Some(package) => package.to_string(),
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
        if let Some(application) = program.application.take() {
            if self.program.application.is_some() {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Type,
                    application.span,
                    "project may declare only one app",
                ));
            } else {
                self.program.application = Some(application);
            }
        }
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
