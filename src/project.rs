use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast::{Expr, ExprKind, Program, Type, UnaryOp, ViewElement};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};
use crate::{codegen, formatter, parser, typecheck};

const PROJECT_CODEGEN_CACHE_VERSION: &str = "flux-project-codegen-v2";
const PROJECT_CODEGEN_CACHE_LIMIT: usize = 8;
const PROJECT_FUNCTION_CODEGEN_CACHE_VERSION: &str = "flux-project-function-codegen-v2";
const COMPILER_SOURCE_FINGERPRINT: &str = env!("FLUX_COMPILER_SOURCE_FINGERPRINT");
const PROJECT_FUNCTION_CODEGEN_CACHE_LIMIT: usize = 8;
const PROJECT_FUNCTION_CODEGEN_CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const PROJECT_TYPED_IR_CACHE_VERSION: &str = "flux-project-typed-ir-v4";
const PROJECT_TYPED_IR_CACHE_LIMIT: usize = 8;
const PROJECT_TYPED_IR_FUNCTION_CACHE_LIMIT: usize = 8;

#[derive(Debug, Clone)]
pub struct ProjectSource {
    pub path: PathBuf,
    pub source_id: SourceId,
    pub module_name: String,
    pub text: String,
}

pub const DEVELOPMENT_ABI_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevelopmentAbi {
    pub version: u32,
    pub fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentUiStringPatch {
    pub element: String,
    pub property: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentStateBoundary {
    pub root_compatible: bool,
    pub preserved: Vec<String>,
    pub reset: Vec<String>,
    pub dropped: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    pub program: Program,
    pub signatures: typecheck::Signatures,
    pub sources: Vec<ProjectSource>,
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
}

impl ProjectAnalysis {
    pub fn development_abi(&self) -> DevelopmentAbi {
        DevelopmentAbi {
            version: DEVELOPMENT_ABI_VERSION,
            fingerprint: development_abi_fingerprint(self),
        }
    }

    pub fn development_state_boundary_from(
        &self,
        previous: &ProjectAnalysis,
    ) -> DevelopmentStateBoundary {
        development_state_boundary(self, previous)
    }

    pub fn development_ui_string_patch_from(
        &self,
        previous: &ProjectAnalysis,
    ) -> Option<Vec<DevelopmentUiStringPatch>> {
        if self.development_abi() != previous.development_abi() {
            return None;
        }
        let mut current = development_ui_string_literals(self)?;
        let mut previous_literals = development_ui_string_literals(previous)?;
        development_application_geometry_lifecycle_defaults(
            self,
            previous,
            &mut current,
            &mut previous_literals,
        )?;
        if current.keys().ne(previous_literals.keys()) {
            return None;
        }
        if development_ui_string_masked_sources(self)?
            != development_ui_string_masked_sources(previous)?
        {
            return None;
        }

        if current.iter().any(|(key, value)| {
            key.1 == "autofocus"
                && value == "0"
                && previous_literals
                    .get(key)
                    .is_some_and(|previous| previous == "1")
        }) {
            return None;
        }

        Some(
            current
                .into_iter()
                .filter_map(|((element, property), value)| {
                    (previous_literals.get(&(element.clone(), property.clone())) != Some(&value))
                        .then_some(DevelopmentUiStringPatch {
                            element,
                            property,
                            value,
                        })
                })
                .collect(),
        )
    }

    pub fn emit_c(&self) -> Result<String, Diagnostic> {
        self.emit_c_for_target(codegen::NativeTarget::Linux)
    }

    /// Reuse a validated generated-C artifact for development builds.
    ///
    /// Analysis still happens before this method is called, so cache hits never
    /// bypass parsing, imports, or type checking. The artifact only avoids
    /// repeating native-source generation and is invalidated by every loaded
    /// source, module identity, translation resource, and this cache version.
    pub fn emit_c_cached(&self, target: &Path) -> Result<String, Diagnostic> {
        self.emit_c_cached_for_target(target, codegen::NativeTarget::Linux)
    }

    /// Reuse a validated generated-C artifact for a specific native target.
    ///
    /// The target is part of the cache identity: the same Flux program can
    /// legitimately produce different native declarations, runtime helpers,
    /// and platform calls for Linux, Android, and Windows.
    pub fn emit_c_cached_for_target(
        &self,
        target: &Path,
        native_target: codegen::NativeTarget,
    ) -> Result<String, Diagnostic> {
        if let Some(generated) = self.read_cached_c_for_target(target, native_target) {
            return Ok(generated);
        }
        let fingerprint = codegen_cache_fingerprint(self, native_target);
        persist_typed_ir_manifest(self, target, native_target, fingerprint);
        let generated = self.emit_c_for_target(native_target)?;
        self.store_cached_c_for_target(target, native_target, &generated);
        Ok(generated)
    }

    fn read_cached_c_for_target(
        &self,
        target: &Path,
        native_target: codegen::NativeTarget,
    ) -> Option<String> {
        let fingerprint = codegen_cache_fingerprint(self, native_target);
        let path = codegen_cache_path(target, fingerprint);
        let header_prefix = format!("{PROJECT_CODEGEN_CACHE_VERSION}:{fingerprint:016x}:");
        let cached = fs::read_to_string(&path).ok()?;
        if let Some((header, generated)) = cached.split_once('\n')
            && let Some(checksum) = header.strip_prefix(&header_prefix)
            && checksum == format!("{:016x}", stable_bytes_hash(generated.as_bytes()))
        {
            if !typed_ir_manifest_is_current(self, target, native_target, fingerprint) {
                persist_typed_ir_manifest(self, target, native_target, fingerprint);
            }
            return Some(generated.to_string());
        }
        remove_cache_artifact_if_unchanged(&path, &cached);
        None
    }

    fn store_cached_c_for_target(
        &self,
        target: &Path,
        native_target: codegen::NativeTarget,
        generated: &str,
    ) {
        let fingerprint = codegen_cache_fingerprint(self, native_target);
        let path = codegen_cache_path(target, fingerprint);
        let header_prefix = format!("{PROJECT_CODEGEN_CACHE_VERSION}:{fingerprint:016x}:");
        let header = format!(
            "{header_prefix}{:016x}\n",
            stable_bytes_hash(generated.as_bytes())
        );
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let temporary = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
        let persisted = File::create(&temporary)
            .and_then(|mut file| {
                file.write_all(header.as_bytes())?;
                file.write_all(generated.as_bytes())?;
                file.sync_all()
            })
            .is_ok();
        if persisted {
            if fs::rename(&temporary, &path).is_err() {
                let _ = fs::remove_file(&temporary);
            }
            prune_codegen_cache(path.parent(), &path);
        }
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

    fn emit_c_for_target_with_function_cache(
        &self,
        target: codegen::NativeTarget,
        cache: &mut codegen::FunctionCodegenCache,
    ) -> Result<(String, codegen::FunctionCodegenStats), Diagnostic> {
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
        codegen::emit_c_for_target_with_source_metadata_cached(
            &self.program,
            &self.signatures,
            &source_paths,
            &source_modules,
            &self.translations,
            target,
            cache,
        )
    }
}

/// Persist a small, deterministic description of the normalized typed IR
/// used by native codegen.  This is deliberately separate from generated C:
/// tooling and future per-module codegen can validate the semantic artifact
/// without reparsing the source, while the bootstrap backend remains free to
/// regenerate the monolithic C file on a miss.
fn persist_typed_ir_manifest(
    analysis: &ProjectAnalysis,
    target: &Path,
    native_target: codegen::NativeTarget,
    fingerprint: u64,
) {
    let root = if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or_else(|| Path::new("."))
    };
    let directory = root.join(".flux").join("ir-cache");
    let path = directory.join(format!("ir-{fingerprint:016x}.manifest"));
    let header_prefix = format!("{PROJECT_TYPED_IR_CACHE_VERSION}:{fingerprint:016x}:");
    let manifest_is_current = if let Ok(cached) = fs::read_to_string(&path)
        && let Some((header, manifest)) = cached.split_once('\n')
        && let Some(checksum) = header.strip_prefix(&header_prefix)
        && checksum == format!("{:016x}", stable_bytes_hash(manifest.as_bytes()))
    {
        true
    } else {
        false
    };
    let mut manifest = String::new();
    manifest.push_str(PROJECT_TYPED_IR_CACHE_VERSION);
    manifest.push('\n');
    manifest.push_str(native_target_cache_tag(native_target));
    manifest.push('\n');
    let mut function_artifacts = Vec::new();
    for function in &analysis.program.functions {
        let cfg = crate::ir::ControlFlowGraph::from_function(function, &analysis.signatures);
        let definitions = cfg
            .nodes()
            .iter()
            .map(|node| node.definitions.len())
            .sum::<usize>()
            + cfg.parameters().len();
        let borrows = cfg
            .nodes()
            .iter()
            .map(|node| node.ownership.borrows.len())
            .sum::<usize>();
        let moves = cfg
            .nodes()
            .iter()
            .map(|node| node.ownership.moves.len())
            .sum::<usize>();
        let calls = cfg
            .nodes()
            .iter()
            .map(|node| node.ownership.calls.len())
            .sum::<usize>();
        let returns = cfg
            .nodes()
            .iter()
            .map(|node| node.ownership.returns.len())
            .sum::<usize>();
        let drops = cfg.drops().len();
        let reachable_values = cfg
            .values()
            .iter()
            .filter(|value| cfg.is_value_reachable(value.id))
            .count();
        // Keep each function's normalized shape independently addressable in
        // the durable artifact.  Future per-module codegen can reuse an
        // unchanged function without treating a changed sibling as a cache
        // miss; the current monolithic backend still consumes the source
        // analysis normally after validating this manifest.
        let shape_hash = normalized_cfg_shape_hash(&cfg);
        let module_name = analysis
            .sources
            .iter()
            .find(|source| source.source_id == function.span.source_id)
            .map(|source| source.module_name.as_str())
            .unwrap_or("<unknown>");
        let function_manifest_line = format!(
            "function\t{}\tmodule={}\tshape={shape_hash:016x}\tnodes={}\tedges={}\tvalues={}\treachable={}\tdefinitions={}\tborrows={}\tmoves={}\tcalls={}\treturns={}\tdrops={}",
            function.name,
            module_name,
            cfg.nodes().len(),
            cfg.edges().len(),
            cfg.values().len(),
            reachable_values,
            definitions,
            borrows,
            moves,
            calls,
            returns,
            drops,
        );
        manifest.push_str(&function_manifest_line);
        manifest.push('\n');
        function_artifacts.push((
            module_name.to_string(),
            function.name.clone(),
            shape_hash,
            function_manifest_line,
        ));
    }
    let checksum = stable_bytes_hash(manifest.as_bytes());
    let header = format!("{header_prefix}{checksum:016x}\n");
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    if !manifest_is_current {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let temporary = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
        let persisted = File::create(&temporary)
            .and_then(|mut file| {
                file.write_all(header.as_bytes())?;
                file.write_all(manifest.as_bytes())?;
                file.sync_all()
            })
            .is_ok();
        if persisted {
            if fs::rename(&temporary, &path).is_err() {
                let _ = fs::remove_file(&temporary);
            } else {
                prune_typed_ir_cache(&directory, &path);
            }
        }
    }
    // Repair missing or corrupt per-function records even when the aggregate
    // manifest is already valid. This keeps the independently addressable
    // cache family self-healing after partial cleanup or interrupted writes.
    if manifest_is_current || path.exists() {
        for (module_name, function_name, shape_hash, function_manifest_line) in function_artifacts {
            persist_typed_ir_function_artifact(
                &directory,
                native_target,
                module_name,
                function_name,
                shape_hash,
                &function_manifest_line,
            );
        }
    }
}

fn typed_ir_manifest_is_current(
    analysis: &ProjectAnalysis,
    target: &Path,
    native_target: codegen::NativeTarget,
    fingerprint: u64,
) -> bool {
    let root = if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or_else(|| Path::new("."))
    };
    let path = root
        .join(".flux")
        .join("ir-cache")
        .join(format!("ir-{fingerprint:016x}.manifest"));
    let Ok(cached) = fs::read_to_string(path) else {
        return false;
    };
    let Some((header, manifest)) = cached.split_once('\n') else {
        return false;
    };
    let prefix = format!("{PROJECT_TYPED_IR_CACHE_VERSION}:{fingerprint:016x}:");
    let Some(checksum) = header.strip_prefix(&prefix) else {
        return false;
    };
    if checksum != format!("{:016x}", stable_bytes_hash(manifest.as_bytes())) {
        return false;
    }
    manifest.starts_with(&format!(
        "{PROJECT_TYPED_IR_CACHE_VERSION}\n{}\n",
        native_target_cache_tag(native_target)
    )) && analysis.program.functions.iter().all(|function| {
        let module_name = analysis
            .sources
            .iter()
            .find(|source| source.source_id == function.span.source_id)
            .map(|source| source.module_name.as_str())
            .unwrap_or("<unknown>");
        let cfg = crate::ir::ControlFlowGraph::from_function(function, &analysis.signatures);
        let shape_hash = normalized_cfg_shape_hash(&cfg);
        manifest.lines().any(|line| {
            line.starts_with("function\t")
                && line
                    .split('\t')
                    .nth(1)
                    .is_some_and(|name| name == function.name)
                && line.contains(&format!("\tmodule={module_name}\t"))
                && line.contains(&format!("\tshape={shape_hash:016x}\t"))
        })
    })
}

/// Publish a separately addressable typed-IR artifact for one function.
///
/// The payload contains the target, source module/function identity, shape
/// hash, and normalized summary. The module plus shape form the stable lookup
/// key, so same-shaped functions in different modules cannot alias while a
/// changed sibling does not invalidate an unchanged function's artifact.
/// Keeping the target in the payload prevents cross-backend reuse.
fn persist_typed_ir_function_artifact(
    directory: &Path,
    native_target: codegen::NativeTarget,
    module_name: String,
    function_name: String,
    shape_hash: u64,
    function_manifest_line: &str,
) {
    let mut key = String::new();
    use std::fmt::Write as _;
    let _ = write!(
        key,
        "{}\n{}\n{}\n{}\n{shape_hash:016x}",
        PROJECT_TYPED_IR_CACHE_VERSION,
        native_target_cache_tag(native_target),
        module_name,
        function_name,
    );
    let artifact_id = stable_bytes_hash(key.as_bytes());
    let path = directory.join(format!("function-{artifact_id:016x}.manifest"));
    let mut payload = String::new();
    payload.push_str(&key);
    payload.push('\n');
    payload.push_str(function_manifest_line);
    let header = format!(
        "{PROJECT_TYPED_IR_CACHE_VERSION}:{artifact_id:016x}:{:016x}\n",
        stable_bytes_hash(payload.as_bytes())
    );
    if read_typed_ir_function_artifact(
        directory,
        native_target,
        &module_name,
        &function_name,
        shape_hash,
    )
    .is_some_and(|cached| cached == payload)
    {
        return;
    }
    let previous = fs::read_to_string(&path).ok();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let persisted = File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(header.as_bytes())?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()
        })
        .is_ok();
    if persisted {
        if fs::rename(&temporary, &path).is_err() {
            // Windows does not replace an existing file with rename.  Only
            // remove the path when it is still the exact bytes we inspected
            // above; a concurrent publisher may have installed a valid
            // artifact while this writer was syncing its temporary file.
            let current = fs::read_to_string(&path).ok();
            if current != previous {
                let _ = fs::remove_file(&temporary);
                return;
            }
            if previous.is_none() {
                let _ = fs::remove_file(&temporary);
                return;
            }
            if fs::remove_file(&path).is_err() || fs::rename(&temporary, &path).is_err() {
                let _ = fs::remove_file(&temporary);
            }
        }
        prune_typed_ir_family(
            directory,
            &path,
            "function-",
            PROJECT_TYPED_IR_FUNCTION_CACHE_LIMIT,
        );
    }
}

/// Read one durable normalized typed-IR function artifact when it matches the
/// requested identity and checksum. The lookup key is reconstructed from the
/// same target/module/function/shape tuple used by publication, so callers do
/// not need to scan a cache directory or trust a filename alone. Invalid,
/// stale, and concurrently replaced artifacts are treated as cache misses.
fn read_typed_ir_function_artifact(
    directory: &Path,
    native_target: codegen::NativeTarget,
    module_name: &str,
    function_name: &str,
    shape_hash: u64,
) -> Option<String> {
    let mut key = String::new();
    use std::fmt::Write as _;
    let _ = write!(
        key,
        "{}\n{}\n{}\n{}\n{shape_hash:016x}",
        PROJECT_TYPED_IR_CACHE_VERSION,
        native_target_cache_tag(native_target),
        module_name,
        function_name,
    );
    let artifact_id = stable_bytes_hash(key.as_bytes());
    let path = directory.join(format!("function-{artifact_id:016x}.manifest"));
    let cached = fs::read_to_string(path).ok()?;
    let (header, payload) = cached.split_once('\n')?;
    let expected_prefix = format!("{PROJECT_TYPED_IR_CACHE_VERSION}:{artifact_id:016x}:");
    let checksum = header.strip_prefix(&expected_prefix)?;
    if checksum != format!("{:016x}", stable_bytes_hash(payload.as_bytes())) {
        return None;
    }
    if !payload.starts_with(&format!("{key}\n")) {
        return None;
    }
    Some(payload.to_string())
}

/// Hash the semantic portion of one normalized CFG for durable IR identity.
///
/// Counts alone are not an identity: changing a literal, callee, edge kind,
/// or ownership provenance can leave every count unchanged.  The normalized
/// graph already has stable IDs for those relationships, so include its
/// semantic payload here.  Source spans are intentionally omitted; the
/// project-level cache fingerprint is canonical-format based and formatting
/// edits must not force a new IR artifact.
fn normalized_cfg_shape_hash(cfg: &crate::ir::ControlFlowGraph) -> u64 {
    use std::fmt::Write as _;

    let mut shape = String::new();
    let _ = write!(shape, "function={:?}\n", cfg.function());
    for parameter in cfg.parameters() {
        let _ = writeln!(shape, "parameter\t{}\t{:?}", parameter.name, parameter.ty);
    }
    for return_type in cfg.returns() {
        let _ = writeln!(shape, "return-type\t{:?}", return_type);
    }
    for edge in cfg.edges() {
        let _ = writeln!(
            shape,
            "edge\t{}\t{}\t{:?}",
            edge.from.0, edge.to.0, edge.kind
        );
    }
    for value in cfg.values() {
        let _ = writeln!(
            shape,
            "value\t{}\t{}\treachable={}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}",
            value.id.0,
            value.producer.0,
            cfg.is_value_reachable(value.id),
            value.result_index,
            value.ty,
            value.kind,
            value.source_constant,
            value.constant,
        );
    }
    for node in cfg.nodes() {
        let _ = writeln!(shape, "node\t{}\t{:?}", node.id.0, node.kind);
        for value_type in &node.value_types {
            let _ = writeln!(shape, "node-value-type\t{}\t{:?}", node.id.0, value_type);
        }
        for value in &node.values {
            let _ = writeln!(shape, "node-value\t{}\t{}", node.id.0, value.0);
        }
        for definition in &node.definitions {
            let _ = writeln!(
                shape,
                "definition\t{}\t{}\t{:?}",
                node.id.0, definition.name, definition.ty
            );
        }
        for read in &node.ownership.reads {
            let _ = writeln!(shape, "read\t{}\t{}", node.id.0, read);
        }
        for borrow in &node.ownership.borrows {
            let _ = writeln!(
                shape,
                "borrow\t{}\t{}\t{:?}\t{:?}",
                node.id.0, borrow.source, borrow.kind, borrow.source_definitions,
            );
        }
        for moved in &node.ownership.moves {
            let _ = writeln!(
                shape,
                "move\t{}\t{}\t{}\t{:?}\t{:?}",
                node.id.0,
                moved.source,
                moved.destination,
                moved.projection,
                moved.source_definitions,
            );
        }
        for call in &node.ownership.calls {
            let _ = writeln!(
                shape,
                "call\t{}\t{}\t{:?}\t{:?}\t{:?}\t{:?}",
                node.id.0,
                call.callee,
                call.arguments,
                call.argument_kinds,
                call.argument_definitions,
                call.borrowed_argument_definitions,
            );
        }
        for returned in &node.ownership.returns {
            let _ = writeln!(
                shape,
                "ownership-return\t{}\t{}\t{:?}\t{:?}\t{:?}",
                node.id.0,
                returned.value.0,
                returned.kind,
                returned.definitions,
                returned.borrowed_definitions,
            );
        }
        for dropped in &node.ownership.drops {
            let _ = writeln!(
                shape,
                "drop\t{}\t{:?}\t{:?}\t{}",
                node.id.0, dropped.definition, dropped.value, dropped.name,
            );
        }
        if let Some(state) = cfg.borrow_state_before(node.id) {
            for borrow in state.borrows() {
                let _ = writeln!(
                    shape,
                    "borrow-state\t{}\t{:?}\t{}\t{}\t{:?}",
                    node.id.0,
                    borrow.definition,
                    borrow.borrower,
                    borrow.source,
                    borrow.source_definition,
                );
            }
        }
    }
    // Borrow event counts alone are insufficient for identity: moving a
    // borrow's end across a branch or loop changes safety even when the same
    // events exist. Include the normalized path-sensitive boundaries and
    // lifetime regions while continuing to omit source spans.
    for start in cfg.borrow_starts() {
        let _ = writeln!(
            shape,
            "borrow-start\t{}\t{}\t{:?}\t{}\t{:?}\t{}",
            start.from.0,
            start.to.0,
            start.definition,
            start.borrower,
            start.source_definition,
            start.source,
        );
    }
    for end in cfg.borrow_ends() {
        let _ = writeln!(
            shape,
            "borrow-end\t{}\t{}\t{:?}\t{}\t{:?}\t{}",
            end.from.0, end.to.0, end.definition, end.borrower, end.source_definition, end.source,
        );
    }
    for lifetime in cfg.borrow_lifetimes() {
        let _ = writeln!(
            shape,
            "borrow-lifetime\t{:?}\t{}\t{:?}\t{}",
            lifetime.definition, lifetime.borrower, lifetime.source_definition, lifetime.source,
        );
        for node in &lifetime.active_before {
            let _ = writeln!(
                shape,
                "borrow-active\t{:?}\t{}",
                lifetime.definition, node.0
            );
        }
        for start in &lifetime.starts {
            let _ = writeln!(
                shape,
                "lifetime-start\t{:?}\t{}\t{}",
                lifetime.definition, start.from.0, start.to.0,
            );
        }
        for end in &lifetime.ends {
            let _ = writeln!(
                shape,
                "lifetime-end\t{:?}\t{}\t{}",
                lifetime.definition, end.from.0, end.to.0,
            );
        }
    }
    stable_bytes_hash(shape.as_bytes())
}

