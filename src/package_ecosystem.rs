use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub const PACKAGE_ARCHIVE_FORMAT_VERSION: u32 = 1;
pub const REGISTRY_INDEX_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRelease {
    pub package: String,
    pub owner: String,
    pub repository: String,
    pub version: String,
    pub flux: String,
    pub asset: String,
    pub sha256: String,
    pub dependencies: BTreeMap<String, String>,
    pub yanked: bool,
}

pub trait RegistryProvider {
    fn versions(&self, package: &str) -> io::Result<Vec<String>>;
    fn release(&self, package: &str, version: &str) -> io::Result<RegistryRelease>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryRegistryProvider {
    root: PathBuf,
}

impl DirectoryRegistryProvider {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticRegistryProvider {
    base_url: String,
    offline: bool,
}

impl StaticRegistryProvider {
    pub fn new(base_url: impl Into<String>) -> io::Result<Self> {
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        if !base_url.starts_with("https://") && !base_url.starts_with("file://") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "static registry URL must use https:// (or file:// for local testing)",
            ));
        }
        Ok(Self {
            base_url,
            offline: false,
        })
    }

    pub fn with_offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    fn read_text(&self, relative: &str) -> io::Result<String> {
        if let Some(root) = self.base_url.strip_prefix("file://") {
            return fs::read_to_string(Path::new(root).join(relative));
        }
        let cache_key = sha256_bytes(self.base_url.as_bytes());
        let cached = package_cache_root()?
            .join("registry")
            .join(cache_key)
            .join(relative);
        match fs::read_to_string(&cached) {
            Ok(source) => return Ok(source),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if self.offline {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("registry metadata '{relative}' is not cached while offline"),
            ));
        }
        let url = format!("{}/{}", self.base_url, relative);
        let output = Command::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error"])
            .arg(&url)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "failed to read static registry resource '{url}'"
            )));
        }
        let source = String::from_utf8(output.stdout).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("static registry resource '{url}' is not UTF-8"),
            )
        })?;
        if let Some(parent) = cached.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = cached.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temporary, source.as_bytes())?;
        match fs::rename(&temporary, &cached) {
            Ok(()) => {}
            Err(_error) if cached.is_file() => {
                let _ = fs::remove_file(&temporary);
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
        }
        Ok(source)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfiguredRegistryProvider {
    Directory(DirectoryRegistryProvider),
    Static(StaticRegistryProvider),
}

impl RegistryProvider for ConfiguredRegistryProvider {
    fn versions(&self, package: &str) -> io::Result<Vec<String>> {
        match self {
            Self::Directory(provider) => provider.versions(package),
            Self::Static(provider) => provider.versions(package),
        }
    }

    fn release(&self, package: &str, version: &str) -> io::Result<RegistryRelease> {
        match self {
            Self::Directory(provider) => provider.release(package, version),
            Self::Static(provider) => provider.release(package, version),
        }
    }
}

pub fn configured_registry_provider(offline: bool) -> io::Result<ConfiguredRegistryProvider> {
    if let Some(root) = env::var_os("FLUX_REGISTRY_DIR") {
        return Ok(ConfiguredRegistryProvider::Directory(
            DirectoryRegistryProvider::new(PathBuf::from(root)),
        ));
    }
    if let Ok(url) = env::var("FLUX_REGISTRY_URL") {
        return StaticRegistryProvider::new(url)
            .map(|provider| provider.with_offline(offline))
            .map(ConfiguredRegistryProvider::Static);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "registry dependencies require FLUX_REGISTRY_DIR or FLUX_REGISTRY_URL",
    ))
}

impl RegistryProvider for DirectoryRegistryProvider {
    fn versions(&self, package: &str) -> io::Result<Vec<String>> {
        validate_package_name(package)?;
        let directory = self.root.join(package);
        let mut versions = Vec::new();
        match fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    if !entry.file_type()?.is_file() {
                        continue;
                    }
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                        continue;
                    }
                    let Some(version) = path.file_stem().and_then(|value| value.to_str()) else {
                        continue;
                    };
                    let release = read_registry_release(&path)?;
                    if release.package != package || release.version != version {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "registry metadata '{}' does not match its package/version path",
                                path.display()
                            ),
                        ));
                    }
                    versions.push(version.to_string());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
        versions.sort();
        versions.dedup();
        Ok(versions)
    }

    fn release(&self, package: &str, version: &str) -> io::Result<RegistryRelease> {
        validate_package_name(package)?;
        validate_version(version)?;
        let path = self.root.join(package).join(format!("{version}.toml"));
        let release = read_registry_release(&path)?;
        if release.package != package || release.version != version {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry metadata '{}' does not match requested {package} {version}",
                    path.display()
                ),
            ));
        }
        Ok(release)
    }
}

impl RegistryProvider for StaticRegistryProvider {
    fn versions(&self, package: &str) -> io::Result<Vec<String>> {
        validate_package_name(package)?;
        let source = self.read_text(&format!("{package}/versions.txt"))?;
        let mut versions = Vec::new();
        for raw in source.lines() {
            let version = raw.trim();
            if version.is_empty() || version.starts_with('#') {
                continue;
            }
            validate_version(version)?;
            versions.push(version.to_string());
        }
        versions.sort();
        versions.dedup();
        Ok(versions)
    }

    fn release(&self, package: &str, version: &str) -> io::Result<RegistryRelease> {
        validate_package_name(package)?;
        validate_version(version)?;
        let relative = format!("{package}/{version}.toml");
        let source = self.read_text(&relative)?;
        let release = parse_registry_release(&source).map_err(|message| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid static registry metadata '{relative}': {message}"),
            )
        })?;
        if release.package != package || release.version != version {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry metadata '{relative}' does not match requested {package} {version}"
                ),
            ));
        }
        Ok(release)
    }
}

pub fn resolve_registry_release(
    provider: &dyn RegistryProvider,
    package: &str,
    requirements: &[String],
) -> io::Result<Option<RegistryRelease>> {
    validate_package_name(package)?;
    let versions = provider.versions(package)?;
    let selected = crate::project::resolve_semver_requirements(
        requirements.iter().map(String::as_str),
        versions.iter().map(String::as_str),
    )
    .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
    let Some(version) = selected else {
        return Ok(None);
    };
    let release = provider.release(package, &version)?;
    if release.yanked {
        let available = versions
            .iter()
            .filter_map(|candidate| provider.release(package, candidate).ok())
            .filter(|candidate| !candidate.yanked)
            .map(|candidate| candidate.version)
            .collect::<Vec<_>>();
        let selected = crate::project::resolve_semver_requirements(
            requirements.iter().map(String::as_str),
            available.iter().map(String::as_str),
        )
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
        return selected
            .map(|version| provider.release(package, &version))
            .transpose();
    }
    Ok(Some(release))
}

pub fn create_fluxpkg(package_root: &Path, output: &Path) -> io::Result<String> {
    let root = fs::canonicalize(package_root)?;
    let manifest = root.join("flux.toml");
    if !manifest.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("package root '{}' has no flux.toml", root.display()),
        ));
    }
    reject_symlinks(&root, &root)?;

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let output = output.to_path_buf();
    let temporary = output.with_extension(format!("fluxpkg.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);

    let status = Command::new("tar")
        .args([
            "--sort=name",
            "--mtime=@0",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "--format=posix",
            "--pax-option=delete=atime,delete=ctime",
            "--exclude=./.git",
            "--exclude=./target",
            "--exclude=./dist",
            "-czf",
        ])
        .arg(&temporary)
        .arg("-C")
        .arg(&root)
        .arg(".")
        .status()?;
    if !status.success() {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::other(format!(
            "tar failed while creating canonical .fluxpkg archive for '{}'",
            root.display()
        )));
    }

    let digest = sha256_file(&temporary)?;
    if output.exists() {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("package archive '{}' already exists", output.display()),
        ));
    }
    fs::rename(&temporary, &output)?;
    Ok(digest)
}

pub fn package_cache_root() -> io::Result<PathBuf> {
    if let Some(path) = env::var_os("FLUX_PACKAGE_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("FLUX_CACHE_DIR") {
        return Ok(PathBuf::from(path).join("packages"));
    }
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("flux/packages"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".cache/flux/packages"));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "cannot determine Flux package cache directory",
    ))
}

pub fn cache_fluxpkg(archive: &Path, expected_sha256: &str) -> io::Result<PathBuf> {
    validate_sha256(expected_sha256)?;
    let actual = sha256_file(archive)?;
    if actual != expected_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("package checksum mismatch: expected {expected_sha256}, got {actual}"),
        ));
    }
    let root = package_cache_root()?.join("sha256");
    fs::create_dir_all(&root)?;
    let destination = root.join(format!("{expected_sha256}.fluxpkg"));
    if destination.exists() {
        verify_cached_fluxpkg(expected_sha256)?;
        return Ok(destination);
    }
    let temporary = root.join(format!(".{expected_sha256}.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    fs::copy(archive, &temporary)?;
    if sha256_file(&temporary)? != expected_sha256 {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package cache copy failed checksum verification",
        ));
    }
    match fs::rename(&temporary, &destination) {
        Ok(()) => Ok(destination),
        Err(_error) if destination.exists() => {
            let _ = fs::remove_file(&temporary);
            verify_cached_fluxpkg(expected_sha256)?;
            Ok(destination)
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

pub fn verify_cached_fluxpkg(expected_sha256: &str) -> io::Result<PathBuf> {
    validate_sha256(expected_sha256)?;
    let path = package_cache_root()?
        .join("sha256")
        .join(format!("{expected_sha256}.fluxpkg"));
    let actual = sha256_file(&path)?;
    if actual != expected_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "cached package '{}' failed checksum verification: expected {expected_sha256}, got {actual}",
                path.display()
            ),
        ));
    }
    Ok(path)
}