fn remove_cache_artifact_if_unchanged(path: &Path, inspected: &str) {
    let Ok(current) = fs::read_to_string(path) else {
        return;
    };
    if current == inspected {
        let _ = fs::remove_file(path);
    }
}

fn codegen_cache_path(target: &Path, fingerprint: u64) -> PathBuf {
    let root = if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or_else(|| Path::new("."))
    };
    root.join(".flux")
        .join("cache")
        .join(format!("codegen-{fingerprint:016x}.c"))
}

fn codegen_cache_fingerprint(
    analysis: &ProjectAnalysis,
    native_target: codegen::NativeTarget,
) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mut add = |bytes: &[u8]| {
        hash = stable_bytes_hash_with_seed(bytes, hash);
    };
    add(PROJECT_CODEGEN_CACHE_VERSION.as_bytes());
    add(env!("CARGO_PKG_VERSION").as_bytes());
    add(COMPILER_SOURCE_FINGERPRINT.as_bytes());
    add(native_target_cache_tag(native_target).as_bytes());
    for source in &analysis.sources {
        add(source.path.to_string_lossy().as_bytes());
        add(source.module_name.as_bytes());
        // Formatting-only edits do not change the parsed program or native
        // output. Hash the canonical form so the durable native cache can be
        // reused during ordinary editor formatting/save cycles.
        let canonical =
            formatter::format_source(&source.text).unwrap_or_else(|_| source.text.clone());
        add(canonical.as_bytes());
    }
    add(analysis
        .signatures
        .package_constants_fingerprint()
        .as_bytes());
    for (locale, entries) in &analysis.translations {
        add(locale.as_bytes());
        for (key, value) in entries {
            add(key.as_bytes());
            add(value.as_bytes());
        }
    }
    hash
}

fn function_codegen_cache_context_fingerprint(
    analysis: &ProjectAnalysis,
    native_target: codegen::NativeTarget,
) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mut add = |bytes: &[u8]| {
        hash = stable_bytes_hash_with_seed(bytes, hash);
    };
    add(PROJECT_FUNCTION_CODEGEN_CACHE_VERSION.as_bytes());
    add(env!("CARGO_PKG_VERSION").as_bytes());
    add(COMPILER_SOURCE_FINGERPRINT.as_bytes());
    add(native_target_cache_tag(native_target).as_bytes());
    add(analysis
        .signatures
        .package_constants_fingerprint()
        .as_bytes());
    for source in &analysis.sources {
        add(source.path.to_string_lossy().as_bytes());
        add(source.module_name.as_bytes());
        add(
            module_type_surface(&analysis.program, &analysis.signatures, source.source_id)
                .as_bytes(),
        );
    }
    for (locale, entries) in &analysis.translations {
        add(locale.as_bytes());
        for (key, value) in entries {
            add(key.as_bytes());
            add(value.as_bytes());
        }
    }
    hash
}

fn function_codegen_cache_path(target: &Path, fingerprint: u64) -> PathBuf {
    let root = if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or_else(|| Path::new("."))
    };
    root.join(".flux")
        .join("cache")
        .join(format!("function-codegen-{fingerprint:016x}.bin"))
}

fn read_function_codegen_cache(
    analysis: &ProjectAnalysis,
    target: &Path,
    native_target: codegen::NativeTarget,
) -> Option<codegen::FunctionCodegenCache> {
    let fingerprint = function_codegen_cache_context_fingerprint(analysis, native_target);
    let path = function_codegen_cache_path(target, fingerprint);
    if fs::metadata(&path).ok()?.len() > PROJECT_FUNCTION_CODEGEN_CACHE_MAX_BYTES {
        return None;
    }
    let cached = fs::read(&path).ok()?;
    let decoded = (|| {
        let newline = cached.iter().position(|byte| *byte == b'\n')?;
        let header = std::str::from_utf8(&cached[..newline]).ok()?;
        let header_prefix = format!("{PROJECT_FUNCTION_CODEGEN_CACHE_VERSION}:{fingerprint:016x}:");
        let checksum = header.strip_prefix(&header_prefix)?;
        let payload = &cached[newline + 1..];
        (checksum == format!("{:016x}", stable_bytes_hash(payload)))
            .then(|| codegen::FunctionCodegenCache::decode_persisted(payload))
            .flatten()
    })();
    if decoded.is_none() {
        remove_binary_cache_artifact_if_unchanged(&path, &cached);
    }
    decoded
}

fn remove_binary_cache_artifact_if_unchanged(path: &Path, inspected: &[u8]) {
    let Ok(current) = fs::read(path) else {
        return;
    };
    if current == inspected {
        let _ = fs::remove_file(path);
    }
}

fn store_function_codegen_cache(
    analysis: &ProjectAnalysis,
    target: &Path,
    native_target: codegen::NativeTarget,
    cache: &codegen::FunctionCodegenCache,
) {
    let fingerprint = function_codegen_cache_context_fingerprint(analysis, native_target);
    let path = function_codegen_cache_path(target, fingerprint);
    let payload = cache.encode_persisted();
    if payload.len() as u64 > PROJECT_FUNCTION_CODEGEN_CACHE_MAX_BYTES {
        return;
    }
    let header = format!(
        "{PROJECT_FUNCTION_CODEGEN_CACHE_VERSION}:{fingerprint:016x}:{:016x}\n",
        stable_bytes_hash(&payload)
    );
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let persisted = File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(header.as_bytes())?;
            file.write_all(&payload)?;
            file.sync_all()
        })
        .is_ok();
    if persisted {
        if fs::rename(&temporary, &path).is_err() {
            let _ = fs::remove_file(&temporary);
            return;
        }
        prune_function_codegen_cache(path.parent(), &path);
    }
}

fn native_target_cache_tag(native_target: codegen::NativeTarget) -> &'static str {
    match native_target {
        codegen::NativeTarget::Linux => "linux",
        codegen::NativeTarget::Android => "android",
        codegen::NativeTarget::Windows => "windows",
    }
}

fn stable_bytes_hash(bytes: &[u8]) -> u64 {
    stable_bytes_hash_with_seed(bytes, 0xcbf29ce484222325)
}

fn stable_bytes_hash_with_seed(bytes: &[u8], mut hash: u64) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ 0xff
}

fn prune_codegen_cache(directory: Option<&Path>, current: &Path) {
    let Some(directory) = directory else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut artifacts = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path() != current
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("codegen-") && name.ends_with(".c"))
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, path) in artifacts.into_iter().skip(PROJECT_CODEGEN_CACHE_LIMIT - 1) {
        let _ = fs::remove_file(path);
    }
}

fn prune_function_codegen_cache(directory: Option<&Path>, current: &Path) {
    let Some(directory) = directory else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut artifacts = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path() != current
                && entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("function-codegen-") && name.ends_with(".bin")
                })
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, path) in artifacts
        .into_iter()
        .skip(PROJECT_FUNCTION_CODEGEN_CACHE_LIMIT - 1)
    {
        let _ = fs::remove_file(path);
    }
}

fn prune_typed_ir_cache(directory: &Path, current: &Path) {
    prune_typed_ir_family(directory, current, "ir-", PROJECT_TYPED_IR_CACHE_LIMIT);
    prune_typed_ir_family(
        directory,
        Path::new(""),
        "function-",
        PROJECT_TYPED_IR_FUNCTION_CACHE_LIMIT,
    );
}

fn prune_typed_ir_family(directory: &Path, current: &Path, prefix: &str, limit: usize) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut artifacts = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path() != current
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(prefix) && name.ends_with(".manifest"))
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| right.0.cmp(&left.0));
    let retain_existing = limit.saturating_sub(1);
    for (_, path) in artifacts.into_iter().skip(retain_existing) {
        let _ = fs::remove_file(path);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectAnalysisCacheStats {
    pub hits: usize,
    pub misses: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IncrementalTypecheckStats {
    pub runs: usize,
    pub rechecked_modules: usize,
    pub full_runs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectAnalysisOutcome {
    Cached,
    Incremental { rechecked_modules: usize },
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectCodegenOutcome {
    Cached,
    Incremental {
        reused_functions: usize,
        regenerated_functions: usize,
        reused_helpers: usize,
        regenerated_helpers: usize,
        reused_runtime_fragments: usize,
        regenerated_runtime_fragments: usize,
        reused_application_fragments: usize,
        regenerated_application_fragments: usize,
    },
    Full,
}

#[derive(Debug, Clone)]
struct CachedProjectAnalysis {
    analysis: ProjectAnalysis,
    manifest_text: Option<String>,
    package_manifest_texts: BTreeMap<PathBuf, String>,
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

    fn clear_entries(&mut self) {
        self.entries.clear();
    }
}

fn normalize_overlay_paths(overlays: &HashMap<PathBuf, String>) -> HashMap<PathBuf, String> {
    overlays
        .iter()
        .map(|(path, source)| {
            let normalized = fs::canonicalize(path).unwrap_or_else(|_| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir()
                        .map(|directory| directory.join(path))
                        .unwrap_or_else(|_| path.clone())
                }
            });
            (normalized, source.clone())
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct ProjectAnalysisCache {
    entries: HashMap<PathBuf, CachedProjectAnalysis>,
    module_parses: ModuleParseCache,
    invalidated_paths: HashSet<PathBuf>,
    hits: usize,
    misses: usize,
    incremental_typecheck_runs: usize,
    incremental_typecheck_modules: usize,
    full_typecheck_runs: usize,
    last_outcome: Option<ProjectAnalysisOutcome>,
    generated_c: HashMap<(PathBuf, u8), String>,
    function_codegen: HashMap<(PathBuf, u8), codegen::FunctionCodegenCache>,
    last_codegen_outcome: Option<ProjectCodegenOutcome>,
}

impl ProjectAnalysisCache {
    pub fn analyze_with_overlays(
        &mut self,
        target: &Path,
        overlays: &HashMap<PathBuf, String>,
    ) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
        let key = cache_target_key(target)?;
        // Loader source identities are canonical paths.  Normalize overlay
        // keys at this boundary as well so editor clients may supply a
        // relative path, a path containing `.`/`..`, or a symlink spelling
        // without making the cache compare the overlay against the wrong
        // source.  Without this, a changed overlay could be ignored on a
        // cache hit while the loader itself correctly used it on a miss.
        let overlays = normalize_overlay_paths(overlays);
        if let Some(entry) = self.entries.get(&key)
            && cached_analysis_is_current(&key, entry, &overlays)
            && !self.entry_is_invalidated(&key, entry)
        {
            self.hits += 1;
            self.last_outcome = Some(ProjectAnalysisOutcome::Cached);
            return Ok(entry.analysis.clone());
        }

        self.misses += 1;
        let previous = self.entries.get(&key).cloned();
        let report = load_report_with_overlays_and_parse_cache(
            target,
            &overlays,
            Some(&mut self.module_parses),
            codegen::NativeTarget::Linux,
        )?;
        if !report.diagnostics.is_empty() {
            return Err(report.diagnostics);
        }

        let current_manifest_text = manifest_snapshot(&key);
        let current_package_manifests = package_manifest_snapshots(&report.sources);
        let incremental_sources = previous.as_ref().and_then(|previous| {
            (previous.manifest_text == current_manifest_text
                && previous.package_manifest_texts == current_package_manifests)
                .then(|| changed_source_ids(&previous.analysis.sources, &report.sources))
                .flatten()
                .filter(|changed| {
                    changed.is_empty()
                        || changed.iter().all(|source_id| {
                            module_type_surface(
                                &previous.analysis.program,
                                &previous.analysis.signatures,
                                *source_id,
                            ) == module_type_surface(
                                &report.program,
                                &previous.analysis.signatures,
                                *source_id,
                            )
                        })
                })
        });

        let analysis = if let (Some(previous), Some(changed_sources)) =
            (previous.as_ref(), incremental_sources.as_ref())
        {
            if changed_sources.is_empty() {
                self.last_outcome = Some(ProjectAnalysisOutcome::Cached);
                // A watcher may invalidate a path for an editor save even when
                // the resulting bytes are unchanged. The loader has already
                // reparsed the graph and reported parse errors above; with an
                // identical source set and unchanged public surfaces there is
                // no semantic work left to repeat.
                ProjectAnalysis {
                    // Keep the freshly parsed source metadata and spans so
                    // editor diagnostics never point into an older buffer.
                    // The canonical module bytes are unchanged, so the
                    // previous signatures remain valid without a semantic
                    // pass.
                    program: report.program,
                    signatures: previous.analysis.signatures.clone(),
                    sources: report.sources,
                    translations: report.translations,
                }
            } else {
                self.incremental_typecheck_runs += 1;
                self.incremental_typecheck_modules += changed_sources.len();
                self.last_outcome = Some(ProjectAnalysisOutcome::Incremental {
                    rechecked_modules: changed_sources.len(),
                });
                typecheck::check_changed_sources_with_signatures(
                    &report.program,
                    &previous.analysis.signatures,
                    changed_sources,
                )?;
                ProjectAnalysis {
                    program: report.program,
                    signatures: previous.analysis.signatures.clone(),
                    sources: report.sources,
                    translations: report.translations,
                }
            }
        } else {
            self.full_typecheck_runs += 1;
            self.last_outcome = Some(ProjectAnalysisOutcome::Full);
            analyze_with_overlays_report(report)?
        };
        self.entries.insert(
            key.clone(),
            CachedProjectAnalysis {
                package_manifest_texts: current_package_manifests,
                analysis: analysis.clone(),
                manifest_text: current_manifest_text,
            },
        );
        self.clear_invalidations_for_analysis(&key, &analysis);
        Ok(analysis)
    }

    pub fn invalidate_path(&mut self, path: &Path) {
        // Watchers report paths after a delete/rename as well as after an
        // ordinary write. `canonicalize` cannot resolve a path that has
        // temporarily disappeared, which used to make a deleted imported
        // module look unchanged and let the analysis cache serve stale
        // semantics. Preserve the same absolute identity when the file is
        // missing; canonicalize the existing path so symlinked projects keep
        // their established identity.
        let normalized = fs::canonicalize(path).unwrap_or_else(|_| {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|directory| directory.join(path))
                    .unwrap_or_else(|_| path.to_path_buf())
            };
            // Canonicalize the deepest existing parent so a deleted file
            // beneath a symlinked directory still matches the canonical
            // source path stored in the project graph.
            let mut missing = Vec::new();
            let mut existing = absolute.as_path();
            while !existing.exists() {
                if let Some(name) = existing.file_name() {
                    missing.push(name.to_os_string());
                }
                let Some(parent) = existing.parent() else {
                    break;
                };
                existing = parent;
            }
            if let Ok(mut canonical_parent) = fs::canonicalize(existing) {
                for component in missing.iter().rev() {
                    canonical_parent.push(component);
                }
                canonical_parent
            } else {
                absolute
            }
        });
        self.invalidated_paths.insert(normalized);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.module_parses.clear_entries();
        self.invalidated_paths.clear();
        self.generated_c.clear();
        self.function_codegen.clear();
        self.last_outcome = None;
        self.last_codegen_outcome = None;
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

    pub const fn incremental_typecheck_stats(&self) -> IncrementalTypecheckStats {
        IncrementalTypecheckStats {
            runs: self.incremental_typecheck_runs,
            rechecked_modules: self.incremental_typecheck_modules,
            full_runs: self.full_typecheck_runs,
        }
    }

    pub const fn last_outcome(&self) -> Option<ProjectAnalysisOutcome> {
        self.last_outcome
    }

    pub fn emit_c_for_target_cached(
        &mut self,
        target: &Path,
        analysis: &ProjectAnalysis,
        native_target: codegen::NativeTarget,
    ) -> Result<String, Diagnostic> {
        let key = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
        let target_key = match native_target {
            codegen::NativeTarget::Linux => 0,
            codegen::NativeTarget::Android => 1,
            codegen::NativeTarget::Windows => 2,
        };
        let cache_key = (key, target_key);
        if self.last_outcome == Some(ProjectAnalysisOutcome::Cached)
            && let Some(generated) = self.generated_c.get(&cache_key)
        {
            self.last_codegen_outcome = Some(ProjectCodegenOutcome::Cached);
            return Ok(generated.clone());
        }
        if self.last_outcome == Some(ProjectAnalysisOutcome::Full)
            && let Some(generated) = analysis.read_cached_c_for_target(target, native_target)
        {
            self.generated_c
                .insert(cache_key.clone(), generated.clone());
            self.function_codegen.remove(&cache_key);
            self.last_codegen_outcome = Some(ProjectCodegenOutcome::Cached);
            return Ok(generated);
        }

        let function_cache = self.function_codegen.entry(cache_key.clone()).or_default();
        let incremental = matches!(
            self.last_outcome,
            Some(ProjectAnalysisOutcome::Incremental { .. })
        );
        if !incremental {
            function_cache.clear();
        }
        if (!incremental || function_cache.is_empty())
            && let Some(durable) = read_function_codegen_cache(analysis, target, native_target)
        {
            *function_cache = durable;
        }
        let fingerprint = codegen_cache_fingerprint(analysis, native_target);
        persist_typed_ir_manifest(analysis, target, native_target, fingerprint);
        let (generated, stats) =
            analysis.emit_c_for_target_with_function_cache(native_target, function_cache)?;
        analysis.store_cached_c_for_target(target, native_target, &generated);
        store_function_codegen_cache(analysis, target, native_target, function_cache);
        self.generated_c.insert(cache_key, generated.clone());
        self.last_codegen_outcome = Some(
            if stats.reused_functions > 0
                || stats.reused_helpers > 0
                || stats.reused_runtime_fragments > 0
                || stats.reused_application_fragments > 0
                || matches!(
                    self.last_outcome,
                    Some(ProjectAnalysisOutcome::Incremental { .. })
                )
            {
                ProjectCodegenOutcome::Incremental {
                    reused_functions: stats.reused_functions,
                    regenerated_functions: stats.regenerated_functions,
                    reused_helpers: stats.reused_helpers,
                    regenerated_helpers: stats.regenerated_helpers,
                    reused_runtime_fragments: stats.reused_runtime_fragments,
                    regenerated_runtime_fragments: stats.regenerated_runtime_fragments,
                    reused_application_fragments: stats.reused_application_fragments,
                    regenerated_application_fragments: stats.regenerated_application_fragments,
                }
            } else {
                ProjectCodegenOutcome::Full
            },
        );
        Ok(generated)
    }

    pub const fn last_codegen_outcome(&self) -> Option<ProjectCodegenOutcome> {
        self.last_codegen_outcome
    }

    fn entry_is_invalidated(&self, target: &Path, entry: &CachedProjectAnalysis) -> bool {
        self.invalidated_paths.contains(target)
            || entry
                .analysis
                .sources
                .iter()
                .any(|source| self.invalidated_paths.contains(&source.path))
    }

    fn clear_invalidations_for_analysis(&mut self, target: &Path, analysis: &ProjectAnalysis) {
        self.invalidated_paths.remove(target);
        for source in &analysis.sources {
            self.invalidated_paths.remove(&source.path);
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinuxPackageConfig {
    pub uri_schemes: Vec<String>,
    pub file_associations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativePackageConfig {
    pub plugin: bool,
    pub libraries: Vec<String>,
    pub search_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformPackageConfig {
    pub linux_modules: BTreeMap<PathBuf, PathBuf>,
    pub android_modules: BTreeMap<PathBuf, PathBuf>,
    pub windows_modules: BTreeMap<PathBuf, PathBuf>,
}

impl PlatformPackageConfig {
    fn modules_for(&self, target: codegen::NativeTarget) -> &BTreeMap<PathBuf, PathBuf> {
        match target {
            codegen::NativeTarget::Linux => &self.linux_modules,
            codegen::NativeTarget::Android => &self.android_modules,
            codegen::NativeTarget::Windows => &self.windows_modules,
        }
    }

    fn implementation_for(&self, target: codegen::NativeTarget, module: &Path) -> Option<&PathBuf> {
        self.modules_for(target).get(module)
    }
}

pub const PACKAGE_FORMAT_VERSION: u32 = 1;
pub const PACKAGE_LOCK_FORMAT_VERSION: u32 = 1;
pub const PACKAGE_RESOLVER_VERSION: u32 = 1;

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
    pub assets: Option<PathBuf>,
    pub dependencies: BTreeMap<String, PackageDependency>,
    pub constants: BTreeMap<String, typecheck::ConstantValue>,
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
    pub native: NativePackageConfig,
    pub platform: PlatformPackageConfig,
    pub android: AndroidPackageConfig,
    pub linux: LinuxPackageConfig,
    /// Relative package directories participating in this package's workspace.
    /// The manifest package itself remains the workspace root package.
    pub workspace_members: Vec<PathBuf>,
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

fn package_manifest_snapshots(sources: &[ProjectSource]) -> BTreeMap<PathBuf, String> {
    let mut manifests = BTreeMap::new();
    for source in sources {
        for parent in source.path.ancestors().skip(1) {
            let manifest = parent.join("flux.toml");
            if manifest.is_file() {
                if let Ok(text) = fs::read_to_string(&manifest) {
                    manifests.insert(manifest, text);
                }
                break;
            }
        }
    }
    manifests
}

fn changed_source_ids(
    previous: &[ProjectSource],
    current: &[ProjectSource],
) -> Option<HashSet<SourceId>> {
    if previous.len() != current.len() {
        return None;
    }
    let previous_by_path = previous
        .iter()
        .map(|source| (&source.path, source))
        .collect::<HashMap<_, _>>();
    let mut changed = HashSet::new();
    for source in current {
        let previous = previous_by_path.get(&source.path)?;
        if previous.source_id != source.source_id {
            return None;
        }
        if source_semantically_changed(&previous.text, &source.text) {
            changed.insert(source.source_id);
        }
    }
    Some(changed)
}

fn source_semantically_changed(previous: &str, current: &str) -> bool {
    if previous == current {
        return false;
    }
    match (
        formatter::format_source(previous),
        formatter::format_source(current),
    ) {
        (Ok(previous), Ok(current)) => previous != current,
        _ => true,
    }
}

fn module_type_surface(
    program: &Program,
    signatures: &typecheck::Signatures,
    source_id: SourceId,
) -> String {
    use std::fmt::Write as _;

    // Constants are part of the semantic API of a module: callers may fold
    // them into defaults, target metadata, or compile-time branches. Keep a
    // span-free fingerprint of their expression in the surface identity so a
    // changed public constant cannot incorrectly reuse the old signatures.
    fn expr_surface(expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(value) => format!("i{value}"),
            ExprKind::Bool(value) => format!("b{value}"),
            ExprKind::Str(value) => format!("s{:?}", value),
            ExprKind::InterpolatedString(parts) => {
                let parts = parts
                    .iter()
                    .map(|part| match part {
                        crate::ast::InterpolatedStringPart::Text(text) => {
                            format!("t{:?}", text)
                        }
                        crate::ast::InterpolatedStringPart::Binding { name, .. } => {
                            format!("v{name}")
                        }
                    })
                    .collect::<Vec<_>>();
                format!("is[{}]", parts.join(","))
            }
            ExprKind::Var(name) => format!("v{name}"),
            ExprKind::Field {
                base,
                name,
                optional,
                ..
            } => {
                format!("f({},{name},{optional})", expr_surface(base))
            }
            ExprKind::Conditional {
                then_expr,
                cond,
                else_expr,
            } => format!(
                "c({},{},{})",
                expr_surface(cond),
                expr_surface(then_expr),
                expr_surface(else_expr)
            ),
            ExprKind::Unary { op, expr: inner } => {
                let op = match op {
                    UnaryOp::Neg => "neg",
                    UnaryOp::Not => "not",
                    UnaryOp::Borrow => "borrow",
                };
                format!("u({op},{})", expr_surface(inner))
            }
            ExprKind::Binary { left, op, right } => {
                format!("x({op:?},{},{})", expr_surface(left), expr_surface(right))
            }
            // These forms are not currently accepted by constant evaluation,
            // but retaining a deterministic fallback makes this helper robust
            // if the constant grammar grows before its evaluator does.
            other => format!("unsupported:{other:?}"),
        }
    }

    fn param_surface(param: &crate::ast::Param, signatures: &typecheck::Signatures) -> String {
        format!(
            "{}:{:?}:{}:{:?}",
            param.name,
            param.ty,
            param.named_only,
            param
                .default
                .as_ref()
                .and_then(|default| typecheck::constant_primitive_value(default, signatures))
        )
    }

    fn params_surface(params: &[crate::ast::Param], signatures: &typecheck::Signatures) -> String {
        params
            .iter()
            .map(|param| param_surface(param, signatures))
            .collect::<Vec<_>>()
            .join(",")
    }

    let mut surface = String::new();
    let imports = program
        .imports
        .iter()
        .filter(|definition| definition.path_span.source_id == source_id)
        .map(|definition| (&definition.path, definition.resolved_source_id))
        .collect::<Vec<_>>();
    let aliases = program
        .aliases
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
        .map(|definition| (definition.public, &definition.name, &definition.target))
        .collect::<Vec<_>>();
    let interfaces = program
        .interfaces
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
        .map(|definition| {
            (
                definition.public,
                &definition.name,
                definition
                    .parents
                    .iter()
                    .map(|parent| parent.name.as_str())
                    .collect::<Vec<_>>(),
                definition
                    .functions
                    .iter()
                    .map(|function| {
                        (
                            &function.name,
                            params_surface(&function.params, signatures),
                            &function.returns,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let implementations = program
        .implementations
        .iter()
        .filter(|definition| definition.interface_span.source_id == source_id)
        .map(|definition| {
            (
                &definition.interface_name,
                &definition.target_name,
                definition
                    .mappings
                    .iter()
                    .map(|mapping| (&mapping.member, &mapping.function))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let structs = program
        .structs
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
        .map(|definition| {
            (
                definition.public,
                &definition.name,
                definition
                    .fields
                    .iter()
                    .map(|field| (&field.name, &field.ty))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let enums = program
        .enums
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
        .map(|definition| {
            (
                definition.public,
                &definition.name,
                definition
                    .variants
                    .iter()
                    .map(|variant| {
                        (
                            &variant.name,
                            variant
                                .payloads
                                .iter()
                                .map(|payload| &payload.ty)
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let constants = program
        .constants
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
        .map(|definition| {
            (
                definition.public,
                &definition.name,
                &definition.ty,
                expr_surface(&definition.value),
            )
        })
        .collect::<Vec<_>>();
    let routes = program
        .routes
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
        .map(|definition| (&definition.name, &definition.view_name))
        .collect::<Vec<_>>();

    write!(
        surface,
        "imports={imports:?};aliases={aliases:?};interfaces={interfaces:?};implementations={implementations:?};structs={structs:?};enums={enums:?};constants={constants:?};routes={routes:?};"
    )
    .expect("writing a String cannot fail");

    if let Some(application) = program
        .application
        .as_ref()
        .filter(|definition| definition.keyword_span.source_id == source_id)
    {
        let metadata = application
            .metadata
            .iter()
            .filter_map(|field| {
                if development_application_metadata_lifecycle_patch_value(field).is_some() {
                    return None;
                }
                let value = if development_application_metadata_patch_value(field).is_some() {
                    "__flux_hot_application_metadata__".to_string()
                } else {
                    expr_surface(&field.value)
                };
                Some((&field.name, value))
            })
            .collect::<Vec<_>>();
        write!(
            surface,
            "application=({:?},{metadata:?});",
            application.view_name
        )
        .expect("writing a String cannot fail");
    }

    for view in program
        .views
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
    {
        write!(
            surface,
            "view=({:?},{:?},{:?},{:?},{:?},{:?});",
            view.public,
            view.name,
            params_surface(&view.params, signatures),
            view.states
                .iter()
                .map(|state| (&state.name, &state.ty))
                .collect::<Vec<_>>(),
            view.derived
                .iter()
                .map(|derived| (&derived.name, &derived.ty))
                .collect::<Vec<_>>(),
            (
                &view.grid.columns,
                &view.grid.rows,
                view.grid.flow,
                view.grid.scroll,
            )
        )
        .expect("writing a String cannot fail");
    }

    for function in program
        .functions
        .iter()
        .filter(|definition| definition.name_span.source_id == source_id)
    {
        write!(
            surface,
            "function=({:?},{:?},{:?},{:?},{:?},{:?},{:?},{:?});",
            function.public,
            function.foreign_symbol,
            function.unsafe_foreign,
            function.asynchronous,
            function.name,
            params_surface(&function.params, signatures),
            function.returns,
            function.expression_body
        )
        .expect("writing a String cannot fail");
    }

    surface
}

const DEVELOPMENT_APPLICATION_PATCH_ELEMENT: &str = "__application__";
const DEVELOPMENT_GRID_PATCH_ELEMENT: &str = "__grid__";
const DEVELOPMENT_APPLICATION_THEME_PALETTE_PROPERTIES: &[&str] = &[
    "surface_color",
    "surface_raised_color",
    "text_color",
    "text_muted_color",
    "accent_color",
    "on_accent_color",
    "outline_color",
    "danger_color",
    "success_color",
    "warning_color",
    "shadow_color",
];
const DEVELOPMENT_APPLICATION_DEFAULT_PROPERTIES: &[&str] = &[
    "title",
    "resizable",
    "theme",
    "layout_direction",
    "surface_color",
    "surface_raised_color",
    "text_color",
    "text_muted_color",
    "accent_color",
    "on_accent_color",
    "outline_color",
    "danger_color",
    "success_color",
    "warning_color",
    "shadow_color",
];
const DEVELOPMENT_APPLICATION_GEOMETRY_PROPERTIES: &[&str] = &["width", "height"];
const DEVELOPMENT_APPLICATION_LIFECYCLE_PROPERTIES: &[&str] = &[
    "title",
    "resizable",
    "theme",
    "layout_direction",
    "width",
    "height",
    "surface_color",
    "surface_raised_color",
    "text_color",
    "text_muted_color",
    "accent_color",
    "on_accent_color",
    "outline_color",
    "danger_color",
    "success_color",
    "warning_color",
    "shadow_color",
];
const DEVELOPMENT_APPLICATION_THEME_DEFAULT_SENTINEL: &str = "__flux_theme_default__";

fn development_application_metadata_patch_value(
    field: &crate::ast::ApplicationMetadataField,
) -> Option<String> {
    match typecheck::source_name_to_internal(&field.name).as_str() {
        "title" => {
            let ExprKind::Str(value) = &field.value.kind else {
                return None;
            };
            (!value.as_bytes().contains(&0)).then(|| value.clone())
        }
        "layout_direction" => {
            let ExprKind::Str(value) = &field.value.kind else {
                return None;
            };
            matches!(value.as_str(), "system" | "ltr" | "rtl").then(|| value.clone())
        }
        "theme" => {
            let ExprKind::Str(value) = &field.value.kind else {
                return None;
            };
            matches!(value.as_str(), "system" | "light" | "dark").then(|| value.clone())
        }
        "surface_color"
        | "surface_raised_color"
        | "text_color"
        | "text_muted_color"
        | "accent_color"
        | "on_accent_color"
        | "outline_color"
        | "danger_color"
        | "success_color"
        | "warning_color"
        | "shadow_color" => {
            let ExprKind::Str(value) = &field.value.kind else {
                return None;
            };
            typecheck::valid_hex_ui_color(value).then(|| value.clone())
        }
        "resizable" => {
            let ExprKind::Bool(value) = field.value.kind else {
                return None;
            };
            Some(if value { "1" } else { "0" }.to_string())
        }
        "width" | "height" => {
            let value = development_ui_i64_literal_value(&field.value)?;
            (value > 0 && value <= i64::from(i32::MAX)).then(|| value.to_string())
        }
        _ => None,
    }
}

fn development_application_metadata_lifecycle_patch_value(
    field: &crate::ast::ApplicationMetadataField,
) -> Option<String> {
    let property = typecheck::source_name_to_internal(&field.name);
    DEVELOPMENT_APPLICATION_LIFECYCLE_PROPERTIES
        .contains(&property.as_str())
        .then(|| development_application_metadata_patch_value(field))
        .flatten()
}

fn development_application_default_patch_value(
    analysis: &ProjectAnalysis,
    property: &str,
) -> Option<String> {
    let application = analysis.program.application.as_ref()?;
    let view = analysis
        .program
        .views
        .iter()
        .find(|view| view.name == application.view_name)?;
    match property {
        "title" => Some(view.name.clone()),
        "resizable" => Some("1".to_string()),
        "theme" | "layout_direction" => Some("system".to_string()),
        property if DEVELOPMENT_APPLICATION_THEME_PALETTE_PROPERTIES.contains(&property) => {
            Some(DEVELOPMENT_APPLICATION_THEME_DEFAULT_SENTINEL.to_string())
        }
        "width" | "height" => {
            let (width, height) = codegen::bootstrap_window_size(view);
            Some(if property == "width" { width } else { height }.to_string())
        }
        _ => None,
    }
}

fn development_application_geometry_lifecycle_defaults(
    current_analysis: &ProjectAnalysis,
    previous_analysis: &ProjectAnalysis,
    current: &mut BTreeMap<(String, String), String>,
    previous: &mut BTreeMap<(String, String), String>,
) -> Option<()> {
    for property in DEVELOPMENT_APPLICATION_GEOMETRY_PROPERTIES {
        let key = (
            DEVELOPMENT_APPLICATION_PATCH_ELEMENT.to_string(),
            (*property).to_string(),
        );
        if current.contains_key(&key) == previous.contains_key(&key) {
            continue;
        }
        if !current.contains_key(&key) {
            current.insert(
                key.clone(),
                development_application_default_patch_value(current_analysis, property)?,
            );
        }
        if !previous.contains_key(&key) {
            previous.insert(
                key,
                development_application_default_patch_value(previous_analysis, property)?,
            );
        }
    }
    Some(())
}

fn development_ui_element_has_property(element: &ViewElement, property: &str) -> bool {
    element
        .properties
        .iter()
        .any(|candidate| typecheck::source_name_to_internal(&candidate.name) == property)
}

fn development_ui_string_property_is_patchable(element: &ViewElement, property: &str) -> bool {
    match property {
        "text" => {
            (matches!(element.kind.as_str(), "Text" | "Button" | "Header")
                && (element.kind != "Text"
                    || !element.properties.iter().any(|property| {
                        typecheck::source_name_to_internal(&property.name) == "rich_text"
                    })))
                || (element.kind == "TextInput"
                    && !development_ui_element_has_property(element, "on_change"))
        }
        "label" => matches!(
            element.kind.as_str(),
            "Toggle" | "Radio" | "Nav" | "Chart" | "Content"
        ),
        "title" => element.kind == "Card",
        "source" | "alt" | "fit" => element.kind == "Image",
        "background_color" | "shadow_color" => true,
        "border_color" => [
            "border_top_color",
            "border_bottom_color",
            "border_start_color",
            "border_end_color",
        ]
        .iter()
        .any(|property| !development_ui_element_has_property(element, property)),
        "border_top_color" | "border_bottom_color" | "border_start_color" | "border_end_color" => {
            true
        }
        "border_style" | "transition_easing" => true,
        "color" => {
            element.kind == "Text" && !development_ui_element_has_property(element, "rich_text")
        }
        "variant" => {
            element.kind == "Text" && !development_ui_element_has_property(element, "rich_text")
        }
        "rich_text" => element.kind == "Text",
        "font_family" | "text_align" | "wrap_mode" | "ellipsize" => element.kind == "Text",
        "tooltip" => {
            element.kind != "TextInput"
                || !development_ui_element_has_property(element, "validation_message")
        }
        "drag_text" => true,
        "context_menu_label" => true,
        "shortcut" => true,
        "shortcut_scope" => development_ui_element_has_property(element, "shortcut"),
        "placeholder" => element.kind == "TextInput",
        "keyboard_type" => element.kind == "TextInput",
        "validation_state" => element.kind == "TextInput",
        "validation_message" => {
            element.kind == "TextInput"
                && !development_ui_element_has_property(element, "tooltip")
                && ![
                    "accessibility_description",
                    "accessibility_action_label",
                    "accessibility_long_press_label",
                    "accessibility_actions",
                ]
                .iter()
                .any(|property| development_ui_element_has_property(element, property))
        }
        "accessibility_label"
        | "accessibility_description"
        | "accessibility_value"
        | "accessibility_role" => true,
        "accessibility_action_label" => {
            !development_ui_element_has_property(element, "accessibility_description")
        }
        "accessibility_long_press_label" => {
            !development_ui_element_has_property(element, "accessibility_description")
                && !development_ui_element_has_property(element, "accessibility_action_label")
        }
        "status" => true,
        "align_x" | "align_y" => true,
        _ => false,
    }
}

fn development_ui_string_list_property_is_patchable(element: &ViewElement, property: &str) -> bool {
    match property {
        "context_menu_items" => {
            development_ui_element_has_property(element, "on_context_menu_item_select")
        }
        "accessibility_actions" => {
            development_ui_element_has_property(element, "on_accessibility_action")
                && ![
                    "accessibility_description",
                    "accessibility_action_label",
                    "accessibility_long_press_label",
                ]
                .iter()
                .any(|property| development_ui_element_has_property(element, property))
        }
        _ => false,
    }
}

fn development_ui_property_lifecycle_patch_value(
    element: &ViewElement,
    property: &crate::ast::ViewProperty,
) -> Option<String> {
    if property.transition.is_some() {
        return None;
    }
    let property_name = typecheck::source_name_to_internal(&property.name);
    if property_name == "bold"
        && element.kind == "Text"
        && !development_ui_element_has_property(element, "variant")
    {
        let ExprKind::Bool(value) = property.value.kind else {
            return None;
        };
        return Some(if value { "1" } else { "0" }.to_string());
    }
    if matches!(
        property_name.as_str(),
        "visible"
            | "clip"
            | "enabled"
            | "primary"
            | "accessibility_hidden"
            | "read_only"
            | "can_shrink"
            | "selectable"
            | "wrap"
            | "italic"
            | "underline"
            | "strikethrough"
            | "password"
            | "checked"
            | "selected"
    ) {
        if !development_ui_bool_property_is_patchable(element, &property_name) {
            return None;
        }
        let ExprKind::Bool(value) = property.value.kind else {
            return None;
        };
        return Some(if value { "1" } else { "0" }.to_string());
    }
    if property_name == "margin" {
        if !development_ui_i64_property_is_patchable(element, &property_name) {
            return None;
        }
        let value = development_ui_i64_literal_value(&property.value)?;
        if !(0..=i64::from(i32::MAX)).contains(&value) {
            return None;
        }
        return Some(value.to_string());
    }
    None
}

fn development_ui_property_lifecycle_default(
    element: &ViewElement,
    property: &str,
) -> Option<String> {
    if property == "bold"
        && element.kind == "Text"
        && !development_ui_element_has_property(element, "variant")
    {
        return Some("0".to_string());
    }
    if matches!(
        property,
        "visible"
            | "clip"
            | "enabled"
            | "primary"
            | "accessibility_hidden"
            | "read_only"
            | "can_shrink"
            | "selectable"
            | "wrap"
            | "italic"
            | "underline"
            | "strikethrough"
            | "password"
            | "checked"
            | "selected"
    ) {
        if !development_ui_bool_property_is_patchable(element, property) {
            return None;
        }
        return Some(
            if matches!(property, "visible" | "enabled" | "wrap") {
                "1"
            } else {
                "0"
            }
            .to_string(),
        );
    }
    if property == "margin" && development_ui_i64_property_is_patchable(element, property) {
        return Some("0".to_string());
    }
    None
}

fn development_ui_bool_property_is_patchable(element: &ViewElement, property: &str) -> bool {
    match property {
        "visible" | "clip" | "focusable" | "accessibility_hidden" => true,
        "autofocus" | "drag_translate" | "pinch_scale" => true,
        "selectable" | "wrap" | "bold" | "italic" | "underline" | "strikethrough" => {
            element.kind == "Text"
        }
        "enabled" => matches!(
            element.kind.as_str(),
            "Button" | "TextInput" | "Toggle" | "Radio"
        ),
        "read_only" => element.kind == "TextInput",
        "submit_on_enter" => {
            element.kind == "TextInput" && development_ui_element_has_property(element, "on_submit")
        }
        "password" => {
            element.kind == "TextInput"
                && element
                    .properties
                    .iter()
                    .find(|property| {
                        typecheck::source_name_to_internal(&property.name) == "multiline"
                    })
                    .is_none_or(|property| matches!(property.value.kind, ExprKind::Bool(false)))
        }
        "primary" => element.kind == "Button",
        "can_shrink" => element.kind == "Image",
        "checked" => {
            element.kind == "Toggle" && !development_ui_element_has_property(element, "on_change")
        }
        "selected" => {
            element.kind == "Radio" && !development_ui_element_has_property(element, "on_select")
        }
        _ => false,
    }
}

const DEVELOPMENT_UI_TRANSFORM_PROPERTIES: &[&str] = &[
    "translate_x",
    "translate_y",
    "rotate_degrees",
    "scale_percent",
    "scale_x_percent",
    "scale_y_percent",
    "skew_x_degrees",
    "skew_y_degrees",
    "transform_origin_x_percent",
    "transform_origin_y_percent",
];

fn development_ui_transform_is_static_literal(element: &ViewElement) -> bool {
    !development_ui_element_has_property(element, "drag_translate")
        && !development_ui_element_has_property(element, "pinch_scale")
        && DEVELOPMENT_UI_TRANSFORM_PROPERTIES
            .iter()
            .all(|property_name| {
                element
                    .properties
                    .iter()
                    .find(|property| {
                        typecheck::source_name_to_internal(&property.name) == *property_name
                    })
                    .is_none_or(|property| {
                        development_ui_i64_literal_value(&property.value)
                            .is_some_and(|value| i32::try_from(value).is_ok())
                    })
            })
}

fn development_ui_i64_property_is_patchable(element: &ViewElement, property: &str) -> bool {
    match property {
        "max_length" => element.kind == "TextInput",
        "max_lines" | "max_width_chars" | "letter_spacing" | "line_height_percent" => {
            element.kind == "Text"
        }
        "size" => matches!(element.kind.as_str(), "Text" | "Button"),
        "focus_scope"
        | "accessibility_order"
        | "layout_transition_ms"
        | "transition_ms"
        | "transition_delay_ms" => true,
        "shadow_blur" | "shadow_offset_x" | "shadow_offset_y" => true,
        "translate_x"
        | "translate_y"
        | "rotate_degrees"
        | "scale_percent"
        | "scale_x_percent"
        | "scale_y_percent"
        | "skew_x_degrees"
        | "skew_y_degrees"
        | "transform_origin_x_percent"
        | "transform_origin_y_percent" => development_ui_transform_is_static_literal(element),
        "min_width" => element
            .properties
            .iter()
            .find(|property| typecheck::source_name_to_internal(&property.name) == "max_width")
            .is_none_or(|property| development_ui_i64_literal_value(&property.value).is_some()),
        "min_height" => element
            .properties
            .iter()
            .find(|property| typecheck::source_name_to_internal(&property.name) == "max_height")
            .is_none_or(|property| development_ui_i64_literal_value(&property.value).is_some()),
        "max_width" => element
            .properties
            .iter()
            .find(|property| typecheck::source_name_to_internal(&property.name) == "min_width")
            .is_none_or(|property| development_ui_i64_literal_value(&property.value).is_some()),
        "max_height" => element
            .properties
            .iter()
            .find(|property| typecheck::source_name_to_internal(&property.name) == "min_height")
            .is_none_or(|property| development_ui_i64_literal_value(&property.value).is_some()),
        "margin" | "margin_top" | "margin_bottom" | "margin_start" | "margin_end" => true,
        "border_width" => [
            "border_top_width",
            "border_bottom_width",
            "border_start_width",
            "border_end_width",
        ]
        .iter()
        .any(|property| !development_ui_element_has_property(element, property)),
        "border_top_width" | "border_bottom_width" | "border_start_width" | "border_end_width" => {
            true
        }
        "padding" => [
            "padding_top",
            "padding_bottom",
            "padding_start",
            "padding_end",
        ]
        .iter()
        .any(|property| !development_ui_element_has_property(element, property)),
        "padding_top" | "padding_bottom" | "padding_start" | "padding_end" => true,
        "radius" => [
            "radius_top_left",
            "radius_top_right",
            "radius_bottom_left",
            "radius_bottom_right",
        ]
        .iter()
        .any(|property| !development_ui_element_has_property(element, property)),
        "radius_top_left" | "radius_top_right" | "radius_bottom_left" | "radius_bottom_right" => {
            true
        }
        _ => false,
    }
}

fn development_ui_i64_literal_value(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int(value) => Some(*value),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => {
            let ExprKind::Int(value) = expr.kind else {
                return None;
            };
            value.checked_neg()
        }
        _ => None,
    }
}

fn development_ui_string_literals(
    analysis: &ProjectAnalysis,
) -> Option<BTreeMap<(String, String), String>> {
    let application = analysis.program.application.as_ref()?;
    let view = analysis
        .program
        .views
        .iter()
        .find(|view| view.name == application.view_name)?;
    let mut literals = BTreeMap::new();
    for field in &application.metadata {
        let Some(value) = development_application_metadata_patch_value(field) else {
            continue;
        };
        literals.insert(
            (
                DEVELOPMENT_APPLICATION_PATCH_ELEMENT.to_string(),
                typecheck::source_name_to_internal(&field.name),
            ),
            value,
        );
    }
    for property in DEVELOPMENT_APPLICATION_DEFAULT_PROPERTIES {
        if application
            .metadata
            .iter()
            .any(|field| typecheck::source_name_to_internal(&field.name) == *property)
        {
            continue;
        }
        literals.insert(
            (
                DEVELOPMENT_APPLICATION_PATCH_ELEMENT.to_string(),
                (*property).to_string(),
            ),
            development_application_default_patch_value(analysis, property)?,
        );
    }
    for (property, value) in [
        ("gap", view.grid.gap.unwrap_or(12)),
        ("padding", view.grid.padding.unwrap_or(20)),
    ] {
        if value > i32::MAX as u32 {
            return None;
        }
        literals.insert(
            (
                DEVELOPMENT_GRID_PATCH_ELEMENT.to_string(),
                property.to_string(),
            ),
            value.to_string(),
        );
    }

    let mut layout_transition_duration = None;
    for element in &view.elements {
        let Some(property) = element.properties.iter().find(|property| {
            typecheck::source_name_to_internal(&property.name) == "layout_transition_ms"
        }) else {
            continue;
        };
        let ExprKind::Int(value) = property.value.kind else {
            continue;
        };
        if !(0..=i64::from(i32::MAX)).contains(&value) {
            return None;
        }
        if layout_transition_duration.is_some_and(|existing| existing != value) {
            return None;
        }
        layout_transition_duration = Some(value);
    }

    for element in &view.elements {
        for (minimum_name, maximum_name) in
            [("min_width", "max_width"), ("min_height", "max_height")]
        {
            let minimum = element.properties.iter().find(|property| {
                typecheck::source_name_to_internal(&property.name) == minimum_name
            });
            let maximum = element.properties.iter().find(|property| {
                typecheck::source_name_to_internal(&property.name) == maximum_name
            });
            if let (Some(minimum), Some(maximum)) = (minimum, maximum)
                && let (Some(minimum), Some(maximum)) = (
                    development_ui_i64_literal_value(&minimum.value),
                    development_ui_i64_literal_value(&maximum.value),
                )
                && minimum > maximum
            {
                return None;
            }
        }
        for property in &element.properties {
            let property_name = typecheck::source_name_to_internal(&property.name);
            if development_ui_string_list_property_is_patchable(element, &property_name) {
                let ExprKind::List(values) = &property.value.kind else {
                    continue;
                };
                if values.is_empty() {
                    return None;
                }
                let mut labels = Vec::with_capacity(values.len());
                for item in values {
                    let ExprKind::Str(value) = &item.kind else {
                        return None;
                    };
                    if value.is_empty() || value.as_bytes().contains(&0) {
                        return None;
                    }
                    labels.push(value.clone());
                }
                if property_name == "accessibility_actions" {
                    literals.insert(
                        (element.name.clone(), property_name),
                        format!("Actions: {}", labels.join("; ")),
                    );
                } else {
                    for (index, value) in labels.into_iter().enumerate() {
                        literals.insert(
                            (element.name.clone(), format!("context_menu_item_{index}")),
                            value,
                        );
                    }
                }
                continue;
            }
            let value = if development_ui_string_property_is_patchable(element, &property_name) {
                let ExprKind::Str(value) = &property.value.kind else {
                    continue;
                };
                if value.as_bytes().contains(&0) {
                    return None;
                }
                if property_name == "font_family" && value.is_empty() {
                    return None;
                }
                if property_name == "rich_text" && !codegen::valid_portable_rich_text(value) {
                    return None;
                }
                if matches!(
                    property_name.as_str(),
                    "background_color"
                        | "shadow_color"
                        | "color"
                        | "border_color"
                        | "border_top_color"
                        | "border_bottom_color"
                        | "border_start_color"
                        | "border_end_color"
                ) && !typecheck::valid_ui_color(value)
                {
                    return None;
                }
                if property_name == "border_style"
                    && !matches!(
                        value.as_str(),
                        "none" | "solid" | "dashed" | "dotted" | "double"
                    )
                {
                    return None;
                }
                if property_name == "transition_easing"
                    && typecheck::transition_easing_css_value(value).is_none()
                {
                    return None;
                }
                if property_name == "keyboard_type" {
                    let valid = matches!(
                        value.as_str(),
                        "text" | "email" | "number" | "decimal" | "phone" | "url"
                    );
                    if !valid {
                        return None;
                    }
                }
                if property_name == "fit" {
                    let valid = matches!(
                        value.as_str(),
                        "fill" | "contain" | "cover" | "scaleDown" | "scale_down"
                    );
                    if !valid {
                        return None;
                    }
                }
                if property_name == "accessibility_role"
                    && !typecheck::ACCESSIBILITY_ROLES.contains(&value.as_str())
                {
                    return None;
                }
                if property_name == "context_menu_label" && value.is_empty() {
                    return None;
                }
                if property_name == "shortcut_scope"
                    && !typecheck::SHORTCUT_SCOPES.contains(&value.as_str())
                {
                    return None;
                }
                if property_name == "variant" {
                    let valid = matches!(
                        value.as_str(),
                        "body" | "caption" | "heading" | "title" | "display"
                    );
                    if !valid {
                        return None;
                    }
                }
                if property_name == "text_align" {
                    let valid = matches!(value.as_str(), "left" | "center" | "right" | "fill");
                    if !valid {
                        return None;
                    }
                }
                if property_name == "wrap_mode" {
                    let valid =
                        matches!(value.as_str(), "word" | "char" | "wordChar" | "word_char");
                    if !valid {
                        return None;
                    }
                }
                if property_name == "ellipsize" {
                    let valid = matches!(value.as_str(), "none" | "start" | "middle" | "end");
                    if !valid {
                        return None;
                    }
                }
                if matches!(property_name.as_str(), "align_x" | "align_y") {
                    let valid = matches!(value.as_str(), "start" | "center" | "end" | "fill");
                    if !valid {
                        return None;
                    }
                }
                if property_name == "status"
                    && !typecheck::UI_PRESENTATION_STATES.contains(&value.as_str())
                {
                    return None;
                }
                if property_name == "shortcut" {
                    codegen::gtk_shortcut_trigger(value)?
                } else {
                    value.clone()
                }
            } else if development_ui_bool_property_is_patchable(element, &property_name) {
                let ExprKind::Bool(value) = property.value.kind else {
                    continue;
                };
                if value {
                    "1".to_string()
                } else {
                    "0".to_string()
                }
            } else if development_ui_i64_property_is_patchable(element, &property_name) {
                let Some(value) = development_ui_i64_literal_value(&property.value) else {
                    continue;
                };
                if property_name == "accessibility_order" && value < 0 {
                    return None;
                }
                if property_name == "max_length" && !(0..=i64::from(i32::MAX)).contains(&value) {
                    return None;
                }
                if property_name == "max_lines" && !(1..=i64::from(i32::MAX)).contains(&value) {
                    return None;
                }
                if property_name == "max_width_chars" && !(0..=i64::from(i32::MAX)).contains(&value)
                {
                    return None;
                }
                if property_name == "size" && !(1..=i64::from(i32::MAX)).contains(&value) {
                    return None;
                }
                if matches!(
                    property_name.as_str(),
                    "layout_transition_ms" | "transition_ms" | "transition_delay_ms"
                ) && !(0..=i64::from(i32::MAX)).contains(&value)
                {
                    return None;
                }
                if property_name == "shadow_blur" && !(0..=i64::from(i32::MAX)).contains(&value) {
                    return None;
                }
                if matches!(
                    property_name.as_str(),
                    "shadow_offset_x" | "shadow_offset_y"
                ) && !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&value)
                {
                    return None;
                }
                if DEVELOPMENT_UI_TRANSFORM_PROPERTIES.contains(&property_name.as_str())
                    && i32::try_from(value).is_err()
                {
                    return None;
                }
                if property_name == "letter_spacing"
                    && !(i64::from(i32::MIN) / 1024..=i64::from(i32::MAX) / 1024).contains(&value)
                {
                    return None;
                }
                if property_name == "line_height_percent"
                    && !(1..=i64::from(i32::MAX)).contains(&value)
                {
                    return None;
                }
                if matches!(
                    property_name.as_str(),
                    "min_width" | "min_height" | "max_width" | "max_height"
                ) && !(1..=i64::from(i32::MAX)).contains(&value)
                {
                    return None;
                }
                if matches!(
                    property_name.as_str(),
                    "margin"
                        | "margin_top"
                        | "margin_bottom"
                        | "margin_start"
                        | "margin_end"
                        | "border_width"
                        | "border_top_width"
                        | "border_bottom_width"
                        | "border_start_width"
                        | "border_end_width"
                        | "padding"
                        | "padding_top"
                        | "padding_bottom"
                        | "padding_start"
                        | "padding_end"
                        | "radius"
                        | "radius_top_left"
                        | "radius_top_right"
                        | "radius_bottom_left"
                        | "radius_bottom_right"
                ) && !(0..=i64::from(i32::MAX)).contains(&value)
                {
                    return None;
                }
                value.to_string()
            } else {
                continue;
            };
            literals.insert((element.name.clone(), property_name), value);
        }
        for property_name in [
            "visible",
            "clip",
            "enabled",
            "primary",
            "accessibility_hidden",
            "read_only",
            "can_shrink",
            "selectable",
            "wrap",
            "bold",
            "italic",
            "underline",
            "strikethrough",
            "password",
            "checked",
            "selected",
            "margin",
        ] {
            if element
                .properties
                .iter()
                .any(|property| typecheck::source_name_to_internal(&property.name) == property_name)
            {
                continue;
            }
            let Some(default) = development_ui_property_lifecycle_default(element, property_name)
            else {
                continue;
            };
            literals.insert((element.name.clone(), property_name.to_string()), default);
        }
    }
    Some(literals)
}

fn development_ui_string_masked_sources(
    analysis: &ProjectAnalysis,
) -> Option<Vec<(PathBuf, String, String)>> {
    let application = analysis.program.application.as_ref()?;
    let view = analysis
        .program
        .views
        .iter()
        .find(|view| view.name == application.view_name)?;
    let masks = view
        .elements
        .iter()
        .flat_map(|element| {
            element.properties.iter().filter(move |property| {
                if development_ui_property_lifecycle_patch_value(element, property).is_some() {
                    return false;
                }
                let property_name = typecheck::source_name_to_internal(&property.name);
                development_ui_string_property_is_patchable(element, &property_name)
                    || development_ui_string_list_property_is_patchable(element, &property_name)
                    || development_ui_bool_property_is_patchable(element, &property_name)
                    || development_ui_i64_property_is_patchable(element, &property_name)
            })
        })
        .filter(|property| {
            matches!(
                property.value.kind,
                ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Int(_) | ExprKind::List(_)
            ) || development_ui_i64_literal_value(&property.value).is_some()
        })
        .map(|property| property.value.span)
        .collect::<Vec<_>>();

    let mut sources = analysis.sources.iter().collect::<Vec<_>>();
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    sources
        .into_iter()
        .map(|source| {
            let mut text = source.text.clone();
            let mut edits = masks
                .iter()
                .copied()
                .filter(|span| span.source_id == source.source_id)
                .map(|span| {
                    source_span_byte_range(&text, span)
                        .map(|(start, end)| (start, end, "\"__flux_hot_string__\"".to_string()))
                })
                .collect::<Option<Vec<_>>>()?;
            for element in &view.elements {
                for property in &element.properties {
                    if property.span.source_id != source.source_id
                        || development_ui_property_lifecycle_patch_value(element, property)
                            .is_none()
                    {
                        continue;
                    }
                    let (start, end) = source_line_byte_range(&text, property.line)?;
                    edits.push((start, end, String::new()));
                }
            }
            if source.source_id == application.keyword_span.source_id {
                let (start, end) = source_span_byte_range(&text, application.span)?;
                edits.push((
                    start,
                    end,
                    development_application_masked_declaration(&text, application)?,
                ));
            }
            if source.source_id == view.name_span.source_id {
                for (line, property) in [
                    (view.grid.gap_line, "gap"),
                    (view.grid.padding_line, "padding"),
                ] {
                    let Some(line) = line else {
                        continue;
                    };
                    let (start, end) =
                        development_grid_declaration_byte_range(&text, line, property)?;
                    edits.push((start, end, String::new()));
                }
            }
            edits.sort_by(|left, right| right.0.cmp(&left.0));
            for (start, end, replacement) in edits {
                text.replace_range(start..end, &replacement);
            }
            let canonical = formatter::format_source(&text).ok()?;
            Some((source.path.clone(), source.module_name.clone(), canonical))
        })
        .collect()
}

fn development_application_masked_declaration(
    source: &str,
    application: &crate::ast::ApplicationDef,
) -> Option<String> {
    let mut metadata = Vec::new();
    for field in &application.metadata {
        if development_application_metadata_lifecycle_patch_value(field).is_some() {
            continue;
        }
        let value = if development_application_metadata_patch_value(field).is_some() {
            "\"__flux_hot_string__\"".to_string()
        } else {
            let (start, end) = source_span_byte_range(source, field.value.span)?;
            source[start..end].to_string()
        };
        metadata.push(format!("{}: {value}", field.name));
    }
    if metadata.is_empty() {
        Some(format!("app {}", application.view_name))
    } else {
        Some(format!(
            "app {}({})",
            application.view_name,
            metadata.join(", ")
        ))
    }
}

fn source_line_byte_range(source: &str, line: usize) -> Option<(usize, usize)> {
    let start = if line <= 1 {
        0
    } else {
        source
            .char_indices()
            .filter(|(_, ch)| *ch == char::from(10))
            .nth(line - 2)
            .map(|(index, _)| index + 1)?
    };
    let line_end = source[start..]
        .find(char::from(10))
        .map(|offset| start + offset)
        .unwrap_or(source.len());
    let end = if line_end < source.len() {
        line_end.checked_add(1)?
    } else {
        line_end
    };
    Some((start, end))
}

fn development_grid_declaration_byte_range(
    source: &str,
    line: usize,
    property: &str,
) -> Option<(usize, usize)> {
    let line_start = if line <= 1 {
        0
    } else {
        source
            .char_indices()
            .filter(|(_, ch)| *ch == char::from(10))
            .nth(line - 2)
            .map(|(index, _)| index + 1)?
    };
    let line_end = source[line_start..]
        .find(char::from(10))
        .map(|offset| line_start + offset)
        .unwrap_or(source.len());
    let raw = &source[line_start..line_end];
    let trimmed = raw.trim_start();
    let grid_prefix = format!("grid {property}:");
    let flow_prefix = format!("flow {property}:");
    let prefix = if trimmed.starts_with(&grid_prefix) {
        grid_prefix.as_str()
    } else if trimmed.starts_with(&flow_prefix) {
        flow_prefix.as_str()
    } else {
        return None;
    };
    let value = trimmed[prefix.len()..].trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let declaration_end = if line_end < source.len() {
        line_end.checked_add(1)?
    } else {
        line_end
    };
    Some((line_start, declaration_end))
}

fn source_span_byte_range(source: &str, span: SourceSpan) -> Option<(usize, usize)> {
    let line_start = if span.line <= 1 {
        0
    } else {
        source
            .char_indices()
            .filter(|(_, ch)| *ch == char::from(10))
            .nth(span.line - 2)
            .map(|(index, _)| index + 1)?
    };
    let start = line_start.checked_add(span.column.checked_sub(1)?)?;
    let end = start.checked_add(span.length)?;
    (end <= source.len() && source.is_char_boundary(start) && source.is_char_boundary(end))
        .then_some((start, end))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DevelopmentStateContract {
    ty: Type,
    text_input_owned: bool,
}

fn development_root_state_contracts(
    analysis: &ProjectAnalysis,
) -> (Option<String>, BTreeMap<String, DevelopmentStateContract>) {
    let Some(application) = analysis.program.application.as_ref() else {
        return (None, BTreeMap::new());
    };
    let Some(view) = analysis
        .program
        .views
        .iter()
        .find(|view| view.name == application.view_name)
    else {
        return (Some(application.view_name.clone()), BTreeMap::new());
    };
    let states = view
        .states
        .iter()
        .map(|state| {
            let ty = analysis.signatures.canonical_type(&state.ty);
            let text_input_owned =
                ty == Type::Str && development_state_accepts_text_input_value(view, &state.name);
            (
                state.name.clone(),
                DevelopmentStateContract {
                    ty,
                    text_input_owned,
                },
            )
        })
        .collect();
    (Some(view.name.clone()), states)
}

fn development_state_accepts_text_input_value(
    view: &crate::ast::ViewDef,
    state_name: &str,
) -> bool {
    view.elements.iter().any(|element| {
        element.kind == "TextInput"
            && ["on_change", "on_submit"].iter().any(|property_name| {
                element.properties.iter().any(|property| {
                    typecheck::source_name_to_internal(&property.name) == *property_name
                        && property.transition.as_ref().is_some_and(|transition| {
                            transition.state == state_name && transition.event_value.is_some()
                        })
                })
            })
    })
}

fn development_state_boundary(
    current: &ProjectAnalysis,
    previous: &ProjectAnalysis,
) -> DevelopmentStateBoundary {
    let (current_root, current_states) = development_root_state_contracts(current);
    let (previous_root, previous_states) = development_root_state_contracts(previous);
    let root_compatible = current_root == previous_root;

    if !root_compatible {
        return DevelopmentStateBoundary {
            root_compatible,
            preserved: Vec::new(),
            reset: current_states.keys().cloned().collect(),
            dropped: previous_states.keys().cloned().collect(),
        };
    }

    let preserved = current_states
        .iter()
        .filter_map(|(name, contract)| {
            (previous_states.get(name) == Some(contract)).then_some(name.clone())
        })
        .collect();
    let reset = current_states
        .iter()
        .filter_map(|(name, contract)| {
            (previous_states.get(name) != Some(contract)).then_some(name.clone())
        })
        .collect();
    let dropped = previous_states
        .keys()
        .filter(|name| !current_states.contains_key(*name))
        .cloned()
        .collect();

    DevelopmentStateBoundary {
        root_compatible,
        preserved,
        reset,
        dropped,
    }
}

fn development_abi_fingerprint(analysis: &ProjectAnalysis) -> u64 {
    use std::fmt::Write as _;

    let mut surface = format!("development-abi-v{};", DEVELOPMENT_ABI_VERSION);
    let mut sources = analysis.sources.iter().collect::<Vec<_>>();
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    for source in sources {
        write!(
            surface,
            "module=({:?},{:?},{});",
            source.path,
            source.module_name,
            module_type_surface(&analysis.program, &analysis.signatures, source.source_id)
        )
        .expect("writing a String cannot fail");
    }

    for view in &analysis.program.views {
        let elements = view
            .elements
            .iter()
            .map(|element| {
                let properties = element
                    .properties
                    .iter()
                    .filter_map(|property| {
                        if development_ui_property_lifecycle_patch_value(element, property)
                            .is_some()
                        {
                            return None;
                        }
                        Some((
                            property.name.as_str(),
                            property.transition.as_ref().map(|transition| {
                                (transition.state.as_str(), transition.event_value.as_deref())
                            }),
                        ))
                    })
                    .collect::<Vec<_>>();
                (
                    element.kind.as_str(),
                    element.name.as_str(),
                    element.row,
                    element.column,
                    element.row_span,
                    element.column_span,
                    properties,
                )
            })
            .collect::<Vec<_>>();
        write!(surface, "view-elements=({:?},{elements:?});", view.name)
            .expect("writing a String cannot fail");
    }

    stable_bytes_hash(surface.as_bytes())
}

fn cached_analysis_is_current(
    target: &Path,
    entry: &CachedProjectAnalysis,
    overlays: &HashMap<PathBuf, String>,
) -> bool {
    if manifest_snapshot(target) != entry.manifest_text
        || entry.package_manifest_texts.iter().any(|(path, expected)| {
            fs::read_to_string(path).map_or(true, |current| &current != expected)
        })
    {
        return false;
    }
    if target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        let Ok(manifest) = read_manifest(target) else {
            return false;
        };
        if validate_lockfile(&manifest).is_err() {
            return false;
        }
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
    package_constants: HashMap<SourceId, BTreeMap<String, typecheck::ConstantValue>>,
    translations: BTreeMap<String, BTreeMap<String, String>>,
}

fn load_report_with_overlays(
    target: &Path,
    overlays: &HashMap<PathBuf, String>,
) -> Result<ProjectLoadReport, Vec<Diagnostic>> {
    load_report_with_overlays_for_target(target, overlays, codegen::NativeTarget::Linux)
}

fn load_report_with_overlays_for_target(
    target: &Path,
    overlays: &HashMap<PathBuf, String>,
    native_target: codegen::NativeTarget,
) -> Result<ProjectLoadReport, Vec<Diagnostic>> {
    load_report_with_overlays_and_parse_cache(target, overlays, None, native_target)
}

fn load_report_with_overlays_and_parse_cache(
    target: &Path,
    overlays: &HashMap<PathBuf, String>,
    parse_cache: Option<&mut ModuleParseCache>,
    native_target: codegen::NativeTarget,
) -> Result<ProjectLoadReport, Vec<Diagnostic>> {
    let (entry, module_root, package_name, dependencies, constants, translations, platform) =
        resolve_project_target(target, native_target)?;
    let has_registry_dependencies = package_name.is_some()
        && crate::package_ecosystem::package_has_registry_dependencies(&module_root).map_err(
            |error| {
                vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("failed to inspect registry dependencies: {error}"),
                )]
            },
        )?;
    let lock_exists = package_name.is_some() && module_root.join("flux.lock").is_file();
    let (registry_releases, registry_roots) = if lock_exists {
        crate::package_ecosystem::materialize_locked_registry_dependencies(&module_root, false)
            .map_err(|error| {
                vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("failed to replay locked registry dependencies: {error}"),
                )]
            })?
    } else if has_registry_dependencies {
        let provider =
            crate::package_ecosystem::configured_registry_provider(false).map_err(|error| {
                vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("failed to configure package registry: {error}"),
                )]
            })?;
        let (graph, roots) = crate::package_ecosystem::materialize_package_registry_dependencies(
            &module_root,
            &provider,
            false,
        )
        .map_err(|error| {
            vec![Diagnostic::global(
                DiagnosticStage::Parse,
                format!("failed to materialize registry dependencies: {error}"),
            )]
        })?;
        (graph.releases, roots)
    } else {
        (BTreeMap::new(), BTreeMap::new())
    };
    let (git_releases, git_roots) =
        if package_name.is_some() && module_root.join("flux.lock").is_file() {
            crate::package_ecosystem::materialize_locked_git_dependencies(&module_root, false)
                .map_err(|error| {
                    vec![Diagnostic::global(
                        DiagnosticStage::Parse,
                        format!("failed to replay locked Git dependencies: {error}"),
                    )]
                })?
        } else {
            (BTreeMap::new(), BTreeMap::new())
        };
    let package_scopes = package_name
        .as_ref()
        .map(|name| {
            vec![PackageScope {
                name: name.clone(),
                root: module_root.clone(),
                dependencies,
                constants,
                platform,
            }]
        })
        .unwrap_or_default();
    let mut loader = Loader {
        loaded: HashSet::new(),
        stack: Vec::new(),
        program: Program::default(),
        sources: Vec::new(),
        diagnostics: Vec::new(),
        package_constants: HashMap::new(),
        module_root,
        package_scopes,
        registry_releases,
        registry_roots,
        git_releases,
        git_roots,
        native_target,
        overlays: overlays.clone(),
        parse_cache,
    };
    loader.load_file(&entry, None);
    Ok(ProjectLoadReport {
        program: loader.program,
        sources: loader.sources,
        diagnostics: loader.diagnostics,
        package_constants: loader.package_constants,
        translations,
    })
}

pub fn analyze(entry: &Path) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    analyze_with_overlays(entry, &HashMap::new())
}

pub fn analyze_for_target(
    entry: &Path,
    native_target: codegen::NativeTarget,
) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    analyze_with_overlays_report(load_report_with_overlays_for_target(
        entry,
        &HashMap::new(),
        native_target,
    )?)
}

pub fn analyze_with_overlays(
    entry: &Path,
    overlays: &HashMap<PathBuf, String>,
) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    analyze_with_overlays_report(load_report_with_overlays(entry, overlays)?)
}

pub fn analyze_package_test(
    package_target: &Path,
    test_entry: &Path,
) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    let manifest_path = if package_target.is_dir() {
        package_target.join("flux.toml")
    } else if package_target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        package_target.to_path_buf()
    } else {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            "package unit tests require a package directory or flux.toml target",
        )]);
    };
    let manifest = read_manifest(&manifest_path)?;
    validate_lockfile(&manifest)?;
    let package_root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    let test_entry = canonical_source(test_entry, "test source")?;
    if !test_entry.starts_with(&package_root) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            "package test source must remain inside the package root",
        )]);
    }
    let has_registry_dependencies = crate::package_ecosystem::package_has_registry_dependencies(
        &package_root,
    )
    .map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("failed to inspect registry dependencies: {error}"),
        )]
    })?;
    let lock_exists = package_root.join("flux.lock").is_file();
    let (registry_releases, registry_roots) = if lock_exists {
        crate::package_ecosystem::materialize_locked_registry_dependencies(&package_root, false)
            .map_err(|error| {
                vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("failed to replay locked registry dependencies: {error}"),
                )]
            })?
    } else if has_registry_dependencies {
        let provider =
            crate::package_ecosystem::configured_registry_provider(false).map_err(|error| {
                vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("failed to configure package registry: {error}"),
                )]
            })?;
        let (graph, roots) = crate::package_ecosystem::materialize_package_registry_dependencies(
            &package_root,
            &provider,
            false,
        )
        .map_err(|error| {
            vec![Diagnostic::global(
                DiagnosticStage::Parse,
                format!("failed to materialize registry dependencies: {error}"),
            )]
        })?;
        (graph.releases, roots)
    } else {
        (BTreeMap::new(), BTreeMap::new())
    };
    let (git_releases, git_roots) = if package_root.join("flux.lock").is_file() {
        crate::package_ecosystem::materialize_locked_git_dependencies(&package_root, false)
            .map_err(|error| {
                vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("failed to replay locked Git dependencies: {error}"),
                )]
            })?
    } else {
        (BTreeMap::new(), BTreeMap::new())
    };
    let mut loader = Loader {
        loaded: HashSet::new(),
        stack: Vec::new(),
        program: Program::default(),
        sources: Vec::new(),
        diagnostics: Vec::new(),
        package_constants: HashMap::new(),
        module_root: package_root.clone(),
        package_scopes: vec![PackageScope {
            name: manifest.name,
            root: package_root,
            dependencies: manifest.dependencies,
            constants: manifest.constants,
            platform: manifest.platform,
        }],
        registry_releases,
        registry_roots,
        git_releases,
        git_roots,
        native_target: codegen::NativeTarget::Linux,
        overlays: HashMap::new(),
        parse_cache: None,
    };
    loader.load_file(&test_entry, None);
    analyze_with_overlays_report(ProjectLoadReport {
        program: loader.program,
        sources: loader.sources,
        diagnostics: loader.diagnostics,
        package_constants: loader.package_constants,
        translations: manifest.translations,
    })
}