pub fn fetch_registry_release(release: &RegistryRelease, offline: bool) -> io::Result<PathBuf> {
    match verify_cached_fluxpkg(&release.sha256) {
        Ok(path) => return Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if offline {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "package {} {} is not available in the verified package cache while offline",
                release.package, release.version
            ),
        ));
    }
    let downloads = package_cache_root()?.join("downloads");
    fs::create_dir_all(&downloads)?;
    let temporary = downloads.join(format!(
        ".{}-{}-{}.fluxpkg",
        release.package,
        release.version,
        std::process::id()
    ));
    let _ = fs::remove_file(&temporary);
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&temporary)
        .arg(&release.asset)
        .status()?;
    if !status.success() {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::other(format!(
            "failed to download immutable package asset '{}' for {} {}",
            release.asset, release.package, release.version
        )));
    }
    let cached = cache_fluxpkg(&temporary, &release.sha256);
    let _ = fs::remove_file(&temporary);
    cached
}

pub fn materialize_registry_release(
    release: &RegistryRelease,
    destination: &Path,
    offline: bool,
) -> io::Result<PathBuf> {
    let archive = fetch_registry_release(release, offline)?;
    extract_fluxpkg(&archive, &release.sha256, destination)
}

pub fn materialize_cached_registry_release(
    release: &RegistryRelease,
    offline: bool,
) -> io::Result<PathBuf> {
    let root = package_cache_root()?.join("sources").join(&release.sha256);
    if root.join("flux.toml").is_file() {
        let manifest =
            crate::project::read_manifest(&root.join("flux.toml")).map_err(|diagnostics| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    diagnostics
                        .into_iter()
                        .map(|diagnostic| diagnostic.message)
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })?;
        if manifest.name != release.package || manifest.version.as_deref() != Some(&release.version)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "cached package source for {} {} has manifest identity {} {}",
                    release.package,
                    release.version,
                    manifest.name,
                    manifest.version.as_deref().unwrap_or("<none>")
                ),
            ));
        }
        return Ok(root);
    }

    let archive = fetch_registry_release(release, offline)?;
    match extract_fluxpkg(&archive, &release.sha256, &root) {
        Ok(path) => path,
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists && root.join("flux.toml").is_file() =>
        {
            root.clone()
        }
        Err(error) => return Err(error),
    };
    let manifest =
        crate::project::read_manifest(&root.join("flux.toml")).map_err(|diagnostics| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })?;
    if manifest.name != release.package || manifest.version.as_deref() != Some(&release.version) {
        let _ = fs::remove_dir_all(&root);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "registry archive for {} {} has manifest identity {} {}",
                release.package,
                release.version,
                manifest.name,
                manifest.version.as_deref().unwrap_or("<none>")
            ),
        ));
    }
    Ok(root)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRegistryGraph {
    pub releases: BTreeMap<String, RegistryRelease>,
}

pub fn package_has_registry_dependencies(target: &Path) -> io::Result<bool> {
    let manifest = package_manifest_target(target)?;
    let mut requirements = BTreeMap::<String, Vec<String>>::new();
    let mut visited_paths = BTreeSet::new();
    collect_manifest_registry_requirements(&manifest, &mut visited_paths, &mut requirements)?;
    Ok(!requirements.is_empty())
}

pub fn resolve_package_registry_graph(
    target: &Path,
    provider: &dyn RegistryProvider,
) -> io::Result<ResolvedRegistryGraph> {
    let manifest = package_manifest_target(target)?;
    let mut base_requirements = BTreeMap::<String, Vec<String>>::new();
    let mut visited_paths = BTreeSet::new();
    collect_manifest_registry_requirements(&manifest, &mut visited_paths, &mut base_requirements)?;
    normalize_requirements(&mut base_requirements);

    let mut requirements = base_requirements.clone();
    for _ in 0..256 {
        let mut releases = BTreeMap::new();
        for (package, package_requirements) in &requirements {
            let release = resolve_registry_release(provider, package, package_requirements)?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "no registry release of '{package}' satisfies {}",
                            package_requirements.join(", ")
                        ),
                    )
                })?;
            let compatible = crate::project::resolve_semver_requirement(
                &release.flux,
                [env!("CARGO_PKG_VERSION")],
            )
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?
            .is_some();
            if !compatible {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "registry package {} {} requires Flux {}, but this compiler is {}",
                        release.package,
                        release.version,
                        release.flux,
                        env!("CARGO_PKG_VERSION")
                    ),
                ));
            }
            releases.insert(package.clone(), release);
        }

        let mut next_requirements = base_requirements.clone();
        for release in releases.values() {
            for (dependency, requirement) in &release.dependencies {
                next_requirements
                    .entry(dependency.clone())
                    .or_default()
                    .push(requirement.clone());
            }
        }
        normalize_requirements(&mut next_requirements);
        if next_requirements == requirements {
            reject_registry_cycles(&releases)?;
            return Ok(ResolvedRegistryGraph { releases });
        }
        requirements = next_requirements;
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "registry dependency resolution did not converge",
    ))
}

pub fn materialize_package_registry_dependencies(
    target: &Path,
    provider: &dyn RegistryProvider,
    offline: bool,
) -> io::Result<(ResolvedRegistryGraph, BTreeMap<String, PathBuf>)> {
    let graph = resolve_package_registry_graph(target, provider)?;
    let mut roots = BTreeMap::new();
    for release in graph.releases.values() {
        roots.insert(
            release.package.clone(),
            materialize_cached_registry_release(release, offline)?,
        );
    }
    Ok((graph, roots))
}

pub fn fetch_package_dependencies(
    target: &Path,
    provider: &dyn RegistryProvider,
    offline: bool,
) -> io::Result<ResolvedRegistryGraph> {
    let graph = resolve_package_registry_graph(target, provider)?;
    for release in graph.releases.values() {
        fetch_registry_release(release, offline)?;
    }
    Ok(graph)
}

pub fn vendor_package_dependencies(
    target: &Path,
    provider: &dyn RegistryProvider,
    destination: &Path,
    offline: bool,
) -> io::Result<ResolvedRegistryGraph> {
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "vendor destination '{}' already exists",
                destination.display()
            ),
        ));
    }
    let graph = fetch_package_dependencies(target, provider, offline)?;
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("vendor");
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temporary);
    fs::create_dir(&temporary)?;

    let result = (|| -> io::Result<()> {
        let packages = temporary.join("packages");
        fs::create_dir(&packages)?;
        let mut lock =
            String::from("# Generated by Flux vendor. Do not edit.\nformat_version = 1\n");
        for release in graph.releases.values() {
            let package_root = packages.join(&release.package).join(&release.version);
            materialize_registry_release(release, &package_root, true)?;
            lock.push_str("\n[[package]]\n");
            lock.push_str(&format!("name = {:?}\n", release.package));
            lock.push_str(&format!("version = {:?}\n", release.version));
            lock.push_str(&format!("sha256 = {:?}\n", release.sha256));
            lock.push_str(&format!("asset = {:?}\n", release.asset));
        }
        fs::write(temporary.join("flux.vendor.lock"), lock)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    Ok(graph)
}

fn package_manifest_target(target: &Path) -> io::Result<crate::project::PackageManifest> {
    let path = if target.is_dir() {
        target.join("flux.toml")
    } else if target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        target.to_path_buf()
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package dependency transfer requires a package directory or flux.toml target",
        ));
    };
    crate::project::read_manifest(&path).map_err(|diagnostics| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })
}

fn collect_manifest_registry_requirements(
    manifest: &crate::project::PackageManifest,
    visited_paths: &mut BTreeSet<PathBuf>,
    requirements: &mut BTreeMap<String, Vec<String>>,
) -> io::Result<()> {
    let root = manifest
        .path
        .parent()
        .expect("canonical manifest path has a parent")
        .to_path_buf();
    if !visited_paths.insert(root.clone()) {
        return Ok(());
    }
    for (name, dependency) in &manifest.dependencies {
        match dependency {
            crate::project::PackageDependency::Registry { requirement } => {
                requirements
                    .entry(name.clone())
                    .or_default()
                    .push(requirement.clone());
            }
            crate::project::PackageDependency::Path { path, .. } => {
                let dependency_manifest = crate::project::read_manifest(
                    &root.join(path).join("flux.toml"),
                )
                .map_err(|diagnostics| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        diagnostics
                            .into_iter()
                            .map(|diagnostic| diagnostic.message)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                })?;
                collect_manifest_registry_requirements(
                    &dependency_manifest,
                    visited_paths,
                    requirements,
                )?;
            }
            crate::project::PackageDependency::Git { .. } => {}
        }
    }
    Ok(())
}

fn normalize_requirements(requirements: &mut BTreeMap<String, Vec<String>>) {
    for values in requirements.values_mut() {
        values.sort();
        values.dedup();
    }
}

fn reject_registry_cycles(releases: &BTreeMap<String, RegistryRelease>) -> io::Result<()> {
    fn visit(
        package: &str,
        releases: &BTreeMap<String, RegistryRelease>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        stack: &mut Vec<String>,
    ) -> io::Result<()> {
        if visited.contains(package) {
            return Ok(());
        }
        if !visiting.insert(package.to_string()) {
            if let Some(start) = stack.iter().position(|item| item == package) {
                let mut cycle = stack[start..].to_vec();
                cycle.push(package.to_string());
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "cyclic registry dependency detected: {}",
                        cycle.join(" -> ")
                    ),
                ));
            }
        }
        stack.push(package.to_string());
        if let Some(release) = releases.get(package) {
            for dependency in release.dependencies.keys() {
                if releases.contains_key(dependency) {
                    visit(dependency, releases, visiting, visited, stack)?;
                }
            }
        }
        stack.pop();
        visiting.remove(package);
        visited.insert(package.to_string());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    for package in releases.keys() {
        visit(package, releases, &mut visiting, &mut visited, &mut stack)?;
    }
    Ok(())
}

pub fn extract_cached_fluxpkg(expected_sha256: &str, destination: &Path) -> io::Result<PathBuf> {
    let archive = verify_cached_fluxpkg(expected_sha256)?;
    extract_fluxpkg(&archive, expected_sha256, destination)
}