fn analyze_with_overlays_report(
    report: ProjectLoadReport,
) -> Result<ProjectAnalysis, Vec<Diagnostic>> {
    if !report.diagnostics.is_empty() {
        return Err(report.diagnostics);
    }
    let signatures =
        typecheck::check_all_with_package_constants(&report.program, &report.package_constants)?;
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
    let (program, sources, package_constants) = match load_report_with_overlays(entry, overlays) {
        Ok(report) => {
            if !report.diagnostics.is_empty() {
                return (report.diagnostics, report.sources);
            }
            (report.program, report.sources, report.package_constants)
        }
        Err(diagnostics) => return (diagnostics, Vec::new()),
    };
    match typecheck::check_all_with_package_constants(&program, &package_constants) {
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
    resolve_project_target(target, codegen::NativeTarget::Linux)
        .map(|(entry, _, _, _, _, _, _)| entry)
}

pub fn development_status_path(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    let entry = resolve_entry(target)?;
    let source_id = SourceId::from_name(entry.to_string_lossy().as_ref());
    Ok(std::env::temp_dir().join(format!("fluxc-run-status-{}.json", source_id.value())))
}

fn resolve_project_target(
    target: &Path,
    native_target: codegen::NativeTarget,
) -> Result<
    (
        PathBuf,
        PathBuf,
        Option<String>,
        BTreeMap<String, PackageDependency>,
        BTreeMap<String, typecheck::ConstantValue>,
        BTreeMap<String, BTreeMap<String, String>>,
        PlatformPackageConfig,
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
        validate_lockfile(&manifest)?;
        let root = manifest
            .path
            .parent()
            .expect("canonical manifest path has a parent")
            .to_path_buf();
        let entry_relative = manifest
            .entry
            .strip_prefix(&root)
            .expect("package entry is inside its package root");
        let entry = manifest
            .platform
            .implementation_for(native_target, entry_relative)
            .map(|implementation| root.join(implementation))
            .unwrap_or_else(|| manifest.entry.clone());
        return Ok((
            entry,
            root,
            Some(manifest.name),
            manifest.dependencies,
            manifest.constants,
            manifest.translations,
            manifest.platform,
        ));
    }
    let entry = canonical_source(target, "entry source")?;
    let root = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok((
        entry,
        root,
        None,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        PlatformPackageConfig::default(),
    ))
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
    let mut assets = None::<String>;
    let mut android_application_id = None::<String>;
    let mut android_version_code = None::<u32>;
    let mut android_min_sdk = None::<u32>;
    let mut android_target_sdk = None::<u32>;
    let mut android_permissions = None::<Vec<String>>;
    let mut android_deep_links = None::<Vec<String>>;
    let mut android_keystore = None::<String>;
    let mut android_key_alias = None::<String>;
    let mut linux_uri_schemes = None::<Vec<String>>;
    let mut linux_file_associations = None::<Vec<String>>;
    let mut native_plugin = None::<bool>;
    let mut native_libraries = None::<Vec<String>>;
    let mut native_search_paths = None::<Vec<String>>;
    let mut platform_linux_modules = None::<BTreeMap<PathBuf, PathBuf>>;
    let mut platform_android_modules = None::<BTreeMap<PathBuf, PathBuf>>;
    let mut platform_windows_modules = None::<BTreeMap<PathBuf, PathBuf>>;
    let mut workspace_members = None::<Vec<String>>;
    let mut dependencies = BTreeMap::<String, PackageDependency>::new();
    let mut constants = BTreeMap::<String, typecheck::ConstantValue>::new();
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
                "package"
                    | "workspace"
                    | "android"
                    | "linux"
                    | "native"
                    | "dependencies"
                    | "constants"
                    | "translations"
                    | "platform.linux"
                    | "platform.android"
                    | "platform.windows"
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
                    "assets" => &mut assets,
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
            Some("workspace") => {
                if key != "members" {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!("unknown [workspace] field '{key}'; only 'members' is supported"),
                    ));
                    continue;
                }
                let value = match parse_manifest_string_array(raw_value) {
                    Ok(value) => value,
                    Err(message) => {
                        diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                        continue;
                    }
                };
                if workspace_members.replace(value).is_some() {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        "duplicate [workspace] field 'members'",
                    ));
                }
            }
            Some("constants") => {
                if !valid_package_constant_name(key) {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!(
                            "invalid package constant name '{key}'; names use Flux identifier syntax"
                        ),
                    ));
                    continue;
                }
                let value = match parse_package_constant(raw_value) {
                    Ok(value) => value,
                    Err(message) => {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("invalid package constant '{key}': {message}"),
                        ));
                        continue;
                    }
                };
                if constants.insert(key.to_string(), value).is_some() {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!("duplicate [constants] entry '{key}'"),
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
            Some("platform.linux") | Some("platform.android") | Some("platform.windows") => {
                if key != "modules" {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!(
                            "unknown [{}] field '{key}'; only 'modules' is supported",
                            section.as_deref().expect("platform section is present")
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
                let modules = match parse_platform_module_map(entries) {
                    Ok(modules) => modules,
                    Err(message) => {
                        diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                        continue;
                    }
                };
                let slot = match section.as_deref() {
                    Some("platform.linux") => &mut platform_linux_modules,
                    Some("platform.android") => &mut platform_android_modules,
                    Some("platform.windows") => &mut platform_windows_modules,
                    _ => unreachable!("platform module section already matched"),
                };
                if slot.replace(modules).is_some() {
                    diagnostics.push(manifest_diagnostic(
                        source_id,
                        line_number,
                        format!(
                            "duplicate [{}] field 'modules'",
                            section.as_deref().expect("platform section is present")
                        ),
                    ));
                }
            }
            Some("native") => match key {
                "plugin" => {
                    let value = match raw_value {
                        "true" => true,
                        "false" => false,
                        _ => {
                            diagnostics.push(manifest_diagnostic(
                                source_id,
                                line_number,
                                "[native].plugin must be true or false",
                            ));
                            continue;
                        }
                    };
                    if native_plugin.replace(value).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            "duplicate [native] field 'plugin'",
                        ));
                    }
                }
                "libraries" | "search_paths" => {
                    let value = match parse_manifest_string_array(raw_value) {
                        Ok(value) => value,
                        Err(message) => {
                            diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                            continue;
                        }
                    };
                    let slot = if key == "libraries" {
                        &mut native_libraries
                    } else {
                        &mut native_search_paths
                    };
                    if slot.replace(value).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("duplicate [native] field '{key}'"),
                        ));
                    }
                }
                _ => diagnostics.push(manifest_diagnostic(
                    source_id,
                    line_number,
                    format!("unknown [native] field '{key}'"),
                )),
            },
            Some("linux") => match key {
                "uri_schemes" | "file_associations" => {
                    let value = match parse_manifest_string_array(raw_value) {
                        Ok(value) => value,
                        Err(message) => {
                            diagnostics.push(manifest_diagnostic(source_id, line_number, message));
                            continue;
                        }
                    };
                    let slot = if key == "uri_schemes" {
                        &mut linux_uri_schemes
                    } else {
                        &mut linux_file_associations
                    };
                    if slot.replace(value).is_some() {
                        diagnostics.push(manifest_diagnostic(
                            source_id,
                            line_number,
                            format!("duplicate [linux] field '{key}'"),
                        ));
                    }
                }
                _ => diagnostics.push(manifest_diagnostic(
                    source_id,
                    line_number,
                    format!("unknown [linux] field '{key}'; expected 'uri_schemes' or 'file_associations'"),
                )),
            },
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
                "manifest fields must be declared inside [package], [dependencies], [constants], [translations], [native], [linux], [android], [platform.linux], [platform.android], or [platform.windows]",
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
    if let Some(schemes) = linux_uri_schemes.as_ref() {
        for scheme in schemes {
            if !valid_desktop_uri_scheme(scheme) {
                diagnostics.push(Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("[linux].uri_schemes entries must be valid URI schemes; invalid value '{scheme}'"),
                ));
            }
        }
    }
    if let Some(associations) = linux_file_associations.as_ref() {
        for association in associations {
            if !valid_mime_type(association) {
                diagnostics.push(Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("[linux].file_associations entries must be MIME types such as 'text/plain'; invalid value '{association}'"),
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
    if let Some(libraries) = native_libraries.as_ref() {
        for library in libraries {
            if !valid_native_library_name(library) {
                diagnostics.push(Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!(
                        "[native].libraries entries must be non-empty logical library names using ASCII letters, digits, '.', '_', '+', or '-'; invalid value '{library}'"
                    ),
                ));
            }
        }
    }
    if let Some(search_paths) = native_search_paths.as_ref() {
        for search_path in search_paths {
            if !valid_native_search_path(search_path) {
                diagnostics.push(Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!(
                        "[native].search_paths entries must be non-empty package-relative paths without '.' or '..' components; invalid value '{search_path}'"
                    ),
                ));
            }
        }
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
    if assets.as_deref().is_some_and(str::is_empty) {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[package].assets cannot be empty",
        ));
    }
    if assets
        .as_deref()
        .is_some_and(|value| Path::new(value).is_absolute())
    {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "[package].assets must be a relative directory path",
        ));
    }
    let workspace_members = workspace_members.unwrap_or_default();
    let mut canonical_workspace_members = Vec::with_capacity(workspace_members.len());
    let mut workspace_member_names = HashSet::new();
    for member in workspace_members {
        let member_path = Path::new(&member);
        if member.is_empty() || member_path.is_absolute() {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Parse,
                "[workspace].members entries must be non-empty relative directories",
            ));
            continue;
        }
        let canonical_member = match canonical_source(
            &canonical_manifest.parent().unwrap().join(member_path),
            "workspace member",
        ) {
            Ok(path) => path,
            Err(member_diagnostics) => {
                diagnostics.extend(member_diagnostics);
                continue;
            }
        };
        if !canonical_member.starts_with(
            canonical_manifest
                .parent()
                .expect("canonical manifest has a parent"),
        ) {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Parse,
                "[workspace].members entries must remain inside the workspace root",
            ));
            continue;
        }
        if !canonical_member.is_dir() {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Parse,
                format!(
                    "workspace member '{}' must be a directory",
                    member_path.display()
                ),
            ));
            continue;
        }
        let member_manifest = canonical_member.join("flux.toml");
        if !member_manifest.is_file() {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Parse,
                format!(
                    "workspace member '{}' must contain flux.toml",
                    member_path.display()
                ),
            ));
            continue;
        }
        if !workspace_member_names.insert(canonical_member.clone()) {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Parse,
                format!(
                    "[workspace].members repeats '{}', including through path normalization",
                    member_path.display()
                ),
            ));
            continue;
        }
        canonical_workspace_members.push(canonical_member);
    }
    canonical_workspace_members.sort();
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let package_root = canonical_manifest
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    let canonical_entry = canonical_source(&package_root.join(entry_path), "package entry")?;
    if !canonical_entry.starts_with(&package_root) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            "[package].entry must remain inside the package root",
        )]);
    }
    let assets = if let Some(asset_path) = assets {
        let canonical_assets = canonical_source(&package_root.join(asset_path), "package assets")?;
        if !canonical_assets.starts_with(&package_root) {
            return Err(vec![Diagnostic::global(
                DiagnosticStage::Parse,
                "[package].assets must remain inside the package root",
            )]);
        }
        if !canonical_assets.is_dir() {
            return Err(vec![Diagnostic::global(
                DiagnosticStage::Parse,
                "[package].assets must reference a directory",
            )]);
        }
        Some(canonical_assets)
    } else {
        None
    };

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
        linux: LinuxPackageConfig {
            uri_schemes: {
                let mut values = linux_uri_schemes.unwrap_or_default();
                values.sort();
                values.dedup();
                values
            },
            file_associations: {
                let mut values = linux_file_associations.unwrap_or_default();
                values.sort();
                values.dedup();
                values
            },
        },
        name,
        version,
        entry: canonical_entry,
        path: canonical_manifest,
        assets,
        dependencies,
        constants,
        translations,
        native: NativePackageConfig {
            plugin: native_plugin.unwrap_or(false),
            libraries: {
                let mut libraries = native_libraries.unwrap_or_default();
                libraries.sort();
                libraries.dedup();
                libraries
            },
            search_paths: {
                let mut paths = native_search_paths
                    .unwrap_or_default()
                    .into_iter()
                    .map(|path| package_root.join(path))
                    .collect::<Vec<_>>();
                paths.sort();
                paths.dedup();
                paths
            },
        },
        platform: PlatformPackageConfig {
            linux_modules: platform_linux_modules.unwrap_or_default(),
            android_modules: platform_android_modules.unwrap_or_default(),
            windows_modules: platform_windows_modules.unwrap_or_default(),
        },
        workspace_members: canonical_workspace_members,
    })
}