pub fn extract_fluxpkg(
    archive: &Path,
    expected_sha256: &str,
    destination: &Path,
) -> io::Result<PathBuf> {
    validate_sha256(expected_sha256)?;
    let actual = sha256_file(archive)?;
    if actual != expected_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("package checksum mismatch: expected {expected_sha256}, got {actual}"),
        ));
    }
    validate_fluxpkg_archive(archive)?;
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "package extraction destination '{}' already exists",
                destination.display()
            ),
        ));
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("package");
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temporary);
    fs::create_dir(&temporary)?;
    let status = Command::new("tar")
        .args(["--no-same-owner", "--no-same-permissions", "-xzf"])
        .arg(archive)
        .arg("-C")
        .arg(&temporary)
        .status()?;
    if !status.success() {
        let _ = fs::remove_dir_all(&temporary);
        return Err(io::Error::other(format!(
            "tar failed while extracting package archive '{}'",
            archive.display()
        )));
    }
    if !temporary.join("flux.toml").is_file() {
        let _ = fs::remove_dir_all(&temporary);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package archive root must contain flux.toml",
        ));
    }
    match fs::rename(&temporary, destination) {
        Ok(()) => Ok(destination.to_path_buf()),
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            Err(error)
        }
    }
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_digest(hasher.finish()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_digest(hasher.finish())
}

fn validate_sha256(value: &str) -> io::Result<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package SHA-256 must be exactly 64 lowercase hexadecimal characters",
        ))
    }
}

fn validate_package_name(value: &str) -> io::Result<()> {
    if !value.is_empty()
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid registry package name '{value}'"),
        ))
    }
}

fn validate_version(value: &str) -> io::Result<()> {
    match crate::project::resolve_semver_requirement(value, [value]) {
        Ok(Some(_)) => Ok(()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid registry package version '{value}'"),
        )),
    }
}

fn validate_fluxpkg_archive(archive: &Path) -> io::Result<()> {
    let listing = Command::new("tar").arg("-tzf").arg(archive).output()?;
    if !listing.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid .fluxpkg archive '{}'", archive.display()),
        ));
    }
    let listing = String::from_utf8(listing.stdout).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "package archive paths must be UTF-8",
        )
    })?;
    for entry in listing.lines() {
        let entry = entry.strip_prefix("./").unwrap_or(entry);
        if entry.is_empty() {
            continue;
        }
        let path = Path::new(entry);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("package archive contains unsafe path '{entry}'"),
            ));
        }
    }
    let verbose = Command::new("tar").arg("-tvzf").arg(archive).output()?;
    if !verbose.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid .fluxpkg archive '{}'", archive.display()),
        ));
    }
    let verbose = String::from_utf8(verbose.stdout).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "package archive metadata must be UTF-8",
        )
    })?;
    if verbose
        .lines()
        .any(|line| matches!(line.as_bytes().first(), Some(b'l' | b'h')))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package archives may not contain symbolic or hard links",
        ));
    }
    Ok(())
}

pub fn read_registry_release(path: &Path) -> io::Result<RegistryRelease> {
    let source = fs::read_to_string(path)?;
    parse_registry_release(&source).map_err(|message| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid registry metadata '{}': {message}", path.display()),
        )
    })
}

pub fn parse_registry_release(source: &str) -> Result<RegistryRelease, String> {
    let mut section = "release";
    let mut format_version = None;
    let mut package = None;
    let mut owner = None;
    let mut repository = None;
    let mut version = None;
    let mut flux = None;
    let mut asset = None;
    let mut sha256 = None;
    let mut yanked = None;
    let mut dependencies = BTreeMap::new();
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if line != "[dependencies]" {
                return Err(format!("unsupported registry metadata table '{line}'"));
            }
            section = "dependencies";
            continue;
        }
        let (key, raw_value) = line
            .split_once('=')
            .ok_or_else(|| "registry metadata fields use 'key = value' syntax".to_string())?;
        let key = key.trim();
        let raw_value = raw_value.trim();
        if section == "dependencies" {
            validate_package_name(key).map_err(|error| error.to_string())?;
            let requirement = registry_string(raw_value)?;
            crate::project::resolve_semver_requirement(&requirement, std::iter::empty::<&str>())
                .map_err(|_| format!("invalid dependency SemVer requirement '{requirement}'"))?;
            if dependencies.insert(key.to_string(), requirement).is_some() {
                return Err(format!("duplicate registry dependency '{key}'"));
            }
            continue;
        }
        match key {
            "format_version" => {
                let value = raw_value
                    .parse::<u32>()
                    .map_err(|_| "registry format_version must be an integer".to_string())?;
                set_once(&mut format_version, value, key)?;
            }
            "package" => set_once(&mut package, registry_string(raw_value)?, key)?,
            "owner" => set_once(&mut owner, registry_string(raw_value)?, key)?,
            "repository" => set_once(&mut repository, registry_string(raw_value)?, key)?,
            "version" => set_once(&mut version, registry_string(raw_value)?, key)?,
            "flux" => set_once(&mut flux, registry_string(raw_value)?, key)?,
            "asset" => set_once(&mut asset, registry_string(raw_value)?, key)?,
            "sha256" => set_once(&mut sha256, registry_string(raw_value)?, key)?,
            "yanked" => {
                let value = match raw_value {
                    "true" => true,
                    "false" => false,
                    _ => return Err("registry yanked must be true or false".to_string()),
                };
                set_once(&mut yanked, value, key)?;
            }
            _ => return Err(format!("unknown registry metadata field '{key}'")),
        }
    }
    if format_version != Some(REGISTRY_INDEX_FORMAT_VERSION) {
        return Err(format!(
            "unsupported registry metadata format {}; expected {}",
            format_version.map_or_else(|| "<missing>".to_string(), |value| value.to_string()),
            REGISTRY_INDEX_FORMAT_VERSION
        ));
    }
    let release = RegistryRelease {
        package: package.ok_or("registry metadata requires package")?,
        owner: owner.ok_or("registry metadata requires owner")?,
        repository: repository.ok_or("registry metadata requires repository")?,
        version: version.ok_or("registry metadata requires version")?,
        flux: flux.ok_or("registry metadata requires flux")?,
        asset: asset.ok_or("registry metadata requires asset")?,
        sha256: sha256.ok_or("registry metadata requires sha256")?,
        dependencies,
        yanked: yanked.unwrap_or(false),
    };
    validate_package_name(&release.package).map_err(|error| error.to_string())?;
    validate_version(&release.version).map_err(|error| error.to_string())?;
    validate_sha256(&release.sha256).map_err(|error| error.to_string())?;
    if !release.repository.starts_with("https://") || !release.asset.starts_with("https://") {
        return Err("registry repository and asset URLs must use https://".to_string());
    }
    if release.owner.trim().is_empty() {
        return Err("registry owner must not be empty".to_string());
    }
    crate::project::resolve_semver_requirement(&release.flux, std::iter::empty::<&str>())
        .map_err(|_| format!("invalid Flux compatibility requirement '{}'", release.flux))?;
    Ok(release)
}