/// Read the packages declared by a workspace root in deterministic order.
/// Workspace members are explicit package identities; they are not implicitly
/// available to source imports or dependency resolution.
pub fn read_workspace_members(
    root: &PackageManifest,
) -> Result<Vec<PackageManifest>, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut names = HashSet::from([root.name.clone()]);
    let mut members = Vec::with_capacity(root.workspace_members.len());
    for member_root in &root.workspace_members {
        let manifest_path = member_root.join("flux.toml");
        let member = match read_manifest(&manifest_path) {
            Ok(member) => member,
            Err(member_diagnostics) => {
                diagnostics.extend(member_diagnostics);
                continue;
            }
        };
        if !names.insert(member.name.clone()) {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Parse,
                format!(
                    "workspace member '{}' duplicates package identity '{}'; member names must be unique",
                    member_root.display(),
                    member.name
                ),
            ));
            continue;
        }
        if !member.workspace_members.is_empty() {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Parse,
                format!(
                    "workspace member '{}' cannot declare a nested [workspace] table",
                    member_root.display()
                ),
            ));
            continue;
        }
        members.push(member);
    }
    if diagnostics.is_empty() {
        Ok(members)
    } else {
        Err(diagnostics)
    }
}

/// Return the root package followed by its explicitly declared workspace
/// members. Direct source-file projects return only the supplied source.
pub fn workspace_package_targets(target: &Path) -> Result<Vec<PathBuf>, Vec<Diagnostic>> {
    if !target.is_dir() && target.file_name().and_then(|name| name.to_str()) != Some("flux.toml") {
        return Ok(vec![target.to_path_buf()]);
    }
    let manifest = read_package_manifest_target(target, "workspace discovery")?;
    let root = workspace_root_manifest(&manifest)?;
    let mut targets = vec![root.path.clone()];
    for member in read_workspace_members(&root)? {
        targets.push(member.path);
    }
    targets.sort();
    targets.dedup();
    Ok(targets)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedDependency {
    id: String,
    package: String,
    version: Option<String>,
    source: String,
    sha256: Option<String>,
    asset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedDependencySource {
    Registry,
    Git { url: String, rev: String },
    Path(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedDependency {
    source: ResolvedDependencySource,
    version: Option<String>,
    requirement: Option<String>,
    via: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DependencyGraphNode {
    alias: String,
    package: String,
    version: Option<String>,
    source: String,
    children: Vec<DependencyGraphNode>,
}

fn read_package_manifest_target(
    target: &Path,
    operation: &str,
) -> Result<PackageManifest, Vec<Diagnostic>> {
    let manifest_path = if target.is_dir() {
        target.join("flux.toml")
    } else if target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        target.to_path_buf()
    } else {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("{operation} requires a package directory or flux.toml target"),
        )]);
    };
    read_manifest(&manifest_path)
}

fn workspace_root_manifest(manifest: &PackageManifest) -> Result<PackageManifest, Vec<Diagnostic>> {
    let package_root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    let mut ancestor = package_root.parent();
    while let Some(directory) = ancestor {
        let candidate = directory.join("flux.toml");
        if candidate.is_file() {
            let candidate_manifest = read_manifest(&candidate)?;
            if candidate_manifest
                .workspace_members
                .iter()
                .any(|member| member == package_root)
            {
                return Ok(candidate_manifest);
            }
        }
        ancestor = directory.parent();
    }
    Ok(manifest.clone())
}

pub fn add_dependency(
    target: &Path,
    name: &str,
    dependency: PackageDependency,
) -> Result<PathBuf, Vec<Diagnostic>> {
    if !valid_dependency_name(name) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("invalid dependency name '{name}'"),
        )]);
    }
    let manifest = read_package_manifest_target(target, "add")?;
    let original = fs::read_to_string(&manifest.path).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to read package manifest '{}': {error}",
                manifest.path.display()
            ),
        )]
    })?;
    if manifest.dependencies.contains_key(name) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("dependency '{name}' is already declared"),
        )]);
    }
    mutate_manifest_dependency(target, &manifest, name, Some(&dependency))?;
    match write_lockfile(target) {
        Ok(lock_path) => Ok(lock_path),
        Err(diagnostics) => {
            let _ = fs::write(&manifest.path, original);
            Err(diagnostics)
        }
    }
}

pub fn remove_dependency(target: &Path, name: &str) -> Result<Option<PathBuf>, Vec<Diagnostic>> {
    if !valid_dependency_name(name) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("invalid dependency name '{name}'"),
        )]);
    }
    let manifest = read_package_manifest_target(target, "remove")?;
    let original = fs::read_to_string(&manifest.path).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to read package manifest '{}': {error}",
                manifest.path.display()
            ),
        )]
    })?;
    if !manifest.dependencies.contains_key(name) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("dependency '{name}' is not declared"),
        )]);
    }
    mutate_manifest_dependency(target, &manifest, name, None)?;
    let updated = read_package_manifest_target(target, "remove")?;
    let root = updated
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    if updated.dependencies.is_empty() {
        let lock_path = root.join("flux.lock");
        match fs::remove_file(&lock_path) {
            Ok(()) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                let _ = fs::write(&manifest.path, &original);
                Err(vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!(
                        "failed to remove package lockfile '{}': {error}",
                        lock_path.display()
                    ),
                )])
            }
        }
    } else {
        match write_lockfile(target) {
            Ok(lock_path) => Ok(Some(lock_path)),
            Err(diagnostics) => {
                let _ = fs::write(&manifest.path, &original);
                Err(diagnostics)
            }
        }
    }
}

fn mutate_manifest_dependency(
    target: &Path,
    manifest: &PackageManifest,
    name: &str,
    dependency: Option<&PackageDependency>,
) -> Result<(), Vec<Diagnostic>> {
    let original = fs::read_to_string(&manifest.path).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to read package manifest '{}': {error}",
                manifest.path.display()
            ),
        )]
    })?;
    let rendered = dependency.map(render_package_dependency);
    if let Some(rendered) = rendered.as_deref() {
        parse_package_dependency(rendered).map_err(|message| {
            vec![Diagnostic::global(
                DiagnosticStage::Parse,
                format!("invalid dependency '{name}': {message}"),
            )]
        })?;
    }
    let updated = rewrite_manifest_dependency(&original, name, rendered.as_deref());
    fs::write(&manifest.path, &updated).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to update package manifest '{}': {error}",
                manifest.path.display()
            ),
        )]
    })?;
    if let Err(diagnostics) = read_package_manifest_target(target, "dependency update") {
        let _ = fs::write(&manifest.path, original);
        return Err(diagnostics);
    }
    Ok(())
}

fn rewrite_manifest_dependency(source: &str, name: &str, rendered: Option<&str>) -> String {
    let mut output = Vec::new();
    let mut in_dependencies = false;
    let mut saw_dependencies = false;
    let mut inserted = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_dependencies && rendered.is_some() && !inserted {
                output.push(format!(
                    "{name} = {}",
                    rendered.expect("rendered dependency")
                ));
                inserted = true;
            }
            in_dependencies = trimmed == "[dependencies]";
            saw_dependencies |= in_dependencies;
            output.push(line.to_string());
            continue;
        }
        if in_dependencies {
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim() == name {
                    if let Some(rendered) = rendered {
                        output.push(format!("{name} = {rendered}"));
                        inserted = true;
                    }
                    continue;
                }
            }
        }
        output.push(line.to_string());
    }
    if in_dependencies && rendered.is_some() && !inserted {
        output.push(format!(
            "{name} = {}",
            rendered.expect("rendered dependency")
        ));
        inserted = true;
    }
    if !saw_dependencies && rendered.is_some() {
        if !output.last().is_some_and(|line| line.is_empty()) {
            output.push(String::new());
        }
        output.push("[dependencies]".to_string());
        output.push(format!(
            "{name} = {}",
            rendered.expect("rendered dependency")
        ));
        inserted = true;
    }
    debug_assert!(rendered.is_none() || inserted);
    let mut updated = output.join("\n");
    if source.ends_with('\n') || !updated.is_empty() {
        updated.push('\n');
    }
    updated
}

fn render_package_dependency(dependency: &PackageDependency) -> String {
    match dependency {
        PackageDependency::Registry { requirement } => lock_string(requirement),
        PackageDependency::Path { path, requirement } => {
            let path = lock_string(&lock_path(path));
            requirement.as_ref().map_or_else(
                || format!("{{ path = {path} }}"),
                |requirement| {
                    format!(
                        "{{ path = {path}, version = {} }}",
                        lock_string(requirement)
                    )
                },
            )
        }
        PackageDependency::Git { url, rev } => format!(
            "{{ git = {}, rev = {} }}",
            lock_string(url),
            lock_string(rev)
        ),
    }
}

pub fn write_lockfile(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    let manifest = read_package_manifest_target(target, "lock")?;
    let lock_manifest = workspace_root_manifest(&manifest)?;
    let rendered = render_lockfile(&lock_manifest)?;
    let root = lock_manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    let lock_path = root.join("flux.lock");
    fs::write(&lock_path, rendered).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "failed to write package lockfile '{}': {error}",
                lock_path.display()
            ),
        )]
    })?;
    Ok(lock_path)
}

pub fn ensure_lockfile(target: &Path, locked: bool) -> Result<Option<PathBuf>, Vec<Diagnostic>> {
    if !target.is_dir() && target.file_name().and_then(|name| name.to_str()) != Some("flux.toml") {
        return Ok(None);
    }
    let manifest = read_package_manifest_target(target, "dependency resolution")?;
    let lock_manifest = workspace_root_manifest(&manifest)?;
    if lock_manifest.dependencies.is_empty()
        && lock_manifest.workspace_members.iter().all(|member| {
            read_manifest(&member.join("flux.toml"))
                .map(|manifest| manifest.dependencies.is_empty())
                .unwrap_or(false)
        })
    {
        return Ok(None);
    }
    let root = lock_manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    let lock_path = root.join("flux.lock");
    match validate_lockfile(&lock_manifest) {
        Ok(()) => Ok(Some(lock_path)),
        Err(diagnostics) if locked => Err(diagnostics),
        Err(diagnostics)
            if lockfile_has_exact_registry_entries(&lock_path)
                && crate::package_ecosystem::configured_registry_provider(false)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Err(diagnostics)
        }
        Err(_) => write_lockfile(target).map(Some),
    }
}

pub fn dependency_tree(target: &Path) -> Result<String, Vec<Diagnostic>> {
    let manifest = read_package_manifest_target(target, "tree")?;
    validate_lockfile(&manifest)?;
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    let mut active = HashSet::from([root]);
    let nodes = collect_dependency_graph(&manifest, &mut active)?;
    let mut lines = vec![dependency_display(
        &manifest.name,
        manifest.version.as_deref(),
    )];
    for node in &nodes {
        render_dependency_tree_node(node, 1, &mut lines);
    }
    Ok(lines.join("\n"))
}

pub fn dependency_why(target: &Path, dependency: &str) -> Result<String, Vec<Diagnostic>> {
    if !valid_dependency_name(dependency) {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("invalid dependency name '{dependency}'"),
        )]);
    }
    let manifest = read_package_manifest_target(target, "why")?;
    validate_lockfile(&manifest)?;
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    let mut active = HashSet::from([root]);
    let nodes = collect_dependency_graph(&manifest, &mut active)?;
    let mut paths = Vec::new();
    let mut prefix = vec![manifest.name.clone()];
    collect_dependency_paths(&nodes, dependency, &mut prefix, &mut paths);
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "dependency '{dependency}' is not present in package '{}'",
                manifest.name
            ),
        )]);
    }
    Ok(paths.join("\n"))
}

pub fn fetch_dependencies(target: &Path) -> Result<usize, Vec<Diagnostic>> {
    ensure_lockfile(target, false)?;
    let manifest = read_package_manifest_target(target, "fetch")?;
    validate_lockfile(&manifest)?;
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    let mut active = HashSet::from([root]);
    let nodes = collect_dependency_graph(&manifest, &mut active)?;
    let mut path_count = 0;
    let mut unsupported = Vec::new();
    collect_fetchable_dependencies(&nodes, &mut path_count, &mut unsupported);
    if !unsupported.is_empty() {
        unsupported.sort();
        unsupported.dedup();
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "dependency fetch transport is not available yet for: {}; local path dependencies are already available without copying",
                unsupported.join(", ")
            ),
        )]);
    }
    Ok(path_count)
}

pub fn update_dependencies(target: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    let manifest = read_package_manifest_target(target, "update")?;
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    let mut active = HashSet::from([root]);
    let nodes = collect_dependency_graph(&manifest, &mut active)?;
    let mut path_count = 0;
    let mut unsupported = Vec::new();
    collect_fetchable_dependencies(&nodes, &mut path_count, &mut unsupported);
    if !unsupported.is_empty() {
        unsupported.sort();
        unsupported.dedup();
        return Err(vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "dependency update transport is not available yet for: {}; only local path dependencies can currently be re-resolved",
                unsupported.join(", ")
            ),
        )]);
    }
    write_lockfile(target)
}

pub fn outdated_dependencies(target: &Path) -> Result<String, Vec<Diagnostic>> {
    let manifest = read_package_manifest_target(target, "outdated")?;
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    let mut active = HashSet::from([root]);
    let mut lines = Vec::new();
    collect_outdated_dependencies(&manifest, "", &mut active, &mut lines)?;
    if lines.is_empty() {
        return Ok("no dependencies".to_string());
    }
    lines.sort();
    Ok(lines.join("\n"))
}

fn collect_fetchable_dependencies(
    nodes: &[DependencyGraphNode],
    path_count: &mut usize,
    unsupported: &mut Vec<String>,
) {
    for node in nodes {
        if node.source.starts_with("path:") {
            *path_count += 1;
        } else {
            unsupported.push(format!("{} [{}]", node.package, node.source));
        }
        collect_fetchable_dependencies(&node.children, path_count, unsupported);
    }
}

fn collect_outdated_dependencies(
    manifest: &PackageManifest,
    prefix: &str,
    active: &mut HashSet<PathBuf>,
    lines: &mut Vec<String>,
) -> Result<(), Vec<Diagnostic>> {
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    for (alias, dependency) in &manifest.dependencies {
        let id = if prefix.is_empty() {
            alias.clone()
        } else {
            format!("{prefix}/{alias}")
        };
        match dependency {
            PackageDependency::Registry { requirement } => lines.push(format!(
                "{id}: current ? requirement {requirement} latest unavailable [registry metadata not fetched]"
            )),
            PackageDependency::Git { url, rev } => lines.push(format!(
                "{id}: current {rev} latest unavailable [git:{url}]"
            )),
            PackageDependency::Path { path, requirement } => {
                let dependency_manifest = read_manifest(&root.join(path).join("flux.toml"))?;
                let current = dependency_manifest.version.as_deref().unwrap_or("unversioned");
                let requirement_text = requirement.as_deref().unwrap_or("*");
                let status = match (requirement.as_deref(), dependency_manifest.version.as_deref()) {
                    (Some(requirement), Some(version)) if semver_requirement_matches(requirement, version) => {
                        "compatible"
                    }
                    (Some(_), Some(_)) => "incompatible",
                    (Some(_), None) => "unversioned",
                    (None, _) => "unconstrained",
                };
                lines.push(format!(
                    "{id}: current {current} requirement {requirement_text} {status} [path:{}]",
                    lock_path(path)
                ));
                let dependency_root = dependency_manifest
                    .path
                    .parent()
                    .expect("canonical manifest path has a parent")
                    .to_path_buf();
                if !active.insert(dependency_root.clone()) {
                    return Err(vec![Diagnostic::global(
                        DiagnosticStage::Parse,
                        format!(
                            "cyclic path dependency encountered while inspecting '{}'",
                            dependency_manifest.name
                        ),
                    )]);
                }
                collect_outdated_dependencies(&dependency_manifest, &id, active, lines)?;
                active.remove(&dependency_root);
            }
        }
    }
    Ok(())
}

fn collect_dependency_graph(
    manifest: &PackageManifest,
    active: &mut HashSet<PathBuf>,
) -> Result<Vec<DependencyGraphNode>, Vec<Diagnostic>> {
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    let mut nodes = Vec::new();
    for (alias, dependency) in &manifest.dependencies {
        let node = match dependency {
            PackageDependency::Registry { requirement } => DependencyGraphNode {
                alias: alias.clone(),
                package: alias.clone(),
                version: None,
                source: format!("registry:{requirement}"),
                children: Vec::new(),
            },
            PackageDependency::Git { url, rev } => DependencyGraphNode {
                alias: alias.clone(),
                package: alias.clone(),
                version: None,
                source: format!("git:{url}#{rev}"),
                children: Vec::new(),
            },
            PackageDependency::Path { path, .. } => {
                let dependency_manifest = read_manifest(&root.join(path).join("flux.toml"))?;
                let dependency_root = dependency_manifest
                    .path
                    .parent()
                    .expect("canonical manifest path has a parent")
                    .to_path_buf();
                if !active.insert(dependency_root.clone()) {
                    return Err(vec![Diagnostic::global(
                        DiagnosticStage::Parse,
                        format!(
                            "cyclic path dependency encountered while inspecting '{}'",
                            dependency_manifest.name
                        ),
                    )]);
                }
                let children = collect_dependency_graph(&dependency_manifest, active)?;
                active.remove(&dependency_root);
                DependencyGraphNode {
                    alias: alias.clone(),
                    package: dependency_manifest.name,
                    version: dependency_manifest.version,
                    source: format!("path:{}", lock_path(path)),
                    children,
                }
            }
        };
        nodes.push(node);
    }
    Ok(nodes)
}

fn dependency_display(package: &str, version: Option<&str>) -> String {
    version.map_or_else(
        || package.to_string(),
        |version| format!("{package} {version}"),
    )
}

fn render_dependency_tree_node(node: &DependencyGraphNode, depth: usize, lines: &mut Vec<String>) {
    let identity = if node.alias == node.package {
        dependency_display(&node.package, node.version.as_deref())
    } else {
        format!(
            "{} -> {}",
            node.alias,
            dependency_display(&node.package, node.version.as_deref())
        )
    };
    lines.push(format!(
        "{}{} [{}]",
        "  ".repeat(depth),
        identity,
        node.source
    ));
    for child in &node.children {
        render_dependency_tree_node(child, depth + 1, lines);
    }
}

fn collect_dependency_paths(
    nodes: &[DependencyGraphNode],
    dependency: &str,
    prefix: &mut Vec<String>,
    paths: &mut Vec<String>,
) {
    for node in nodes {
        prefix.push(node.package.clone());
        if node.package == dependency || node.alias == dependency {
            paths.push(prefix.join(" -> "));
        }
        collect_dependency_paths(&node.children, dependency, prefix, paths);
        prefix.pop();
    }
}

fn lockfile_has_exact_registry_entries(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|source| parse_lockfile(&source).ok())
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| exact_registry_source(entry).is_some())
        })
}

fn validate_lockfile(manifest: &PackageManifest) -> Result<(), Vec<Diagnostic>> {
    let lock_manifest = workspace_root_manifest(manifest)?;
    let has_workspace_dependencies = !lock_manifest.dependencies.is_empty()
        || lock_manifest.workspace_members.iter().any(|member| {
            read_manifest(&member.join("flux.toml"))
                .map(|manifest| !manifest.dependencies.is_empty())
                .unwrap_or(false)
        });
    if !has_workspace_dependencies {
        return Ok(());
    }
    let root = lock_manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    let lock_path = root.join("flux.lock");
    let actual = fs::read_to_string(&lock_path).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "package dependencies require a current flux.lock; run 'flux lock {}' ({error})",
                root.display()
            ),
        )]
    })?;
    validate_lockfile_source(&lock_manifest, &actual).map_err(|message| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!(
                "package lockfile '{}' is stale or invalid: {message}; run 'flux lock {}'",
                lock_path.display(),
                root.display()
            ),
        )]
    })
}

fn collect_manifest_lock_entries(
    manifest: &PackageManifest,
    resolve_git: bool,
) -> Result<Vec<LockedDependency>, Vec<Diagnostic>> {
    let lock_manifest = workspace_root_manifest(manifest)?;
    collect_workspace_lock_entries(&lock_manifest, resolve_git)
}

fn collect_workspace_lock_entries(
    manifest: &PackageManifest,
    resolve_git: bool,
) -> Result<Vec<LockedDependency>, Vec<Diagnostic>> {
    let mut entries = Vec::new();
    let mut active = HashSet::new();
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    active.insert(root.clone());
    let mut resolved = BTreeMap::from([(
        manifest.name.clone(),
        ResolvedDependency {
            source: ResolvedDependencySource::Path(root),
            version: manifest.version.clone(),
            requirement: None,
            via: "<root>".to_string(),
        },
    )]);
    collect_locked_dependencies(
        manifest,
        "",
        &mut active,
        &mut resolved,
        &mut entries,
        resolve_git,
    )?;
    for member in read_workspace_members(manifest)? {
        let member_prefix = member.name.clone();
        let member_root = member
            .path
            .parent()
            .expect("canonical member manifest has a parent")
            .to_path_buf();
        if !active.insert(member_root.clone()) {
            return Err(vec![Diagnostic::global(
                DiagnosticStage::Parse,
                format!(
                    "workspace member '{}' repeats an active package root",
                    member_root.display()
                ),
            )]);
        }
        resolved.insert(
            member.name.clone(),
            ResolvedDependency {
                source: ResolvedDependencySource::Path(member_root.clone()),
                version: member.version.clone(),
                requirement: None,
                via: format!("workspace:{member_prefix}"),
            },
        );
        collect_locked_dependencies(
            &member,
            &member_prefix,
            &mut active,
            &mut resolved,
            &mut entries,
            resolve_git,
        )?;
    }
    Ok(entries)
}

fn validate_lockfile_source(manifest: &PackageManifest, source: &str) -> Result<(), String> {
    let actual = parse_lockfile(source)?;
    let expected = collect_manifest_lock_entries(manifest, false).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let actual_by_id = actual
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut registry_identity = BTreeMap::<&str, (&str, &str, &str, &str)>::new();

    for entry in &actual {
        if exact_git_source(entry).is_some() {
            let sha256 = entry
                .sha256
                .as_deref()
                .ok_or_else(|| format!("locked Git package '{}' has no sha256", entry.package))?;
            if !valid_lock_sha256(sha256) {
                return Err(format!(
                    "locked Git package '{}' has invalid sha256 '{sha256}'",
                    entry.package
                ));
            }
        }
        if let Some((repository, version)) = exact_registry_source(entry) {
            let sha256 = entry.sha256.as_deref().ok_or_else(|| {
                format!("locked registry package '{}' has no sha256", entry.package)
            })?;
            if !valid_lock_sha256(sha256) {
                return Err(format!(
                    "locked registry package '{}' has invalid sha256 '{sha256}'",
                    entry.package
                ));
            }
            let asset = entry.asset.as_deref().ok_or_else(|| {
                format!(
                    "locked registry package '{}' has no immutable asset URL",
                    entry.package
                )
            })?;
            if !asset.starts_with("https://") && !asset.starts_with("file://") {
                return Err(format!(
                    "locked registry package '{}' has invalid asset URL '{asset}'",
                    entry.package
                ));
            }
            let identity = (version, repository, sha256, asset);
            if let Some(previous) = registry_identity.insert(&entry.package, identity) {
                if previous != identity {
                    return Err(format!(
                        "locked registry package '{}' resolves to more than one immutable release",
                        entry.package
                    ));
                }
            }
        }
    }

    for expected_entry in &expected {
        let actual_entry = actual_by_id
            .get(expected_entry.id.as_str())
            .ok_or_else(|| {
                format!(
                    "dependency '{}' is missing from the lockfile",
                    expected_entry.id
                )
            })?;
        if let Some(expected_git) = expected_entry.source.strip_prefix("git:") {
            if *actual_entry == expected_entry {
                continue;
            }
            let (expected_url, expected_rev) = expected_git.rsplit_once('#').ok_or_else(|| {
                format!(
                    "invalid Git dependency identity '{}'",
                    expected_entry.source
                )
            })?;
            let (actual_url, actual_rev, _commit) =
                exact_git_source(actual_entry).ok_or_else(|| {
                    format!(
                        "Git dependency '{}' is not pinned to an immutable commit",
                        expected_entry.id
                    )
                })?;
            if actual_url != expected_url || actual_rev != expected_rev {
                return Err(format!(
                    "Git dependency '{}' no longer matches the package manifest source",
                    expected_entry.id
                ));
            }
            if actual_entry
                .sha256
                .as_deref()
                .is_none_or(|sha256| !valid_lock_sha256(sha256))
            {
                return Err(format!(
                    "Git dependency '{}' has no valid immutable content hash",
                    expected_entry.id
                ));
            }
        } else if let Some(requirement) = expected_entry.source.strip_prefix("registry:") {
            if actual_entry.package != expected_entry.package {
                return Err(format!(
                    "dependency '{}' changed package identity from '{}' to '{}'",
                    expected_entry.id, expected_entry.package, actual_entry.package
                ));
            }
            if *actual_entry == expected_entry {
                continue;
            }
            let (_, version) = exact_registry_source(actual_entry).ok_or_else(|| {
                format!(
                    "registry dependency '{}' is not pinned to an immutable release",
                    expected_entry.id
                )
            })?;
            if !semver_requirement_matches(requirement, version) {
                return Err(format!(
                    "registry dependency '{}' locks version '{version}', which no longer satisfies '{requirement}'",
                    expected_entry.id
                ));
            }
        } else if *actual_entry != expected_entry {
            return Err(format!(
                "dependency '{}' no longer matches the package manifest graph",
                expected_entry.id
            ));
        }
    }

    for actual_entry in &actual {
        if expected.iter().any(|entry| entry.id == actual_entry.id) {
            continue;
        }
        if exact_registry_source(actual_entry).is_none() && exact_git_source(actual_entry).is_none()
        {
            return Err(format!(
                "lockfile contains undeclared dependency '{}'",
                actual_entry.id
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct LockEntryBuilder {
    id: Option<String>,
    package: Option<String>,
    version: Option<String>,
    source: Option<String>,
    sha256: Option<String>,
    asset: Option<String>,
}

fn parse_lockfile(source: &str) -> Result<Vec<LockedDependency>, String> {
    let mut format_version = None;
    let mut resolver_version = None;
    let mut current: Option<LockEntryBuilder> = None;
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[dependency]]" {
            if let Some(builder) = current.take() {
                push_lock_entry(builder, &mut entries, &mut ids)?;
            }
            current = Some(LockEntryBuilder::default());
            continue;
        }
        let (key, raw_value) = line
            .split_once('=')
            .map(|(key, value)| (key.trim(), value.trim()))
            .ok_or_else(|| format!("invalid lockfile line '{line}'"))?;
        if let Some(builder) = current.as_mut() {
            let value = parse_lock_string(raw_value)?;
            let slot = match key {
                "id" => &mut builder.id,
                "package" => &mut builder.package,
                "version" => &mut builder.version,
                "source" => &mut builder.source,
                "sha256" => &mut builder.sha256,
                "asset" => &mut builder.asset,
                _ => return Err(format!("unknown lockfile dependency field '{key}'")),
            };
            if slot.replace(value).is_some() {
                return Err(format!("duplicate lockfile dependency field '{key}'"));
            }
        } else {
            let value = raw_value
                .parse::<u32>()
                .map_err(|_| format!("lockfile field '{key}' must be an integer"))?;
            match key {
                "version" if format_version.replace(value).is_none() => {}
                "resolver" if resolver_version.replace(value).is_none() => {}
                "version" | "resolver" => return Err(format!("duplicate lockfile field '{key}'")),
                _ => return Err(format!("unknown lockfile field '{key}'")),
            }
        }
    }
    if let Some(builder) = current.take() {
        push_lock_entry(builder, &mut entries, &mut ids)?;
    }
    if format_version != Some(PACKAGE_LOCK_FORMAT_VERSION) {
        return Err(format!(
            "unsupported lockfile format {:?}; expected {}",
            format_version, PACKAGE_LOCK_FORMAT_VERSION
        ));
    }
    if resolver_version != Some(PACKAGE_RESOLVER_VERSION) {
        return Err(format!(
            "unsupported lockfile resolver {:?}; expected {}",
            resolver_version, PACKAGE_RESOLVER_VERSION
        ));
    }
    Ok(entries)
}

fn push_lock_entry(
    builder: LockEntryBuilder,
    entries: &mut Vec<LockedDependency>,
    ids: &mut BTreeSet<String>,
) -> Result<(), String> {
    let id = builder.id.ok_or("lockfile dependency requires id")?;
    if !ids.insert(id.clone()) {
        return Err(format!("duplicate lockfile dependency id '{id}'"));
    }
    entries.push(LockedDependency {
        id,
        package: builder
            .package
            .ok_or("lockfile dependency requires package")?,
        version: builder.version,
        source: builder
            .source
            .ok_or("lockfile dependency requires source")?,
        sha256: builder.sha256,
        asset: builder.asset,
    });
    Ok(())
}

fn parse_lock_string(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return Err("lockfile string values must be quoted".to_string());
    }
    let mut output = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = chars.next().ok_or("unterminated lockfile string escape")?;
        output.push(match escaped {
            '\\' => '\\',
            '"' => '"',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            _ => return Err(format!("unsupported lockfile string escape '\\{escaped}'")),
        });
    }
    Ok(output)
}