fn set_once<T>(slot: &mut Option<T>, value: T, key: &str) -> Result<(), String> {
    if slot.replace(value).is_some() {
        Err(format!("duplicate registry metadata field '{key}'"))
    } else {
        Ok(())
    }
}

fn registry_string(value: &str) -> Result<String, String> {
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return Err("registry string values must be quoted".to_string());
    }
    let inner = &value[1..value.len() - 1];
    if inner.contains('"') || inner.contains('\\') || inner.contains('\n') || inner.contains('\r') {
        return Err(
            "registry string values may not contain quotes, backslashes, or newlines".to_string(),
        );
    }
    Ok(inner.to_string())
}

fn reject_symlinks(root: &Path, directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if matches!(
            relative
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str()),
            Some(".git" | "target" | "dist")
        ) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    ".fluxpkg archives do not allow symlinks: '{}'",
                    relative.display()
                ),
            ));
        }
        if metadata.is_dir() {
            reject_symlinks(root, &path)?;
        }
    }
    Ok(())
}

fn hex_digest(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    bytes_seen: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            bytes_seen: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.bytes_seen = self.bytes_seen.wrapping_add(input.len() as u64);
        if self.buffer_len != 0 {
            let needed = 64 - self.buffer_len;
            let take = needed.min(input.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&input[..take]);
            self.buffer_len += take;
            input = &input[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }
        while input.len() >= 64 {
            let mut block = [0_u8; 64];
            block.copy_from_slice(&input[..64]);
            self.compress(&block);
            input = &input[64..];
        }
        self.buffer[..input.len()].copy_from_slice(input);
        self.buffer_len = input.len();
    }

    fn finish(mut self) -> [u8; 32] {
        let bit_len = self.bytes_seen.wrapping_mul(8);
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffer = [0; 64];
            self.buffer_len = 0;
        }
        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        self.compress(&block);
        let mut output = [0_u8; 32];
        for (chunk, word) in output.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        output
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0_u32; 64];
        for (index, chunk) in block.chunks_exact(4).enumerate().take(16) {
            w[index] = u32::from_be_bytes(chunk.try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static CACHE_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "flux-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn sha256_matches_standard_vectors() {
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn fluxpkg_is_reproducible_and_excludes_build_outputs() {
        let root = temp_root("fluxpkg-repro");
        let package = root.join("package");
        fs::create_dir_all(package.join("src")).unwrap();
        fs::create_dir_all(package.join("target")).unwrap();
        fs::create_dir_all(package.join("dist")).unwrap();
        fs::write(
            package.join("flux.toml"),
            "[package]\nname = \"demo\"\nentry = \"src/main.flux\"\n",
        )
        .unwrap();
        fs::write(package.join("src/main.flux"), "fn main() -> i64 { 0 }\n").unwrap();
        fs::write(package.join("target/ignored"), "host-specific").unwrap();
        fs::write(package.join("dist/ignored"), "generated").unwrap();
        let first = root.join("first.fluxpkg");
        let second = root.join("second.fluxpkg");
        let first_hash = create_fluxpkg(&package, &first).unwrap();
        fs::write(package.join("target/ignored"), "changed host output").unwrap();
        let second_hash = create_fluxpkg(&package, &second).unwrap();
        assert_eq!(first_hash, second_hash);
        assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_cache_is_content_addressed_and_rejects_tampering() {
        let _guard = CACHE_ENV_LOCK.lock().unwrap();
        let root = temp_root("package-cache");
        let archive = root.join("source.fluxpkg");
        let cache = root.join("cache");
        fs::create_dir_all(&root).unwrap();
        fs::write(&archive, b"canonical package bytes").unwrap();
        unsafe { std::env::set_var("FLUX_PACKAGE_CACHE_DIR", &cache) };
        let hash = sha256_file(&archive).unwrap();
        let stored = cache_fluxpkg(&archive, &hash).unwrap();
        assert_eq!(stored, cache.join("sha256").join(format!("{hash}.fluxpkg")));
        assert_eq!(cache_fluxpkg(&archive, &hash).unwrap(), stored);
        fs::write(&stored, b"tampered").unwrap();
        let error = verify_cached_fluxpkg(&hash).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        unsafe { std::env::remove_var("FLUX_PACKAGE_CACHE_DIR") };
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_metadata_resolves_highest_non_yanked_compatible_release() {
        let root = temp_root("registry-index");
        let package = root.join("demo");
        fs::create_dir_all(&package).unwrap();
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        for (version, yanked) in [("1.2.0", false), ("1.3.0", true), ("2.0.0", false)] {
            fs::write(
                package.join(format!("{version}.toml")),
                format!(
                    "format_version = 1\npackage = \"demo\"\nowner = \"flux-lang\"\nrepository = \"https://github.com/flux-lang/demo\"\nversion = \"{version}\"\nflux = \"^0.1.0\"\nasset = \"https://github.com/flux-lang/demo/releases/download/v{version}/demo.fluxpkg\"\nsha256 = \"{hash}\"\nyanked = {yanked}\n\n[dependencies]\nutil = \"^1.0.0\"\n"
                ),
            )
            .unwrap();
        }
        let provider = DirectoryRegistryProvider::new(&root);
        let release = resolve_registry_release(&provider, "demo", &["^1.0.0".to_string()])
            .unwrap()
            .unwrap();
        assert_eq!(release.version, "1.2.0");
        assert_eq!(
            release.dependencies.get("util"),
            Some(&"^1.0.0".to_string())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn static_registry_provider_reads_version_index_and_release_metadata() {
        let root = temp_root("static-registry");
        let package = root.join("demo");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("versions.txt"), "1.2.0\n1.0.0\n1.2.0\n").unwrap();
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        fs::write(
            package.join("1.2.0.toml"),
            format!(
                "format_version = 1\npackage = \"demo\"\nowner = \"flux-lang\"\nrepository = \"https://github.com/flux-lang/demo\"\nversion = \"1.2.0\"\nflux = \"*\"\nasset = \"https://github.com/flux-lang/demo/releases/download/v1.2.0/demo.fluxpkg\"\nsha256 = \"{hash}\"\nyanked = false\n"
            ),
        )
        .unwrap();
        let provider = StaticRegistryProvider::new(format!("file://{}", root.display())).unwrap();
        assert_eq!(provider.versions("demo").unwrap(), vec!["1.0.0", "1.2.0"]);
        assert_eq!(provider.release("demo", "1.2.0").unwrap().version, "1.2.0");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_graph_fetch_and_vendor_are_transitive_and_offline() {
        let _guard = CACHE_ENV_LOCK.lock().unwrap();
        let root = temp_root("registry-graph-vendor");
        let app = root.join("app");
        let registry = root.join("registry");
        let cache = root.join("cache");
        fs::create_dir_all(app.join("src")).unwrap();
        fs::write(
            app.join("flux.toml"),
            "[package]\nname = \"app\"\nversion = \"1.0.0\"\nentry = \"src/main.flux\"\n\n[dependencies]\nalpha = \"^1.0.0\"\n",
        )
        .unwrap();
        fs::write(app.join("src/main.flux"), "fn main() -> i64 { 0 }\n").unwrap();

        unsafe { std::env::set_var("FLUX_PACKAGE_CACHE_DIR", &cache) };
        for (name, version) in [("alpha", "1.1.0"), ("beta", "2.0.0")] {
            let package_root = root.join(format!("src-{name}"));
            fs::create_dir_all(package_root.join("src")).unwrap();
            fs::write(
                package_root.join("flux.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"{version}\"\nentry = \"src/main.flux\"\n"
                ),
            )
            .unwrap();
            fs::write(
                package_root.join("src/main.flux"),
                "fn value() -> i64 { 1 }\n",
            )
            .unwrap();
            let archive = root.join(format!("{name}-{version}.fluxpkg"));
            let hash = create_fluxpkg(&package_root, &archive).unwrap();
            cache_fluxpkg(&archive, &hash).unwrap();
            let package_index = registry.join(name);
            fs::create_dir_all(&package_index).unwrap();
            let dependencies = if name == "alpha" {
                "\n[dependencies]\nbeta = \"~2.0.0\"\n"
            } else {
                ""
            };
            fs::write(
                package_index.join(format!("{version}.toml")),
                format!(
                    "format_version = 1\npackage = \"{name}\"\nowner = \"flux-lang\"\nrepository = \"https://github.com/flux-lang/{name}\"\nversion = \"{version}\"\nflux = \"*\"\nasset = \"https://invalid.example/{name}-{version}.fluxpkg\"\nsha256 = \"{hash}\"\nyanked = false\n{dependencies}"
                ),
            )
            .unwrap();
        }

        let provider = DirectoryRegistryProvider::new(&registry);
        let graph = fetch_package_dependencies(&app, &provider, true).unwrap();
        assert_eq!(graph.releases.len(), 2);
        assert_eq!(graph.releases["alpha"].version, "1.1.0");
        assert_eq!(graph.releases["beta"].version, "2.0.0");

        let vendor = root.join("vendor");
        vendor_package_dependencies(&app, &provider, &vendor, true).unwrap();
        assert!(vendor.join("packages/alpha/1.1.0/flux.toml").is_file());
        assert!(vendor.join("packages/beta/2.0.0/flux.toml").is_file());
        let lock = fs::read_to_string(vendor.join("flux.vendor.lock")).unwrap();
        assert!(lock.contains("name = \"alpha\""));
        assert!(lock.contains("name = \"beta\""));

        unsafe { std::env::remove_var("FLUX_PACKAGE_CACHE_DIR") };
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_metadata_rejects_install_hooks_and_unknown_fields() {
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let error = parse_registry_release(&format!(
            "format_version = 1\npackage = \"demo\"\nowner = \"flux-lang\"\nrepository = \"https://github.com/flux-lang/demo\"\nversion = \"1.0.0\"\nflux = \"^0.1.0\"\nasset = \"https://github.com/flux-lang/demo/releases/download/v1/demo.fluxpkg\"\nsha256 = \"{hash}\"\ninstall = \"sh setup.sh\"\n"
        ))
        .unwrap_err();
        assert!(error.contains("unknown registry metadata field 'install'"));
    }

    #[test]
    fn verified_fluxpkg_extraction_requires_safe_archive_paths_and_manifest() {
        let root = temp_root("fluxpkg-extract");
        let package = root.join("package");
        fs::create_dir_all(package.join("src")).unwrap();
        fs::write(
            package.join("flux.toml"),
            "[package]\nname = \"demo\"\nentry = \"src/main.flux\"\n",
        )
        .unwrap();
        fs::write(package.join("src/main.flux"), "fn main() -> i64 { 0 }\n").unwrap();
        let archive = root.join("demo.fluxpkg");
        let hash = create_fluxpkg(&package, &archive).unwrap();
        let extracted = root.join("extracted");
        extract_fluxpkg(&archive, &hash, &extracted).unwrap();
        assert_eq!(
            fs::read_to_string(extracted.join("src/main.flux")).unwrap(),
            "fn main() -> i64 { 0 }\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn offline_registry_materialization_uses_only_verified_cached_content() {
        let _guard = CACHE_ENV_LOCK.lock().unwrap();
        let root = temp_root("registry-offline");
        let package = root.join("package");
        let cache = root.join("cache");
        fs::create_dir_all(package.join("src")).unwrap();
        fs::write(
            package.join("flux.toml"),
            "[package]\nname = \"demo\"\nentry = \"src/main.flux\"\n",
        )
        .unwrap();
        fs::write(package.join("src/main.flux"), "fn main() -> i64 { 0 }\n").unwrap();
        let archive = root.join("demo.fluxpkg");
        let hash = create_fluxpkg(&package, &archive).unwrap();
        unsafe { std::env::set_var("FLUX_PACKAGE_CACHE_DIR", &cache) };
        cache_fluxpkg(&archive, &hash).unwrap();
        let release = RegistryRelease {
            package: "demo".to_string(),
            owner: "flux-lang".to_string(),
            repository: "https://github.com/flux-lang/demo".to_string(),
            version: "1.0.0".to_string(),
            flux: "^0.1.0".to_string(),
            asset: "https://invalid.example/should-not-be-read.fluxpkg".to_string(),
            sha256: hash.clone(),
            dependencies: BTreeMap::new(),
            yanked: false,
        };
        let destination = root.join("vendor/demo");
        materialize_registry_release(&release, &destination, true).unwrap();
        assert!(destination.join("flux.toml").is_file());
        fs::remove_file(cache.join("sha256").join(format!("{hash}.fluxpkg"))).unwrap();
        let error = fetch_registry_release(&release, true).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        unsafe { std::env::remove_var("FLUX_PACKAGE_CACHE_DIR") };
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn fluxpkg_rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let root = temp_root("fluxpkg-symlink");
        let package = root.join("package");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("flux.toml"),
            "[package]\nname = \"demo\"\nentry = \"main.flux\"\n",
        )
        .unwrap();
        fs::write(package.join("main.flux"), "fn main() -> i64 { 0 }\n").unwrap();
        symlink("main.flux", package.join("alias.flux")).unwrap();
        let error = create_fluxpkg(&package, &root.join("demo.fluxpkg")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(root);
    }
}