fn exact_registry_source(entry: &LockedDependency) -> Option<(&str, &str)> {
    let source = entry.source.strip_prefix("registry:")?;
    let (repository, version) = source.rsplit_once('#')?;
    if !repository.starts_with("https://")
        || version.is_empty()
        || entry.version.as_deref() != Some(version)
    {
        return None;
    }
    Some((repository, version))
}

fn exact_git_source(entry: &LockedDependency) -> Option<(&str, &str, &str)> {
    let source = entry.source.strip_prefix("git:")?;
    let (requested, commit) = source.rsplit_once("@")?;
    let (url, requested_rev) = requested.rsplit_once("#")?;
    if url.is_empty()
        || requested_rev.is_empty()
        || commit.len() != 40
        || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some((url, requested_rev, commit))
}

fn valid_lock_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn locked_registry_releases(
    target: &Path,
) -> Result<BTreeMap<String, crate::package_ecosystem::RegistryRelease>, Vec<Diagnostic>> {
    let manifest = read_package_manifest_target(target, "locked dependency replay")?;
    validate_lockfile(&manifest)?;
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    let source = fs::read_to_string(root.join("flux.lock")).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("failed to read package lockfile: {error}"),
        )]
    })?;
    let entries = parse_lockfile(&source)
        .map_err(|message| vec![Diagnostic::global(DiagnosticStage::Parse, message)])?;
    let mut releases = BTreeMap::new();
    for entry in entries {
        let Some((repository, version)) = exact_registry_source(&entry) else {
            continue;
        };
        let release = crate::package_ecosystem::RegistryRelease {
            package: entry.package.clone(),
            owner: String::new(),
            repository: repository.to_string(),
            version: version.to_string(),
            flux: "*".to_string(),
            asset: entry
                .asset
                .expect("validated registry lock entry has asset"),
            sha256: entry
                .sha256
                .expect("validated registry lock entry has sha256"),
            dependencies: BTreeMap::new(),
            yanked: false,
        };
        releases.entry(entry.package).or_insert(release);
    }
    Ok(releases)
}

pub fn locked_git_releases(
    target: &Path,
) -> Result<BTreeMap<String, crate::package_ecosystem::GitRelease>, Vec<Diagnostic>> {
    let manifest = read_package_manifest_target(target, "locked Git dependency replay")?;
    validate_lockfile(&manifest)?;
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    let source = fs::read_to_string(root.join("flux.lock")).map_err(|error| {
        vec![Diagnostic::global(
            DiagnosticStage::Parse,
            format!("failed to read package lockfile: {error}"),
        )]
    })?;
    let entries = parse_lockfile(&source)
        .map_err(|message| vec![Diagnostic::global(DiagnosticStage::Parse, message)])?;
    let mut releases = BTreeMap::new();
    for entry in entries {
        let Some((url, requested_rev, commit)) = exact_git_source(&entry) else {
            continue;
        };
        let release = crate::package_ecosystem::GitRelease {
            package: entry.package.clone(),
            url: url.to_string(),
            requested_rev: requested_rev.to_string(),
            commit: commit.to_string(),
            version: entry.version.clone(),
            sha256: entry
                .sha256
                .clone()
                .expect("validated Git lock entry has sha256"),
        };
        let key = format!("{url}#{requested_rev}");
        if let Some(previous) = releases.insert(key.clone(), release.clone()) {
            if previous != release {
                return Err(vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!(
                        "Git dependency source '{key}' resolves to conflicting lock identities"
                    ),
                )]);
            }
        }
    }
    Ok(releases)
}

fn render_lockfile(manifest: &PackageManifest) -> Result<String, Vec<Diagnostic>> {
    let mut entries = collect_manifest_lock_entries(manifest, true)?;

    if entries
        .iter()
        .any(|entry| entry.source.starts_with("registry:"))
    {
        let provider = match crate::package_ecosystem::configured_registry_provider(false) {
            Ok(provider) => Some(provider),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(vec![Diagnostic::global(
                    DiagnosticStage::Parse,
                    format!("failed to configure package registry: {error}"),
                )]);
            }
        };
        if let Some(provider) = provider {
            let mut requirements = BTreeMap::<String, Vec<String>>::new();
            for entry in &entries {
                let Some(requirement) = entry.source.strip_prefix("registry:") else {
                    continue;
                };
                requirements
                    .entry(entry.package.clone())
                    .or_default()
                    .push(requirement.to_string());
            }
            let graph =
                crate::package_ecosystem::resolve_registry_requirements(&provider, requirements)
                    .map_err(|error| {
                        vec![Diagnostic::global(
                            DiagnosticStage::Parse,
                            format!("failed to resolve registry dependencies: {error}"),
                        )]
                    })?;
            for entry in entries
                .iter_mut()
                .filter(|entry| entry.source.starts_with("registry:"))
            {
                let release = graph.releases.get(&entry.package).ok_or_else(|| {
                    vec![Diagnostic::global(
                        DiagnosticStage::Parse,
                        format!(
                            "registry resolver did not produce package '{}'",
                            entry.package
                        ),
                    )]
                })?;
                entry.version = Some(release.version.clone());
                entry.source = format!("registry:{}#{}", release.repository, release.version);
                entry.sha256 = Some(release.sha256.clone());
                entry.asset = Some(release.asset.clone());
            }
            for release in graph.releases.values() {
                if entries.iter().any(|entry| entry.package == release.package) {
                    continue;
                }
                entries.push(LockedDependency {
                    id: release.package.clone(),
                    package: release.package.clone(),
                    version: Some(release.version.clone()),
                    source: format!("registry:{}#{}", release.repository, release.version),
                    sha256: Some(release.sha256.clone()),
                    asset: Some(release.asset.clone()),
                });
            }
        }
    }

    entries.sort_by(|left, right| left.id.cmp(&right.id));

    let mut output = format!(
        "# Generated by Flux. Do not edit.\nversion = {}\nresolver = {}\n",
        PACKAGE_LOCK_FORMAT_VERSION, PACKAGE_RESOLVER_VERSION
    );
    for entry in entries {
        output.push_str("\n[[dependency]]\n");
        output.push_str(&format!("id = {}\n", lock_string(&entry.id)));
        output.push_str(&format!("package = {}\n", lock_string(&entry.package)));
        if let Some(version) = entry.version {
            output.push_str(&format!("version = {}\n", lock_string(&version)));
        }
        output.push_str(&format!("source = {}\n", lock_string(&entry.source)));
        if let Some(sha256) = entry.sha256 {
            output.push_str(&format!("sha256 = {}\n", lock_string(&sha256)));
        }
        if let Some(asset) = entry.asset {
            output.push_str(&format!("asset = {}\n", lock_string(&asset)));
        }
    }
    Ok(output)
}

fn collect_locked_dependencies(
    manifest: &PackageManifest,
    prefix: &str,
    active: &mut HashSet<PathBuf>,
    resolved: &mut BTreeMap<String, ResolvedDependency>,
    entries: &mut Vec<LockedDependency>,
    resolve_git: bool,
) -> Result<(), Vec<Diagnostic>> {
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent");
    for (name, dependency) in &manifest.dependencies {
        let id = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        match dependency {
            PackageDependency::Registry { requirement } => {
                register_dependency_resolution(
                    resolved,
                    name,
                    ResolvedDependency {
                        source: ResolvedDependencySource::Registry,
                        version: None,
                        requirement: Some(requirement.clone()),
                        via: id.clone(),
                    },
                )?;
                entries.push(LockedDependency {
                    id,
                    package: name.clone(),
                    version: None,
                    source: format!("registry:{requirement}"),
                    sha256: None,
                    asset: None,
                });
            }
            PackageDependency::Git { url, rev } => {
                if !resolve_git {
                    register_dependency_resolution(
                        resolved,
                        name,
                        ResolvedDependency {
                            source: ResolvedDependencySource::Git {
                                url: url.clone(),
                                rev: rev.clone(),
                            },
                            version: None,
                            requirement: None,
                            via: id.clone(),
                        },
                    )?;
                    entries.push(LockedDependency {
                        id,
                        package: name.clone(),
                        version: None,
                        source: format!("git:{url}#{rev}"),
                        sha256: None,
                        asset: None,
                    });
                    continue;
                }
                let release = crate::package_ecosystem::resolve_git_release(name, url, rev)
                    .map_err(|error| {
                        vec![Diagnostic::global(
                            DiagnosticStage::Parse,
                            format!("failed to resolve Git dependency '{id}': {error}"),
                        )]
                    })?;
                register_dependency_resolution(
                    resolved,
                    &release.package,
                    ResolvedDependency {
                        source: ResolvedDependencySource::Git {
                            url: release.url.clone(),
                            rev: release.commit.clone(),
                        },
                        version: release.version.clone(),
                        requirement: None,
                        via: id.clone(),
                    },
                )?;
                entries.push(LockedDependency {
                    id: id.clone(),
                    package: release.package.clone(),
                    version: release.version.clone(),
                    source: format!(
                        "git:{}#{}@{}",
                        release.url, release.requested_rev, release.commit
                    ),
                    sha256: Some(release.sha256.clone()),
                    asset: None,
                });
                let dependency_root = crate::package_ecosystem::materialize_git_release(
                    &release, true,
                )
                .map_err(|error| {
                    vec![Diagnostic::global(
                        DiagnosticStage::Parse,
                        format!("failed to materialize Git dependency '{id}': {error}"),
                    )]
                })?;
                let dependency_manifest = read_manifest(&dependency_root.join("flux.toml"))?;
                if !active.insert(dependency_root.clone()) {
                    return Err(vec![Diagnostic::global(
                        DiagnosticStage::Parse,
                        format!("cyclic Git dependency detected at dependency path '{id}'"),
                    )]);
                }
                collect_locked_dependencies(
                    &dependency_manifest,
                    &id,
                    active,
                    resolved,
                    entries,
                    resolve_git,
                )?;
                active.remove(&dependency_root);
            }
            PackageDependency::Path { path, requirement } => {
                let dependency_manifest = root.join(path).join("flux.toml");
                let dependency_manifest = read_manifest(&dependency_manifest)?;
                if let Some(requirement) = requirement {
                    let Some(version) = dependency_manifest.version.as_deref() else {
                        return Err(vec![Diagnostic::global(
                            DiagnosticStage::Parse,
                            format!(
                                "dependency path '{id}' requires version '{requirement}', but path package has no [package].version"
                            ),
                        )]);
                    };
                    if !semver_requirement_matches(requirement, version) {
                        return Err(vec![Diagnostic::global(
                            DiagnosticStage::Parse,
                            format!(
                                "dependency path '{id}' requires version '{requirement}', but path package '{}' is version '{version}'",
                                dependency_manifest.name
                            ),
                        )]);
                    }
                }
                let dependency_root = dependency_manifest
                    .path
                    .parent()
                    .expect("canonical manifest path has a parent")
                    .to_path_buf();
                register_dependency_resolution(
                    resolved,
                    &dependency_manifest.name,
                    ResolvedDependency {
                        source: ResolvedDependencySource::Path(dependency_root.clone()),
                        version: dependency_manifest.version.clone(),
                        requirement: requirement.clone(),
                        via: id.clone(),
                    },
                )?;
                entries.push(LockedDependency {
                    id: id.clone(),
                    package: dependency_manifest.name.clone(),
                    version: dependency_manifest.version.clone(),
                    source: format!("path:{}", lock_path(path)),
                    sha256: None,
                    asset: None,
                });
                if !active.insert(dependency_root.clone()) {
                    return Err(vec![Diagnostic::global(
                        DiagnosticStage::Parse,
                        format!(
                            "cyclic path dependency detected at dependency path '{id}' while resolving package '{}'",
                            dependency_manifest.name
                        ),
                    )]);
                }
                collect_locked_dependencies(
                    &dependency_manifest,
                    &id,
                    active,
                    resolved,
                    entries,
                    resolve_git,
                )?;
                active.remove(&dependency_root);
            }
        }
    }
    Ok(())
}

fn register_dependency_resolution(
    resolved: &mut BTreeMap<String, ResolvedDependency>,
    package: &str,
    current: ResolvedDependency,
) -> Result<(), Vec<Diagnostic>> {
    let Some(previous) = resolved.get(package) else {
        resolved.insert(package.to_string(), current);
        return Ok(());
    };

    let compatible = match (&previous.source, &current.source) {
        (ResolvedDependencySource::Path(left), ResolvedDependencySource::Path(right)) => {
            left == right && previous.version == current.version
        }
        (
            ResolvedDependencySource::Git {
                url: left_url,
                rev: left_rev,
            },
            ResolvedDependencySource::Git {
                url: right_url,
                rev: right_rev,
            },
        ) => left_url == right_url && left_rev == right_rev,
        (ResolvedDependencySource::Registry, ResolvedDependencySource::Registry) => {
            registry_requirements_compatible(
                previous.requirement.as_deref().unwrap_or("*"),
                current.requirement.as_deref().unwrap_or("*"),
            )
        }
        _ => false,
    };
    if compatible {
        return Ok(());
    }

    Err(vec![Diagnostic::global(
        DiagnosticStage::Parse,
        format!(
            "dependency resolution conflict for package '{package}': dependency path '{}' resolves {}, but dependency path '{}' resolves {}; one package name must resolve to one compatible source/version",
            previous.via,
            describe_dependency_resolution(previous),
            current.via,
            describe_dependency_resolution(&current),
        ),
    )])
}

fn registry_requirements_compatible(left: &str, right: &str) -> bool {
    if left == "*" && right == "*" {
        return true;
    }
    requirement_candidate_points(left)
        .into_iter()
        .chain(requirement_candidate_points(right))
        .any(|candidate| {
            semver_requirement_matches(left, &candidate)
                && semver_requirement_matches(right, &candidate)
        })
}

fn requirement_candidate_points(requirement: &str) -> Vec<String> {
    if requirement == "*" {
        return Vec::new();
    }
    let base = requirement
        .strip_prefix('^')
        .or_else(|| requirement.strip_prefix('~'))
        .unwrap_or(requirement);
    let mut candidates = vec![base.to_string()];
    if let Some(version) = SemanticVersion::parse(base) {
        if !version.prerelease.is_empty() {
            candidates.push(format!(
                "{}.{}.{}",
                version.major, version.minor, version.patch
            ));
        }
    }
    candidates
}

fn describe_dependency_resolution(resolution: &ResolvedDependency) -> String {
    match &resolution.source {
        ResolvedDependencySource::Registry => format!(
            "registry requirement '{}'",
            resolution.requirement.as_deref().unwrap_or("*")
        ),
        ResolvedDependencySource::Git { url, rev } => format!("Git source '{url}#{rev}'"),
        ResolvedDependencySource::Path(_) => match resolution.version.as_deref() {
            Some(version) => format!("path package version '{version}'"),
            None => "unversioned path package".to_string(),
        },
    }
}

fn lock_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn lock_string(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
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

fn valid_package_constant_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && !value.starts_with("flux__")
        && !matches!(
            value,
            "fn" | "struct"
                | "enum"
                | "type"
                | "const"
                | "let"
                | "match"
                | "return"
                | "if"
                | "elif"
                | "else"
                | "for"
                | "while"
                | "in"
                | "var"
                | "break"
                | "continue"
                | "true"
                | "false"
                | "nil"
                | "none"
                | "error"
        )
}

fn valid_native_library_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn valid_native_search_path(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
}

fn valid_platform_module_path(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path.extension().and_then(|extension| extension.to_str()) == Some("flux")
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
}

fn parse_platform_module_map(entries: Vec<String>) -> Result<BTreeMap<PathBuf, PathBuf>, String> {
    let mut modules = BTreeMap::new();
    for entry in entries {
        let Some((module, implementation)) = entry.split_once('=') else {
            return Err(
                "platform module entries use 'shared/module.flux=platform/module.flux' strings"
                    .to_string(),
            );
        };
        let module = module.trim();
        let implementation = implementation.trim();
        if !valid_platform_module_path(module) || !valid_platform_module_path(implementation) {
            return Err(format!(
                "platform module paths must be normalized package-relative '.flux' paths; invalid value '{entry}'"
            ));
        }
        let module = PathBuf::from(module);
        if modules
            .insert(module.clone(), PathBuf::from(implementation))
            .is_some()
        {
            return Err(format!(
                "platform module '{}' is mapped more than once",
                module.display()
            ));
        }
    }
    Ok(modules)
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
    resolve_semver_requirements([requirement], candidates)
}

pub fn resolve_semver_requirements<'a, 'b>(
    requirements: impl IntoIterator<Item = &'a str>,
    candidates: impl IntoIterator<Item = &'b str>,
) -> Result<Option<String>, String> {
    let requirements = requirements.into_iter().collect::<Vec<_>>();
    for requirement in &requirements {
        if !valid_semver_requirement(requirement) {
            return Err(format!("invalid SemVer requirement '{requirement}'"));
        }
    }
    let mut best = None::<(SemanticVersion, String)>;
    for candidate in candidates {
        let parsed = SemanticVersion::parse(candidate)
            .ok_or_else(|| format!("invalid SemVer candidate '{candidate}'"))?;
        if !requirements
            .iter()
            .all(|requirement| semver_requirement_matches(requirement, candidate))
        {
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

fn parse_package_constant(text: &str) -> Result<typecheck::ConstantValue, String> {
    let text = text.trim();
    if text.starts_with('"') {
        return parse_manifest_string(text).map(typecheck::ConstantValue::Str);
    }
    match text {
        "true" => return Ok(typecheck::ConstantValue::Bool(true)),
        "false" => return Ok(typecheck::ConstantValue::Bool(false)),
        _ => {}
    }
    text.parse::<i64>()
        .map(typecheck::ConstantValue::I64)
        .map_err(|_| "values must be an i64, bool, or quoted string literal".to_string())
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

fn valid_desktop_uri_scheme(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn valid_mime_type(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    !kind.is_empty()
        && !subtype.is_empty()
        && kind
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        && subtype
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'*'))
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
    constants: BTreeMap<String, typecheck::ConstantValue>,
    platform: PlatformPackageConfig,
}

impl PackageScope {
    fn resolve_module(&self, target: codegen::NativeTarget, requested: &Path) -> PathBuf {
        let Ok(relative) = requested.strip_prefix(&self.root) else {
            return requested.to_path_buf();
        };
        self.platform
            .implementation_for(target, relative)
            .map(|implementation| self.root.join(implementation))
            .unwrap_or_else(|| requested.to_path_buf())
    }

    fn logical_module_path(&self, target: codegen::NativeTarget, actual: &Path) -> PathBuf {
        let relative = actual.strip_prefix(&self.root).unwrap_or(actual);
        self.platform
            .modules_for(target)
            .iter()
            .find_map(|(logical, implementation)| {
                (implementation == relative).then(|| logical.clone())
            })
            .unwrap_or_else(|| relative.to_path_buf())
    }
}

struct Loader<'a> {
    loaded: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
    program: Program,
    sources: Vec<ProjectSource>,
    diagnostics: Vec<Diagnostic>,
    package_constants: HashMap<SourceId, BTreeMap<String, typecheck::ConstantValue>>,
    module_root: PathBuf,
    package_scopes: Vec<PackageScope>,
    registry_releases: BTreeMap<String, crate::package_ecosystem::RegistryRelease>,
    registry_roots: BTreeMap<String, PathBuf>,
    git_releases: BTreeMap<String, crate::package_ecosystem::GitRelease>,
    git_roots: BTreeMap<String, PathBuf>,
    native_target: codegen::NativeTarget,
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
        if let Some(package) = &current_package {
            self.package_constants
                .insert(source_id, package.constants.clone());
        }
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
            let requested = if let Some(package) = current_package.as_ref() {
                let logical_source = package
                    .root
                    .join(package.logical_module_path(self.native_target, &canonical));
                logical_source
                    .parent()
                    .unwrap_or(package.root.as_path())
                    .join(import_path)
            } else {
                canonical
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(import_path)
            };
            let resolved = current_package
                .as_ref()
                .map(|package| package.resolve_module(self.native_target, &requested))
                .unwrap_or(requested);
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
        if dependency_name == "self" {
            let resolved = current_package.root.join(module_path);
            if fs::canonicalize(&resolved)
                .ok()
                .is_some_and(|path| !path.starts_with(&current_package.root))
            {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticStage::Parse,
                    span,
                    "package self-imports must remain inside the package root",
                ));
                return None;
            }
            return Some(resolved);
        }
        let Some(dependency) = current_package.dependencies.get(dependency_name) else {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticStage::Parse,
                span,
                format!("package dependency '{dependency_name}' is not declared in [dependencies]"),
            ));
            return None;
        };
        let (manifest, dependency_root) = match dependency {
            PackageDependency::Path { path, requirement } => {
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
                            format!(
                                "failed to load package dependency '{dependency_name}': {detail}"
                            ),
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
                (manifest, dependency_root)
            }
            PackageDependency::Registry { requirement } => {
                let Some(release) = self.registry_releases.get(dependency_name) else {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        span,
                        format!(
                            "package dependency '{dependency_name}' requires dependency resolution before it can be imported; refresh flux.lock from the public registry or configure a registry override"
                        ),
                    ));
                    return None;
                };
                if !semver_requirement_matches(requirement, &release.version) {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        span,
                        format!(
                            "registry dependency '{dependency_name}' requires '{requirement}', but flux.lock resolved '{}'",
                            release.version
                        ),
                    ));
                    return None;
                }
                let Some(dependency_root) = self.registry_roots.get(dependency_name).cloned()
                else {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        span,
                        format!("registry dependency '{dependency_name}' is not materialized in the package cache"),
                    ));
                    return None;
                };
                let manifest = match read_manifest(&dependency_root.join("flux.toml")) {
                    Ok(manifest) => manifest,
                    Err(diagnostics) => {
                        let detail = diagnostics
                            .first()
                            .map(|diagnostic| diagnostic.message.as_str())
                            .unwrap_or("invalid dependency manifest");
                        self.diagnostics.push(Diagnostic::new(
                            DiagnosticStage::Parse,
                            span,
                            format!(
                                "failed to load registry dependency '{dependency_name}': {detail}"
                            ),
                        ));
                        return None;
                    }
                };
                if manifest.name != release.package
                    || manifest.version.as_deref() != Some(&release.version)
                {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        span,
                        format!(
                            "registry dependency '{dependency_name}' materialized as {} {}, expected {} {}",
                            manifest.name,
                            manifest.version.as_deref().unwrap_or("<none>"),
                            release.package,
                            release.version
                        ),
                    ));
                    return None;
                }
                (manifest, dependency_root)
            }
            PackageDependency::Git { url, rev } => {
                let key = format!("{url}#{rev}");
                let Some(release) = self.git_releases.get(&key) else {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        span,
                        format!("Git dependency '{dependency_name}' is missing an immutable flux.lock entry"),
                    ));
                    return None;
                };
                let Some(dependency_root) = self.git_roots.get(&key).cloned() else {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        span,
                        format!("Git dependency '{dependency_name}' is not materialized in the package cache"),
                    ));
                    return None;
                };
                let manifest = match read_manifest(&dependency_root.join("flux.toml")) {
                    Ok(manifest) => manifest,
                    Err(diagnostics) => {
                        let detail = diagnostics
                            .first()
                            .map(|diagnostic| diagnostic.message.as_str())
                            .unwrap_or("invalid dependency manifest");
                        self.diagnostics.push(Diagnostic::new(
                            DiagnosticStage::Parse,
                            span,
                            format!("failed to load Git dependency '{dependency_name}': {detail}"),
                        ));
                        return None;
                    }
                };
                if manifest.name != release.package || manifest.version != release.version {
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticStage::Parse,
                        span,
                        format!("Git dependency '{dependency_name}' materialized with a manifest that does not match flux.lock"),
                    ));
                    return None;
                }
                (manifest, dependency_root)
            }
        };
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
                name: manifest.name.clone(),
                root: dependency_root.clone(),
                dependencies: manifest.dependencies.clone(),
                constants: manifest.constants.clone(),
                platform: manifest.platform.clone(),
            });
        }
        let selected_module = manifest
            .platform
            .implementation_for(self.native_target, module_path)
            .cloned()
            .unwrap_or_else(|| module_path.to_path_buf());
        let resolved = dependency_root.join(selected_module);
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
        let relative = package
            .map(|package| package.logical_module_path(self.native_target, path))
            .unwrap_or_else(|| path.strip_prefix(module_root).unwrap_or(path).to_path_buf());
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
        self.program.routes.append(&mut program.routes);
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

#[cfg(test)]
mod tests {
    use super::{module_type_surface, remove_cache_artifact_if_unchanged};
    use crate::diagnostic::SourceId;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "flux-project-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn cache_cleanup_removes_the_inspected_artifact() {
        let path = test_path("cache-cleanup");
        fs::write(&path, "stale").expect("cache fixture should be writable");

        remove_cache_artifact_if_unchanged(&path, "stale");

        assert!(!path.exists());
    }

    #[test]
    fn cache_cleanup_preserves_a_concurrent_replacement() {
        let path = test_path("cache-replacement");
        fs::write(&path, "fresh").expect("cache fixture should be writable");

        remove_cache_artifact_if_unchanged(&path, "stale");

        assert_eq!(fs::read_to_string(&path).unwrap(), "fresh");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn semantic_surface_includes_constant_expression_without_source_spans() {
        let first = crate::parser::parse_with_source(
            "pub const LIMIT: i64 = 3\nfn main() -> i64 {\n    return LIMIT\n}\n",
            SourceId::new(41),
        )
        .expect("constant fixture should parse");
        let second = crate::parser::parse_with_source(
            "\n\npub const LIMIT: i64 = 4\nfn main() -> i64 {\n    return LIMIT\n}\n",
            SourceId::new(41),
        )
        .expect("shifted constant fixture should parse");
        let first_signatures =
            crate::typecheck::check(&first).expect("first fixture should typecheck");
        let second_signatures =
            crate::typecheck::check(&second).expect("second fixture should typecheck");

        assert_ne!(
            module_type_surface(&first, &first_signatures, SourceId::new(41)),
            module_type_surface(&second, &second_signatures, SourceId::new(41)),
            "a changed constant must invalidate semantic reuse"
        );
        assert_eq!(
            module_type_surface(&first, &first_signatures, SourceId::new(41)),
            module_type_surface(
                &crate::parser::parse_with_source(
                    "\npub const LIMIT: i64 = 3\nfn main() -> i64 {\n    return LIMIT\n}\n",
                    SourceId::new(41),
                )
                .expect("shifted equivalent fixture should parse"),
                &first_signatures,
                SourceId::new(41),
            ),
            "source-only line shifts must not invalidate the semantic surface"
        );
    }

    #[test]
    fn changed_imported_constant_forces_fresh_semantic_analysis() {
        let root = test_path("constant-invalidation");
        fs::create_dir_all(&root).expect("temporary cache project should be writable");
        let dependency = root.join("constants.flux");
        let entry = root.join("main.flux");
        fs::write(&dependency, "pub const LIMIT: i64 = 3\n")
            .expect("constant dependency should be writable");
        fs::write(
            &entry,
            "import \"constants.flux\"\nfn main() -> i64 {\n    return LIMIT\n}\n",
        )
        .expect("cache entry should be writable");

        let mut cache = super::ProjectAnalysisCache::default();
        let overlays = std::collections::HashMap::new();
        cache
            .analyze_with_overlays(&entry, &overlays)
            .expect("initial project should analyze");
        fs::write(&dependency, "pub const LIMIT: i64 = 4\n")
            .expect("changed constant dependency should be writable");
        cache
            .invalidate_path(&fs::canonicalize(&dependency).expect("constant path should resolve"));
        cache
            .analyze_with_overlays(&entry, &overlays)
            .expect("changed constant project should analyze");

        assert_eq!(cache.incremental_typecheck_stats().full_runs, 2);
        assert_eq!(cache.incremental_typecheck_stats().runs, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn overlay_path_spellings_share_canonical_incremental_cache_identity() {
        let root = test_path("overlay-path-identity");
        fs::create_dir_all(&root).expect("temporary cache project should be writable");
        let dependency = root.join("dependency.flux");
        let entry = root.join("main.flux");
        fs::write(&dependency, "pub fn value() -> i64 {\n    return 1\n}\n")
            .expect("dependency should be writable");
        fs::write(
            &entry,
            "import \"dependency.flux\"\nfn main() -> i64 {\n    return value()\n}\n",
        )
        .expect("entry should be writable");

        let mut cache = super::ProjectAnalysisCache::default();
        cache
            .analyze_with_overlays(&entry, &HashMap::new())
            .expect("initial project should analyze");

        let noncanonical = root.join(".").join("dependency.flux");
        let mut overlays = HashMap::new();
        overlays.insert(
            noncanonical,
            "pub fn value() -> i64 {\n    return 2\n}\n".to_string(),
        );
        cache
            .analyze_with_overlays(&entry, &overlays)
            .expect("overlay project should analyze");

        assert_eq!(cache.incremental_typecheck_stats().full_runs, 1);
        assert_eq!(cache.incremental_typecheck_stats().runs, 1);
        assert_eq!(cache.incremental_typecheck_stats().rechecked_modules, 1);
        assert_eq!(
            cache.module_parse_stats().hits,
            1,
            "unchanged entry parse is reused while the changed overlay is reparsed"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn durable_codegen_cache_reuses_canonical_format_and_invalidates_body_edits() {
        let root = test_path("durable-codegen-cache");
        fs::create_dir_all(&root).expect("temporary cache project should be writable");
        let entry = root.join("main.flux");
        fs::write(
            &entry,
            "fn helper() -> i64 {\n    return 3\n}\nfn main() -> i64 {\n    let value: i64 = 7\n    return value\n}\n",
        )
        .expect("initial source should be writable");

        let first = super::analyze(&entry).expect("initial project should analyze");
        let first_c = first
            .emit_c_cached_for_target(&root, crate::codegen::NativeTarget::Linux)
            .expect("initial generated C should be emitted");
        let cache_dir = root.join(".flux/cache");
        let initial_entries = fs::read_dir(&cache_dir)
            .expect("codegen cache directory should exist")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("codegen-") && name.ends_with(".c"))
            })
            .count();
        assert_eq!(
            initial_entries, 1,
            "one durable artifact should be published"
        );
        let ir_dir = root.join(".flux/ir-cache");
        let ir_entries = fs::read_dir(&ir_dir)
            .expect("typed IR cache directory should exist")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("ir-") && name.ends_with(".manifest"))
            })
            .count();
        assert_eq!(ir_entries, 1, "one typed IR manifest should be published");
        let function_ir_entries = fs::read_dir(&ir_dir)
            .expect("typed IR cache directory should contain function artifacts")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("function-") && name.ends_with(".manifest")
                })
            })
            .count();
        assert_eq!(
            function_ir_entries, 2,
            "one function IR artifact should be published per function"
        );
        // Function names are not globally unique: a project may contain the
        // same declaration name in different source modules.  A manifest
        // validated by name alone could therefore be accepted for the wrong
        // module and leave tooling with stale normalized facts.
        let fingerprint =
            super::codegen_cache_fingerprint(&first, crate::codegen::NativeTarget::Linux);
        let mut wrong_module = first.clone();
        wrong_module.sources[0].module_name.push_str(".changed");
        assert!(
            !super::typed_ir_manifest_is_current(
                &wrong_module,
                &root,
                crate::codegen::NativeTarget::Linux,
                fingerprint,
            ),
            "typed IR cache validation must include source module identity"
        );
        // A valid native cache hit must repair independently cleaned IR
        // metadata instead of silently leaving tooling without a manifest.
        fs::remove_dir_all(&ir_dir).expect("typed IR cache should be removable");
        let repaired_c = first
            .emit_c_cached_for_target(&root, crate::codegen::NativeTarget::Linux)
            .expect("native cache hit should repair missing typed IR metadata");
        assert_eq!(repaired_c, first_c);
        let repaired_ir_entries = fs::read_dir(&ir_dir)
            .expect("repaired typed IR directory should be readable")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("ir-"))
            .count();
        assert_eq!(
            repaired_ir_entries, 1,
            "native cache hit should republish the aggregate typed IR manifest"
        );
        let ir_path = fs::read_dir(&ir_dir)
            .expect("typed IR cache directory should remain readable")
            .filter_map(Result::ok)
            .find(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("ir-") && name.ends_with(".manifest")
            })
            .expect("typed IR manifest should be discoverable")
            .path();
        let ir_manifest =
            fs::read_to_string(ir_path).expect("typed IR manifest should be readable");
        assert!(ir_manifest.starts_with("flux-project-typed-ir-v4:"));
        assert!(
            ir_manifest.contains("\nflux-project-typed-ir-v4\nlinux\nfunction\thelper\tmodule=")
        );
        assert!(
            ir_manifest.contains("function\tmain\tmodule=") && ir_manifest.contains("\tshape=")
        );
        assert!(ir_manifest.contains("\tnodes="));
        assert!(ir_manifest.contains("\tborrows="));
        assert!(ir_manifest.contains("\tdrops="));
        let first_function_shape = ir_manifest
            .lines()
            .find(|line| line.starts_with("function\tmain\t"))
            .expect("manifest should contain main function identity")
            .to_string();

        fs::write(
            &entry,
            "fn helper() -> i64 {\n    return 3\n}\nfn main() -> i64 {\n  let value: i64 = 7\n  return value\n}\n",
        )
        .expect("formatting-only source edit should be writable");
        let formatted = super::analyze(&entry).expect("formatted project should analyze");
        let formatted_c = formatted
            .emit_c_cached_for_target(&root, crate::codegen::NativeTarget::Linux)
            .expect("formatted generated C should be emitted");
        assert_eq!(
            formatted_c, first_c,
            "canonical formatting should reuse the artifact"
        );
        let formatted_entries = fs::read_dir(&cache_dir)
            .expect("codegen cache directory should remain available")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("codegen-") && name.ends_with(".c"))
            })
            .count();
        assert_eq!(
            formatted_entries, 1,
            "formatting should not add a cache artifact"
        );
        let formatted_ir = fs::read_dir(&ir_dir)
            .expect("typed IR cache directory should remain readable after formatting")
            .filter_map(Result::ok)
            .find(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("ir-") && name.ends_with(".manifest")
            })
            .expect("formatted manifest should remain discoverable")
            .path();
        let formatted_manifest =
            fs::read_to_string(formatted_ir).expect("formatted manifest should be readable");
        assert_eq!(
            formatted_manifest
                .lines()
                .find(|line| line.starts_with("function\tmain\t")),
            Some(first_function_shape.as_str()),
            "formatting-only edits should preserve normalized function identity"
        );

        fs::write(
            &entry,
            "fn helper() -> i64 {\n    return 3\n}\nfn main() -> i64 {\n    let value: i64 = 8\n    return value\n}\n",
        )
        .expect("semantic source edit should be writable");
        let changed = super::analyze(&entry).expect("changed project should analyze");
        let changed_c = changed
            .emit_c_cached_for_target(&root, crate::codegen::NativeTarget::Linux)
            .expect("changed generated C should be emitted");
        assert_ne!(
            changed_c, first_c,
            "semantic edits must invalidate generated C"
        );
        let changed_function_shapes = fs::read_dir(&ir_dir)
            .expect("typed IR cache directory should remain readable after semantic edit")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".manifest"))
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|manifest| {
                manifest
                    .lines()
                    .find(|line| line.starts_with("function\tmain\t"))
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        assert!(
            changed_function_shapes
                .iter()
                .any(|shape| shape != &first_function_shape),
            "semantic edits with unchanged counts must change normalized function identity"
        );
        let function_ir_artifacts = fs::read_dir(&ir_dir)
            .expect("function IR artifacts should remain readable")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("function-"))
            .count();
        assert_eq!(
            function_ir_artifacts, 3,
            "a changed function should publish a new artifact while retaining the old one"
        );

        for value in 9..=16 {
            fs::write(
                &entry,
                format!(
                    "fn main() -> i64 {{\n    let value: i64 = {value}\n    return value\n}}\n"
                ),
            )
            .expect("repeated semantic source edit should be writable");
            let analysis = super::analyze(&entry).expect("repeated project should analyze");
            analysis
                .emit_c_cached_for_target(&root, crate::codegen::NativeTarget::Linux)
                .expect("repeated generated C should be emitted");
        }
        let retained_ir = fs::read_dir(&ir_dir)
            .expect("typed IR cache directory should remain readable")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("ir-") && name.ends_with(".manifest"))
            })
            .count();
        assert!(
            retained_ir <= 8,
            "typed IR cache should retain only the bounded recent manifest set"
        );
        let retained_function_ir = fs::read_dir(&ir_dir)
            .expect("typed IR cache should remain readable for function retention")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("function-") && name.ends_with(".manifest")
                })
            })
            .count();
        assert!(
            retained_function_ir <= 8,
            "typed IR function cache should retain only the bounded recent artifact set"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn typed_ir_function_artifacts_include_source_module_identity() {
        let root = test_path("typed-ir-module-identity");
        fs::create_dir_all(&root).expect("module identity fixture should be writable");
        let cache_dir = root.join(".flux/ir-cache");
        fs::create_dir_all(&cache_dir).expect("typed IR cache should be writable");
        super::persist_typed_ir_function_artifact(
            &cache_dir,
            crate::codegen::NativeTarget::Linux,
            "package.dep".to_string(),
            "helper".to_string(),
            0x1234,
            "function\thelper\tmodule=package.dep\tshape=0000000000001234",
        );
        super::persist_typed_ir_function_artifact(
            &cache_dir,
            crate::codegen::NativeTarget::Linux,
            "package.main".to_string(),
            "helper".to_string(),
            0x1234,
            "function\thelper\tmodule=package.main\tshape=0000000000001234",
        );
        let manifests = fs::read_dir(&cache_dir)
            .expect("typed IR cache should be readable")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("function-"))
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .collect::<Vec<_>>();
        let helper_manifests = manifests
            .iter()
            .filter(|manifest| {
                manifest
                    .lines()
                    .any(|line| line.starts_with("function\thelper\t"))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            helper_manifests.len(),
            2,
            "each module's helper needs its own artifact"
        );
        assert_ne!(helper_manifests[0], helper_manifests[1]);
        assert!(
            helper_manifests
                .iter()
                .all(|manifest| manifest.contains("\tmodule="))
        );

        let dependency = super::read_typed_ir_function_artifact(
            &cache_dir,
            crate::codegen::NativeTarget::Linux,
            "package.dep",
            "helper",
            0x1234,
        )
        .expect("the exact function artifact should be discoverable");
        assert!(dependency.contains("function\thelper\tmodule=package.dep"));
        assert!(
            super::read_typed_ir_function_artifact(
                &cache_dir,
                crate::codegen::NativeTarget::Linux,
                "package.dep",
                "helper",
                0x4321,
            )
            .is_none(),
            "a changed function shape must be a cache miss"
        );

        let dependency_path = fs::read_dir(&cache_dir)
            .expect("typed IR cache should remain readable")
            .filter_map(Result::ok)
            .find(|entry| {
                entry.file_name().to_string_lossy().contains("function-")
                    && fs::read_to_string(entry.path())
                        .is_ok_and(|manifest| manifest.contains("module=package.dep"))
            })
            .expect("dependency artifact should remain discoverable")
            .path();
        fs::write(&dependency_path, "corrupt\npayload")
            .expect("corrupt artifact fixture should be writable");
        assert!(
            super::read_typed_ir_function_artifact(
                &cache_dir,
                crate::codegen::NativeTarget::Linux,
                "package.dep",
                "helper",
                0x1234,
            )
            .is_none(),
            "checksum failures must be treated as cache misses"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn typed_ir_manifest_rebuild_repairs_missing_function_artifact() {
        let root = test_path("typed-ir-repair");
        fs::create_dir_all(&root).expect("typed IR repair fixture should be writable");
        let entry = root.join("main.flux");
        fs::write(
            &entry,
            "fn helper() -> i64 {\n    return 3\n}\nfn main() -> i64 {\n    return helper()\n}\n",
        )
        .expect("typed IR repair source should be writable");

        let analysis = super::analyze(&entry).expect("repair fixture should analyze");
        analysis
            .emit_c_cached_for_target(&root, crate::codegen::NativeTarget::Linux)
            .expect("initial codegen should succeed");
        let ir_dir = root.join(".flux/ir-cache");
        let missing = fs::read_dir(&ir_dir)
            .expect("typed IR cache should exist")
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().starts_with("function-"))
            .expect("function artifact should be published")
            .path();
        fs::remove_file(&missing).expect("function artifact should be removable");
        let codegen = fs::read_dir(root.join(".flux/cache"))
            .expect("codegen cache should exist")
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().starts_with("codegen-"))
            .expect("codegen artifact should be published")
            .path();
        fs::remove_file(codegen).expect("codegen artifact should be removable");

        analysis
            .emit_c_cached_for_target(&root, crate::codegen::NativeTarget::Linux)
            .expect("codegen miss should repair typed IR artifacts");
        assert!(
            missing.exists(),
            "missing function artifact should be restored"
        );
        let _ = fs::remove_dir_all(root);
    }
}
