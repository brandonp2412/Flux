use crate as fluxc;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fluxc::{Diagnostic, DiagnosticSource, TerminalRenderOptions};

const FLUX_GDB_SUPPORT: &str = include_str!("../tools/flux-gdb.py");
const COVERAGE_COMPILATION_DIR: &str = "/__flux_coverage__";

enum CliError {
    Message(String),
    Reported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildMode {
    Debug,
    Profile,
    Release,
}

impl BuildMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "debug" => Ok(Self::Debug),
            "profile" => Ok(Self::Profile),
            "release" => Ok(Self::Release),
            _ => Err(format!(
                "unknown build mode '{value}'; expected debug, profile, or release"
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Profile => "profile",
            Self::Release => "release",
        }
    }

    fn clang_args(self) -> &'static [&'static str] {
        match self {
            Self::Debug => &["-O0", "-g3", "-fno-omit-frame-pointer"],
            Self::Profile => &["-O2", "-g", "-fno-omit-frame-pointer", "-DNDEBUG"],
            Self::Release => &["-O3", "-flto", "-DNDEBUG"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeInstrumentation {
    None,
    Gprof,
    Coverage,
}

impl NativeInstrumentation {
    const fn cache_tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gprof => "gprof",
            Self::Coverage => "coverage-v2",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct NativeTargetOptions {
    triple: Option<String>,
    sysroot: Option<PathBuf>,
}

#[derive(Debug)]
struct BuildOptions {
    output: Option<PathBuf>,
    mode: BuildMode,
    native_target: NativeTargetOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageFormat {
    Directory,
    TarGz,
    Container,
    Systemd,
}

impl PackageFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "directory" | "dir" => Ok(Self::Directory),
            "tar.gz" | "tgz" => Ok(Self::TarGz),
            "container" => Ok(Self::Container),
            "systemd" => Ok(Self::Systemd),
            _ => Err(format!(
                "unknown package format '{value}'; expected directory, tar.gz, container, or systemd"
            )),
        }
    }
}

#[derive(Debug)]
struct PackageOptions {
    output: Option<PathBuf>,
    mode: BuildMode,
    format: PackageFormat,
    native_target: NativeTargetOptions,
}

#[derive(Debug, PartialEq, Eq)]
struct DebugOptions {
    target: PathBuf,
    breakpoints: Vec<String>,
    run_immediately: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct SymbolizeOptions {
    binary: PathBuf,
    addresses: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct SplitSymbolsOptions {
    binary: PathBuf,
    output_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TestOptions {
    mode: BuildMode,
    coverage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AndroidAbi {
    Arm64V8a,
    X86_64,
    ArmeabiV7a,
}

impl AndroidAbi {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "arm64-v8a" => Ok(Self::Arm64V8a),
            "x86_64" => Ok(Self::X86_64),
            "armeabi-v7a" => Ok(Self::ArmeabiV7a),
            _ => Err(format!(
                "unknown Android ABI '{value}'; expected arm64-v8a, x86_64, or armeabi-v7a"
            )),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Arm64V8a => "arm64-v8a",
            Self::X86_64 => "x86_64",
            Self::ArmeabiV7a => "armeabi-v7a",
        }
    }

    fn clang_name(self, api: u32) -> String {
        match self {
            Self::Arm64V8a => format!("aarch64-linux-android{api}-clang"),
            Self::X86_64 => format!("x86_64-linux-android{api}-clang"),
            Self::ArmeabiV7a => format!("armv7a-linux-androideabi{api}-clang"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AndroidArtifactKind {
    Apk,
    Aab,
}

impl AndroidArtifactKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "apk" => Ok(Self::Apk),
            "aab" => Ok(Self::Aab),
            _ => Err(format!(
                "unknown Android artifact format '{value}'; expected apk or aab"
            )),
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Apk => "apk",
            Self::Aab => "aab",
        }
    }
}

#[derive(Debug)]
struct AndroidBuildOptions {
    target: PathBuf,
    output: Option<PathBuf>,
    mode: BuildMode,
    abi: AndroidAbi,
    abi_explicit: bool,
    device: Option<String>,
    kind: AndroidArtifactKind,
}

#[derive(Debug)]
struct AndroidPublishOptions {
    target: PathBuf,
    output: Option<PathBuf>,
    json: bool,
}

impl From<String> for CliError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

pub fn main_entry() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Message(message)) => {
            eprintln!("{}: {message}", command_name());
            ExitCode::FAILURE
        }
        Err(CliError::Reported) => ExitCode::FAILURE,
    }
}

fn command_name() -> String {
    env::args()
        .next()
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()))
        .and_then(|name| name.to_str().map(str::to_string))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "flux".to_string())
}

fn run() -> Result<(), CliError> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err(CliError::Message(usage()));
    }

    match args[0].as_str() {
        "new" => {
            let path = require_target(&args)?;
            if args.len() != 2 {
                return Err(CliError::Message(
                    "new syntax is 'new <directory>'".to_string(),
                ));
            }
            create_project(path)?;
            println!("created: {}", path.display());
            Ok(())
        }
        "check" | "analyze" => {
            let path = require_target(&args)?;
            let json = analysis_json_mode(&args[0], &args[2..])?;
            let resolved =
                fluxc::project::resolve_entry(path).unwrap_or_else(|_| path.to_path_buf());
            let source_id = fluxc::SourceId::from_name(resolved.to_string_lossy().as_ref());
            let (diagnostics, sources) = fluxc::project::check_with_sources(path);
            if diagnostics.is_empty() {
                if json {
                    println!(
                        "{}",
                        fluxc::diagnostics_envelope_to_json(true, source_id, &[])
                    );
                } else {
                    println!("ok: {}", path.display());
                }
                return Ok(());
            }
            if json {
                println!(
                    "{}",
                    fluxc::diagnostics_envelope_to_json(false, source_id, &diagnostics)
                );
            } else {
                report_diagnostics(path, &diagnostics, &sources);
            }
            Err(CliError::Reported)
        }
        "format" => {
            if args.len() == 2 && args[1] == "--version" {
                println!("{}", fluxc::formatter::FORMATTER_VERSION);
                return Ok(());
            }
            let path = require_source(&args)?;
            let check_only = match &args[2..] {
                [] => false,
                [flag] if flag == "--check" => true,
                _ => {
                    return Err(CliError::Message(
                        "format syntax is 'format <file.flux> [--check]' or 'format --version'"
                            .to_string(),
                    ));
                }
            };
            let source = fs::read_to_string(path)
                .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
            let formatted = match fluxc::formatter::format_source(&source) {
                Ok(formatted) => formatted,
                Err(diagnostics) => {
                    let source_id = canonical_source_id(path);
                    let source_context = DiagnosticSource::new(
                        source_id,
                        path.display().to_string(),
                        source.clone(),
                    );
                    report_diagnostic_context(&diagnostics, &[source_context]);
                    return Err(CliError::Reported);
                }
            };
            if formatted == source {
                if check_only {
                    println!("formatted: {}", path.display());
                }
                return Ok(());
            }
            if check_only {
                return Err(CliError::Message(format!(
                    "{} is not canonically formatted",
                    path.display()
                )));
            }
            fs::write(path, formatted)
                .map_err(|error| format!("failed to write '{}': {error}", path.display()))?;
            println!("formatted: {}", path.display());
            Ok(())
        }
        "emit-c" => {
            let path = require_target(&args)?;
            let sources = validate_project(path)?;
            let generated = match fluxc::project::compile_to_c(path) {
                Ok(generated) => generated,
                Err(diagnostic) => {
                    report_diagnostics(path, &[diagnostic], &sources);
                    return Err(CliError::Reported);
                }
            };
            if let Some(output) = output_path(&args[2..])? {
                fs::write(&output, generated)
                    .map_err(|error| format!("failed to write '{}': {error}", output.display()))?;
            } else {
                print!("{generated}");
            }
            Ok(())
        }
        "build" => {
            if args.get(1).is_some_and(|value| value == "android") {
                return build_android_command(&args[2..], BuildMode::Release, false, true)
                    .map(|_| ());
            }
            let path = require_target(&args)?;
            let options = build_options(&args[2..], BuildMode::Release)?;
            let sources = validate_project(path)?;
            let generated = match fluxc::project::compile_to_c(path) {
                Ok(generated) => generated,
                Err(diagnostic) => {
                    report_diagnostics(path, &[diagnostic], &sources);
                    return Err(CliError::Reported);
                }
            };
            let output = if let Some(output) = options.output {
                output
            } else {
                let entry = fluxc::project::resolve_entry(path).map_err(|diagnostics| {
                    diagnostics
                        .into_iter()
                        .map(|diagnostic| diagnostic.to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                })?;
                default_binary_path(&entry)
            };
            build_native_configured(
                &generated,
                &output,
                options.mode,
                NativeInstrumentation::None,
                &options.native_target,
            )?;
            println!("built ({}): {}", options.mode.name(), output.display());
            Ok(())
        }
        "package" => {
            let path = require_target(&args)?;
            let options = package_options(&args[2..])?;
            package_target(path, options)
        }
        "publish" => {
            if args.get(1).is_some_and(|value| value == "android") {
                return publish_android_command(&args[2..]);
            }
            Err(CliError::Message(
                "publish syntax is 'publish android <package-dir|flux.toml> [-o artifact.aab] [--json]'"
                    .to_string(),
            ))
        }
        "run" => {
            if args.get(1).is_some_and(|value| value == "android") {
                let built = build_android_command(&args[2..], BuildMode::Debug, true, true)?;
                return run_android_apk(&built);
            }
            let path = require_target(&args)?;
            let options = build_options(&args[2..], BuildMode::Debug)?;
            if options.output.is_some() {
                return Err(CliError::Message(
                    "run does not accept '-o'; development binaries are managed automatically"
                        .to_string(),
                ));
            }
            if options.native_target != NativeTargetOptions::default() {
                return Err(CliError::Message(
                    "run executes on the development host and does not accept '--target' or '--sysroot'; use 'build' for cross-target artifacts"
                        .to_string(),
                ));
            }
            run_development(path, options.mode)
        }
        "test" => {
            let path = require_target(&args)?;
            let options = test_options(&args[2..])?;
            run_tests(path, options)
        }
        "debug" => {
            let options = debug_options(&args[1..])?;
            debug_target(&options)
        }
        "profile" => {
            let path = require_target(&args)?;
            if args.len() != 2 {
                return Err(CliError::Message(
                    "profile syntax is 'profile <file.flux|package-dir|flux.toml>'".to_string(),
                ));
            }
            profile_target(path)
        }
        "symbolize" => {
            let options = symbolize_options(&args[1..])?;
            symbolize_binary(&options)
        }
        "symbols" => {
            if !args.get(1).is_some_and(|value| value == "split") {
                return Err(CliError::Message(
                    "symbols syntax is 'symbols split <native-binary> [-o directory]'".to_string(),
                ));
            }
            let options = split_symbols_options(&args[2..])?;
            split_debug_symbols(&options)
        }
        "devices" => {
            if args.len() != 1 {
                return Err(CliError::Message("devices syntax is 'devices'".to_string()));
            }
            run_devices()
        }
        "doctor" => {
            if args.len() != 1 {
                return Err(CliError::Message("doctor syntax is 'doctor'".to_string()));
            }
            run_doctor()
        }
        "clean" => {
            let path = require_target(&args)?;
            if args.len() != 2 {
                return Err(CliError::Message(
                    "clean syntax is 'clean <file.flux|package-dir|flux.toml>'".to_string(),
                ));
            }
            clean_target(path)
        }
        "lsp" => {
            if args.len() != 1 {
                return Err(CliError::Message("lsp syntax is 'lsp'".to_string()));
            }
            fluxc::lsp::run_stdio()
                .map_err(|error| CliError::Message(format!("language server failed: {error}")))
        }
        _ => Err(CliError::Message(usage())),
    }
}

#[derive(Debug)]
struct AndroidBuildResult {
    artifact: PathBuf,
    application_id: String,
    activity_name: String,
    kind: AndroidArtifactKind,
    abi: AndroidAbi,
    device: Option<String>,
}

#[derive(Debug)]
struct AndroidSigningConfig {
    keystore: PathBuf,
    key_alias: String,
    store_password: std::ffi::OsString,
    key_password: std::ffi::OsString,
    release: bool,
}

fn build_android_command(
    args: &[String],
    default_mode: BuildMode,
    for_run: bool,
    announce: bool,
) -> Result<AndroidBuildResult, CliError> {
    let mut options = android_build_options(args, default_mode)?;
    if !for_run && options.device.is_some() {
        return Err(CliError::Message(
            "'--device' is only valid with 'run android'".to_string(),
        ));
    }
    if for_run && !options.abi_explicit {
        if let Some(detected) = detect_android_run_abi(options.device.as_deref()) {
            options.abi = detected;
        } else if let Some(device) = options.device.as_deref() {
            return Err(CliError::Message(format!(
                "could not determine ABI for requested Android device '{device}'; verify the device is responsive or pass '--abi <abi>' explicitly"
            )));
        }
    }
    if for_run && options.kind != AndroidArtifactKind::Apk {
        return Err(CliError::Message(
            "flux run android requires '--format apk'; AAB files are publishing artifacts and cannot be installed directly"
                .to_string(),
        ));
    }
    let manifest_path = if options.target.is_dir() {
        options.target.join("flux.toml")
    } else if options.target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        options.target.clone()
    } else {
        return Err(CliError::Message(
            "Android builds require a manifest-backed package directory or flux.toml".to_string(),
        ));
    };
    let manifest = fluxc::project::read_manifest(&manifest_path).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let package_root = manifest
        .path
        .parent()
        .expect("canonical manifest has a parent");
    let analysis = fluxc::project::analyze(&manifest.path).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    if analysis.program.application.is_none() {
        return Err(CliError::Message(
            "Android application builds require an 'app' declaration".to_string(),
        ));
    }
    let generated = analysis
        .emit_c_for_target(fluxc::codegen::NativeTarget::Android)
        .map_err(|diagnostic| diagnostic.to_string())?;
    let output = options.output.unwrap_or_else(|| {
        let file_name = match options.kind {
            AndroidArtifactKind::Apk => format!(
                "{}-{}.{}",
                manifest.name,
                options.abi.name(),
                options.kind.extension()
            ),
            AndroidArtifactKind::Aab => {
                format!(
                    "{}-{}.{}",
                    manifest.name,
                    options.mode.name(),
                    options.kind.extension()
                )
            }
        };
        package_root
            .join("build")
            .join("android")
            .join(options.mode.name())
            .join(file_name)
    });
    match options.kind {
        AndroidArtifactKind::Apk => {
            build_android_apk(&generated, &manifest, &output, options.mode, options.abi)?
        }
        AndroidArtifactKind::Aab => {
            build_android_aab(&generated, &manifest, &output, options.mode)?
        }
    }
    let target = if options.kind == AndroidArtifactKind::Apk {
        options.abi.name()
    } else {
        "all ABIs"
    };
    if announce {
        println!(
            "built Android {} {} {}: {}",
            options.kind.extension().to_uppercase(),
            target,
            options.mode.name(),
            output.display()
        );
    }
    Ok(AndroidBuildResult {
        artifact: output,
        application_id: manifest.android.application_id,
        activity_name: if android_has_generated_activity(&generated) {
            "app.flux.runtime.FluxActivity".to_string()
        } else {
            "android.app.NativeActivity".to_string()
        },
        kind: options.kind,
        abi: options.abi,
        device: options.device,
    })
}

fn validate_android_publish_manifest(
    manifest: &fluxc::project::PackageManifest,
) -> Result<(), String> {
    if manifest.version.is_none() {
        return Err(
            "Android publishing requires an explicit non-empty [package].version for Play versionName"
                .to_string(),
        );
    }
    if manifest.android.target_sdk < 36 {
        return Err(format!(
            "Android publishing requires [android].target_sdk >= 36 for the current Google Play phone/tablet submission requirement; got {}",
            manifest.android.target_sdk
        ));
    }
    if manifest.android.keystore.is_none() || manifest.android.key_alias.is_none() {
        return Err(
            "Android publishing requires [android].keystore and [android].key_alias; development signing keys are never accepted by 'publish android'"
                .to_string(),
        );
    }
    Ok(())
}

fn publish_android_command(args: &[String]) -> Result<(), CliError> {
    let options = android_publish_options(args)?;
    let manifest_path = if options.target.is_dir() {
        options.target.join("flux.toml")
    } else if options.target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        options.target.clone()
    } else {
        return Err(CliError::Message(
            "Android publishing requires a manifest-backed package directory or flux.toml"
                .to_string(),
        ));
    };
    let manifest = fluxc::project::read_manifest(&manifest_path).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    validate_android_publish_manifest(&manifest)?;
    android_signing_config(&manifest, BuildMode::Release)?;

    let mut build_args = vec![options.target.to_string_lossy().into_owned()];
    if let Some(output) = options.output.as_ref() {
        build_args.push("-o".to_string());
        build_args.push(output.to_string_lossy().into_owned());
    }
    build_args.push("--format".to_string());
    build_args.push("aab".to_string());
    let built = build_android_command(&build_args, BuildMode::Release, false, false)?;
    verify_android_publish_aab(&built.artifact)?;

    let version = manifest
        .version
        .as_deref()
        .expect("publish version preflight");
    if options.json {
        println!(
            "{{\"ok\":true,\"artifact\":{},\"application_id\":{},\"version\":{},\"version_code\":{},\"target_sdk\":{},\"format\":\"aab\",\"mode\":\"release\",\"verified\":true}}",
            json_string(&built.artifact.to_string_lossy()),
            json_string(&manifest.android.application_id),
            json_string(version),
            manifest.android.version_code,
            manifest.android.target_sdk,
        );
    } else {
        println!(
            "prepared Google Play upload bundle: {}",
            built.artifact.display()
        );
        println!("  application id: {}", manifest.android.application_id);
        println!(
            "  version: {} (code {})",
            version, manifest.android.version_code
        );
        println!("  target API: {}", manifest.android.target_sdk);
        println!("  signing: configured release/upload key");
        println!("  verification: bundletool validate + jarsigner signature check");
    }
    Ok(())
}

fn verify_android_publish_aab(output: &Path) -> Result<(), CliError> {
    let bundletool = find_android_bundletool()?;
    run_checked(
        Command::new("java")
            .arg("-jar")
            .arg(bundletool)
            .arg("validate")
            .arg(format!("--bundle={}", output.display())),
        "bundletool validate",
    )?;
    run_checked(
        Command::new("jarsigner").arg("-verify").arg(output),
        "jarsigner verify",
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AndroidRunTarget {
    Adb(String),
    Waydroid,
}

fn select_android_run_target(
    build_abi: AndroidAbi,
    ready_devices: &[(AdbDevice, Option<AndroidAbi>)],
    waydroid_is_running: bool,
    waydroid_abi: Option<AndroidAbi>,
    requested_device: Option<&str>,
) -> Result<AndroidRunTarget, String> {
    if let Some(requested) = requested_device {
        if requested.eq_ignore_ascii_case("waydroid") {
            if !waydroid_is_running {
                return Err("requested Android device 'waydroid' is not running".to_string());
            }
            if let Some(runtime_abi) = waydroid_abi
                && runtime_abi != build_abi
            {
                return Err(format!(
                    "Android artifact ABI {} does not match requested Waydroid runtime ABI {}",
                    build_abi.name(),
                    runtime_abi.name()
                ));
            }
            return Ok(AndroidRunTarget::Waydroid);
        }

        let Some((device, runtime_abi)) = ready_devices
            .iter()
            .find(|(device, _)| device.serial == requested)
        else {
            let available = ready_devices
                .iter()
                .map(|(device, _)| device.serial.as_str())
                .collect::<Vec<_>>();
            return Err(if available.is_empty() {
                format!("requested adb device '{requested}' is not ready; no adb devices are ready")
            } else {
                format!(
                    "requested adb device '{requested}' is not ready; ready devices: {}",
                    available.join(", ")
                )
            });
        };
        if let Some(runtime_abi) = runtime_abi
            && *runtime_abi != build_abi
        {
            return Err(format!(
                "Android artifact ABI {} does not match requested adb device {} ABI {}",
                build_abi.name(),
                device.serial,
                runtime_abi.name()
            ));
        }
        return Ok(AndroidRunTarget::Adb(device.serial.clone()));
    }

    let matching_adb = ready_devices
        .iter()
        .filter(|(_, abi)| *abi == Some(build_abi))
        .collect::<Vec<_>>();
    if let [entry] = matching_adb.as_slice() {
        return Ok(AndroidRunTarget::Adb(entry.0.serial.clone()));
    }
    if matching_adb.len() > 1 {
        return Err(format!(
            "multiple adb Android devices match ABI {} ({}); rerun with '--device <serial>' to select one explicitly",
            build_abi.name(),
            matching_adb
                .iter()
                .map(|(device, _)| device.serial.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if waydroid_is_running && waydroid_abi == Some(build_abi) {
        return Ok(AndroidRunTarget::Waydroid);
    }
    if ready_devices.len() == 1 && ready_devices[0].1.is_none() && !waydroid_is_running {
        return Ok(AndroidRunTarget::Adb(ready_devices[0].0.serial.clone()));
    }
    if ready_devices.is_empty() && waydroid_is_running && waydroid_abi.is_none() {
        return Ok(AndroidRunTarget::Waydroid);
    }

    let mut available = ready_devices
        .iter()
        .map(|(device, abi)| match abi {
            Some(abi) => format!("adb {} ({})", device.serial, abi.name()),
            None => format!("adb {} (unknown ABI)", device.serial),
        })
        .collect::<Vec<_>>();
    if waydroid_is_running {
        available.push(match waydroid_abi {
            Some(abi) => format!("Waydroid ({})", abi.name()),
            None => "Waydroid (unknown ABI)".to_string(),
        });
    }
    if available.is_empty() {
        return Err(
            "no runnable Android device found; connect one through adb or start Waydroid"
                .to_string(),
        );
    }
    Err(format!(
        "Android artifact ABI {} does not match an available runtime: {}",
        build_abi.name(),
        available.join(", ")
    ))
}

fn run_android_apk(build: &AndroidBuildResult) -> Result<(), CliError> {
    debug_assert_eq!(build.kind, AndroidArtifactKind::Apk);
    let requested_device = build.device.as_deref();
    let requested_waydroid =
        requested_device.is_some_and(|device| device.eq_ignore_ascii_case("waydroid"));
    let ready_devices = if requested_waydroid {
        Vec::new()
    } else {
        adb_devices()
            .unwrap_or_default()
            .into_iter()
            .filter(|device| {
                device.status == "device"
                    && requested_device.is_none_or(|requested| device.serial == requested)
            })
            .map(|device| {
                let abi = adb_device_abi(&device.serial);
                (device, abi)
            })
            .collect::<Vec<_>>()
    };
    let probe_waydroid = requested_waydroid || requested_device.is_none();
    let waydroid_is_running = probe_waydroid && waydroid_running().unwrap_or(false);
    let waydroid_abi = waydroid_is_running.then(waydroid_runtime_abi).flatten();
    match select_android_run_target(
        build.abi,
        &ready_devices,
        waydroid_is_running,
        waydroid_abi,
        build.device.as_deref(),
    )
    .map_err(CliError::Message)?
    {
        AndroidRunTarget::Adb(serial) => run_android_apk_adb(build, &serial),
        AndroidRunTarget::Waydroid => run_android_apk_waydroid(build),
    }
}

fn run_android_apk_adb(build: &AndroidBuildResult, serial: &str) -> Result<(), CliError> {
    let adb = adb_path();
    let install = output_with_timeout(
        Command::new(&adb)
            .args(["-s", serial, "install", "-r"])
            .arg(&build.artifact),
        Duration::from_secs(120),
    )
    .map_err(|error| format!("failed to launch adb: {error}"))?
    .ok_or_else(|| {
        CliError::Message(format!(
            "adb install timed out after 120 seconds for {serial}; verify the device connection and retry"
        ))
    })?;
    if !install.status.success() {
        return Err(CliError::Message(format!(
            "adb install failed for {serial}:\n{}",
            String::from_utf8_lossy(&install.stderr)
        )));
    }
    let component = format!("{}/{}", build.application_id, build.activity_name);
    let launch = output_with_timeout(
        Command::new(&adb)
            .args(["-s", serial, "shell", "am", "start", "-W", "-n"])
            .arg(&component),
        Duration::from_secs(30),
    )
    .map_err(|error| format!("failed to launch Android application through adb: {error}"))?
    .ok_or_else(|| {
        CliError::Message(format!(
            "adb launch timed out after 30 seconds for {serial} ({component})"
        ))
    })?;
    let launch_stdout = String::from_utf8_lossy(&launch.stdout);
    let launch_stderr = String::from_utf8_lossy(&launch.stderr);
    if !launch.status.success()
        || launch_stdout.contains("Error:")
        || launch_stderr.contains("Error:")
    {
        return Err(CliError::Message(format!(
            "adb launch failed for {serial} ({component}):\n{launch_stdout}{launch_stderr}"
        )));
    }
    println!(
        "launched Android app on adb device {serial}: {}",
        build.application_id
    );
    Ok(())
}

fn run_android_apk_waydroid(build: &AndroidBuildResult) -> Result<(), CliError> {
    run_checked_with_timeout(
        Command::new("waydroid")
            .args(["app", "install"])
            .arg(&build.artifact),
        "Waydroid install",
        Duration::from_secs(120),
    )?;
    run_checked_with_timeout(
        Command::new("waydroid")
            .args(["app", "launch"])
            .arg(&build.application_id),
        "Waydroid launch",
        Duration::from_secs(30),
    )?;
    println!("launched Android app on Waydroid: {}", build.application_id);
    Ok(())
}

fn package_target(target: &Path, options: PackageOptions) -> Result<(), CliError> {
    let manifest_path = if target.is_dir() {
        target.join("flux.toml")
    } else if target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        target.to_path_buf()
    } else {
        return Err(CliError::Message(
            "package requires a manifest-backed package directory or flux.toml".to_string(),
        ));
    };
    let manifest = fluxc::project::read_manifest(&manifest_path).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let package_root = manifest
        .path
        .parent()
        .expect("canonical manifest has a parent");
    let artifact_name = package_artifact_name(
        &manifest.name,
        manifest.version.as_deref(),
        options.native_target.triple.as_deref(),
    )?;
    let default_output = match options.format {
        PackageFormat::Directory => package_root.join("dist").join(&artifact_name),
        PackageFormat::TarGz => package_root
            .join("dist")
            .join(format!("{artifact_name}.tar.gz")),
        PackageFormat::Container => package_root
            .join("dist")
            .join(format!("{artifact_name}-container")),
        PackageFormat::Systemd => package_root
            .join("dist")
            .join(format!("{artifact_name}-systemd")),
    };
    let output = options.output.unwrap_or(default_output);
    if output.exists() {
        return Err(CliError::Message(format!(
            "package output '{}' already exists; remove it or choose another path with -o",
            output.display()
        )));
    }

    if matches!(
        options.format,
        PackageFormat::Container | PackageFormat::Systemd
    ) {
        let analysis = fluxc::project::analyze(&manifest.path).map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        if analysis.program.application.is_some() {
            let format = match options.format {
                PackageFormat::Container => "container",
                PackageFormat::Systemd => "systemd",
                _ => unreachable!(),
            };
            return Err(CliError::Message(format!(
                "{format} packaging currently supports headless 'fn main() -> i64' packages only"
            )));
        }
    }

    let sources = validate_project(&manifest.path)?;
    let generated = match fluxc::project::compile_to_c(&manifest.path) {
        Ok(generated) => generated,
        Err(diagnostic) => {
            report_diagnostics(&manifest.path, &[diagnostic], &sources);
            return Err(CliError::Reported);
        }
    };

    match options.format {
        PackageFormat::Directory => {
            build_package_directory(
                &manifest,
                &generated,
                &output,
                options.mode,
                &options.native_target,
            )?;
        }
        PackageFormat::TarGz => {
            command_first_line("tar", &["--version"]).map_err(|message| {
                CliError::Message(format!("Flux tar.gz packaging requires tar: {message}"))
            })?;
            if let Some(parent) = output.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "failed to create package output directory '{}': {error}",
                        parent.display()
                    )
                })?;
            }
            let staging_root = package_staging_dir();
            let staged_bundle = staging_root.join(&artifact_name);
            let result = (|| -> Result<(), CliError> {
                fs::create_dir_all(&staging_root).map_err(|error| {
                    format!("failed to create package staging directory: {error}")
                })?;
                build_package_directory(
                    &manifest,
                    &generated,
                    &staged_bundle,
                    options.mode,
                    &options.native_target,
                )?;
                run_checked(
                    Command::new("tar")
                        .args([
                            "--sort=name",
                            "--mtime=@0",
                            "--owner=0",
                            "--group=0",
                            "--numeric-owner",
                            "-czf",
                        ])
                        .arg(&output)
                        .arg("-C")
                        .arg(&staging_root)
                        .arg(&artifact_name),
                    "tar.gz package archive",
                )?;
                Ok(())
            })();
            let _ = fs::remove_dir_all(&staging_root);
            if let Err(error) = result {
                let _ = fs::remove_file(&output);
                return Err(error);
            }
        }
        PackageFormat::Container => {
            build_container_context(&generated, &output, options.mode, &options.native_target)?;
        }
        PackageFormat::Systemd => {
            build_systemd_bundle(
                &manifest.name,
                &generated,
                &output,
                options.mode,
                &options.native_target,
            )?;
        }
    }
    println!("packaged ({}): {}", options.mode.name(), output.display());
    Ok(())
}

fn build_package_directory(
    manifest: &fluxc::project::PackageManifest,
    generated: &str,
    output: &Path,
    mode: BuildMode,
    native_target: &NativeTargetOptions,
) -> Result<(), CliError> {
    fs::create_dir_all(output)
        .map_err(|error| format!("failed to create package '{}': {error}", output.display()))?;
    let binary = output.join(&manifest.name);
    if let Err(error) = build_native_configured(
        generated,
        &binary,
        mode,
        NativeInstrumentation::None,
        native_target,
    ) {
        let _ = fs::remove_dir_all(output);
        return Err(CliError::Message(error));
    }
    if let Err(error) = fs::copy(&manifest.path, output.join("flux.toml")) {
        let _ = fs::remove_dir_all(output);
        return Err(CliError::Message(format!(
            "failed to copy package manifest: {error}"
        )));
    }
    Ok(())
}

fn build_container_context(
    generated: &str,
    output: &Path,
    mode: BuildMode,
    native_target: &NativeTargetOptions,
) -> Result<(), CliError> {
    fs::create_dir_all(output).map_err(|error| {
        format!(
            "failed to create container build context '{}': {error}",
            output.display()
        )
    })?;
    let binary = output.join("app");
    if let Err(error) = build_native_configured(
        generated,
        &binary,
        mode,
        NativeInstrumentation::None,
        native_target,
    ) {
        let _ = fs::remove_dir_all(output);
        return Err(CliError::Message(error));
    }
    let containerfile = "FROM debian:stable-slim\nWORKDIR /app\nCOPY app /app/flux-app\nENTRYPOINT [\"/app/flux-app\"]\n";
    if let Err(error) = fs::write(output.join("Containerfile"), containerfile) {
        let _ = fs::remove_dir_all(output);
        return Err(CliError::Message(format!(
            "failed to write container build context: {error}"
        )));
    }
    Ok(())
}

fn build_systemd_bundle(
    service_name: &str,
    generated: &str,
    output: &Path,
    mode: BuildMode,
    native_target: &NativeTargetOptions,
) -> Result<(), CliError> {
    if service_name.is_empty()
        || !service_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(CliError::Message(
            "systemd service package names may contain only ASCII letters, digits, '.', '-', and '_'"
                .to_string(),
        ));
    }
    fs::create_dir_all(output).map_err(|error| {
        format!(
            "failed to create systemd service bundle '{}': {error}",
            output.display()
        )
    })?;
    let binary = output.join(service_name);
    if let Err(error) = build_native_configured(
        generated,
        &binary,
        mode,
        NativeInstrumentation::None,
        native_target,
    ) {
        let _ = fs::remove_dir_all(output);
        return Err(CliError::Message(error));
    }
    let absolute_output = fs::canonicalize(output).map_err(|error| {
        CliError::Message(format!(
            "failed to resolve systemd service bundle '{}': {error}",
            output.display()
        ))
    })?;
    let absolute_binary = absolute_output.join(service_name);
    let quote = |path: &Path| {
        let escaped = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!("\"{escaped}\"")
    };
    let unit = format!(
        "[Unit]\nDescription=Flux service {service_name}\nAfter=network.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nExecStart={}\nRestart=on-failure\nRestartSec=1s\nKillSignal=SIGTERM\nTimeoutStopSec=30s\n\n[Install]\nWantedBy=default.target\n",
        quote(&absolute_output),
        quote(&absolute_binary),
    );
    if let Err(error) = fs::write(output.join(format!("{service_name}.service")), unit) {
        let _ = fs::remove_dir_all(output);
        return Err(CliError::Message(format!(
            "failed to write systemd service unit: {error}"
        )));
    }
    Ok(())
}

fn package_staging_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("flux-package-{}-{nonce}", std::process::id()))
}

fn package_artifact_name(
    name: &str,
    version: Option<&str>,
    target_triple: Option<&str>,
) -> Result<String, CliError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(CliError::Message(
            "package name must be a safe filesystem name without path separators".to_string(),
        ));
    }
    let version = version.unwrap_or("unversioned");
    if version.is_empty()
        || version == "."
        || version == ".."
        || version.contains('/')
        || version.contains('\\')
    {
        return Err(CliError::Message(
            "package version must be safe for a package artifact name".to_string(),
        ));
    }
    let platform = target_triple
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", env::consts::OS, env::consts::ARCH));
    Ok(format!("{name}-{version}-{platform}"))
}

fn create_project(target: &Path) -> Result<(), CliError> {
    if target.exists() {
        if !target.is_dir() {
            return Err(CliError::Message(format!(
                "cannot create project '{}': path is not a directory",
                target.display()
            )));
        }
        let mut entries = fs::read_dir(target)
            .map_err(|error| format!("failed to inspect '{}': {error}", target.display()))?;
        if entries.next().is_some() {
            return Err(CliError::Message(format!(
                "cannot create project '{}': directory is not empty",
                target.display()
            )));
        }
    }

    let src = target.join("src");
    let tests = target.join("tests");
    fs::create_dir_all(&src)
        .map_err(|error| format!("failed to create '{}': {error}", src.display()))?;
    fs::create_dir_all(&tests)
        .map_err(|error| format!("failed to create '{}': {error}", tests.display()))?;
    let package_name = project_name_from_path(target);
    let manifest = format!(
        "[package]\nformat_version = {}\nname = \"{package_name}\"\nversion = \"0.1.0\"\nentry = \"src/main.flux\"\n",
        fluxc::project::PACKAGE_FORMAT_VERSION
    );
    let main = "view App {\n    grid columns: 1fr\n    grid rows: auto auto\n    grid gap: 12\n    state clicked: bool = false\n\n    Text title at 1,1\n        text: \"Hello, Flux!\"\n        visible: !clicked\n\n    Button action at 2,1\n        text: \"Toggle\"\n        onPress: clicked => !clicked\n}\n\napp App\n";
    fs::write(target.join("flux.toml"), manifest)
        .map_err(|error| format!("failed to write project manifest: {error}"))?;
    fs::write(src.join("main.flux"), main)
        .map_err(|error| format!("failed to write project entry source: {error}"))?;
    fs::write(
        tests.join("smoke.flux"),
        "fn main() -> i64 {\n    return 0\n}\n",
    )
    .map_err(|error| format!("failed to write starter integration test: {error}"))?;
    Ok(())
}

fn project_name_from_path(target: &Path) -> String {
    let raw = target
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("flux-app");
    let mut out = String::new();
    let mut last_dash = false;
    for ch in raw.chars() {
        let ch = if ch.is_ascii_alphanumeric() || ch == '_' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if ch == '-' {
            if out.is_empty() || last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        out.push(ch);
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "flux-app".to_string()
    } else {
        out
    }
}

fn run_tests(target: &Path, options: TestOptions) -> Result<(), CliError> {
    let tests = discover_test_targets(target)?;
    let coverage_dir = options.coverage.then(coverage_data_dir);
    if let Some(directory) = &coverage_dir {
        command_first_line("llvm-profdata", &["--version"]).map_err(|message| {
            CliError::Message(format!(
                "Flux test coverage requires llvm-profdata: {message}"
            ))
        })?;
        command_first_line("llvm-cov", &["--version"]).map_err(|message| {
            CliError::Message(format!("Flux test coverage requires llvm-cov: {message}"))
        })?;
        if directory.exists() {
            fs::remove_dir_all(directory).map_err(|error| {
                format!(
                    "failed to clear coverage data '{}': {error}",
                    directory.display()
                )
            })?;
        }
        fs::create_dir_all(directory).map_err(|error| {
            format!(
                "failed to create coverage data '{}': {error}",
                directory.display()
            )
        })?;
    }

    let result = run_tests_inner(&tests, options, coverage_dir.as_deref());
    for index in 0..tests.len() {
        let _ = fs::remove_file(test_binary_path(index));
    }
    if let Some(directory) = &coverage_dir {
        let _ = fs::remove_dir_all(directory);
    }
    result
}

fn run_tests_inner(
    tests: &[PathBuf],
    options: TestOptions,
    coverage_dir: Option<&Path>,
) -> Result<(), CliError> {
    let mut passed = 0usize;
    let mut binaries = Vec::new();
    let mut raw_profiles = Vec::new();
    let mut generated_sources = Vec::new();
    for (index, test) in tests.iter().enumerate() {
        let analysis = match fluxc::project::analyze(test) {
            Ok(analysis) => analysis,
            Err(diagnostics) => {
                let (_, sources) = fluxc::project::check_with_sources(test);
                eprintln!("test {} ... FAILED", test.display());
                report_diagnostics(test, &diagnostics, &sources);
                return Err(CliError::Reported);
            }
        };
        if analysis.program.application.is_some() {
            return Err(CliError::Message(format!(
                "test '{}' declares an app; bootstrap flux test runs headless fn main() integration programs",
                test.display()
            )));
        }
        let generated = match analysis.emit_c() {
            Ok(generated) => generated,
            Err(diagnostic) => {
                eprintln!("test {} ... FAILED", test.display());
                report_diagnostics(test, &[diagnostic], &analysis.sources);
                return Err(CliError::Reported);
            }
        };
        let binary = test_binary_path(index);
        if options.coverage {
            build_native_instrumented(
                &generated,
                &binary,
                options.mode,
                NativeInstrumentation::Coverage,
            )?;
        } else {
            build_native(&generated, &binary, options.mode)?;
        }
        let raw_profile =
            coverage_dir.map(|directory| directory.join(format!("test-{index}.profraw")));
        let mut command = Command::new(&binary);
        if let Some(raw_profile) = &raw_profile {
            command.env("LLVM_PROFILE_FILE", raw_profile);
        }
        let status = command.status().map_err(|error| {
            format!(
                "failed to launch test binary '{}': {error}",
                binary.display()
            )
        })?;
        if !status.success() {
            eprintln!("test {} ... FAILED", test.display());
            return Err(CliError::Message(format!(
                "test '{}' exited with {}",
                test.display(),
                status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "a signal".to_string())
            )));
        }
        if options.coverage {
            binaries.push(binary);
            generated_sources.push(generated);
            if let Some(raw_profile) = raw_profile {
                raw_profiles.push(raw_profile);
            }
        } else {
            let _ = fs::remove_file(&binary);
        }
        println!("test {} ... ok", test.display());
        passed += 1;
    }

    if let Some(directory) = coverage_dir {
        print_coverage_report(&binaries, &raw_profiles, &generated_sources, directory)?;
    }
    println!(
        "test result: ok. {passed} passed; 0 failed; mode {}{}",
        options.mode.name(),
        if options.coverage {
            "; coverage reported"
        } else {
            ""
        }
    );
    Ok(())
}

fn coverage_data_dir() -> PathBuf {
    env::temp_dir().join(format!("flux-coverage-{}", std::process::id()))
}

fn print_coverage_report(
    binaries: &[PathBuf],
    raw_profiles: &[PathBuf],
    generated_sources: &[String],
    directory: &Path,
) -> Result<(), CliError> {
    if binaries.is_empty() || raw_profiles.is_empty() {
        return Err(CliError::Message(
            "coverage requires at least one successfully executed Flux test".to_string(),
        ));
    }
    if binaries.len() != raw_profiles.len() || binaries.len() != generated_sources.len() {
        return Err(CliError::Message(
            "internal coverage artifact count mismatch".to_string(),
        ));
    }
    for profile in raw_profiles {
        if !profile.is_file() {
            return Err(CliError::Message(format!(
                "test coverage data '{}' was not produced",
                profile.display()
            )));
        }
    }
    let merged = directory.join("coverage.profdata");
    let mut merge = Command::new("llvm-profdata");
    merge.args(["merge", "-sparse", "-o"]).arg(&merged);
    for profile in raw_profiles {
        merge.arg(profile);
    }
    run_checked(&mut merge, "llvm-profdata merge")?;

    let generated_path = directory.join("<stdin>");
    let path_equivalence = format!(
        "--path-equivalence={COVERAGE_COMPILATION_DIR},{}",
        directory.display()
    );
    let mut source_lines: BTreeMap<PathBuf, BTreeMap<usize, bool>> = BTreeMap::new();
    for (binary, generated) in binaries.iter().zip(generated_sources) {
        fs::write(&generated_path, generated).map_err(|error| {
            format!(
                "failed to stage generated coverage source '{}': {error}",
                generated_path.display()
            )
        })?;
        let output = Command::new("llvm-cov")
            .arg("show")
            .arg(binary)
            .arg("-instr-profile")
            .arg(&merged)
            .arg("--show-line-counts-or-regions")
            .arg("--use-color=false")
            .arg(&path_equivalence)
            .output()
            .map_err(|error| format!("failed to launch llvm-cov: {error}"))?;
        if !output.status.success() {
            return Err(CliError::Message(format!(
                "llvm-cov failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let locations = coverage_source_locations(generated);
        collect_flux_coverage(
            &String::from_utf8_lossy(&output.stdout),
            &locations,
            &mut source_lines,
        );
    }
    let _ = fs::remove_file(&generated_path);

    if source_lines.is_empty() {
        return Err(CliError::Message(
            "coverage instrumentation produced no Flux source locations".to_string(),
        ));
    }
    print_flux_coverage_summary(&source_lines);
    Ok(())
}

fn coverage_source_locations(generated: &str) -> Vec<Option<(PathBuf, usize)>> {
    let mut locations = vec![None; generated.lines().count() + 1];
    let mut current: Option<(PathBuf, usize)> = None;
    for (index, line) in generated.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("#line ") {
            if let Some((number, quoted_path)) = rest.split_once(' ')
                && let Ok(line_number) = number.parse::<usize>()
                && let Some(path) = quoted_path
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
            {
                current = Some((PathBuf::from(path), line_number));
                continue;
            }
        }
        if let Some((path, line_number)) = &mut current {
            if path.extension().and_then(|value| value.to_str()) == Some("flux") {
                locations[index + 1] = Some((path.clone(), *line_number));
            }
            *line_number += 1;
        }
    }
    locations
}

fn collect_flux_coverage(
    report: &str,
    locations: &[Option<(PathBuf, usize)>],
    source_lines: &mut BTreeMap<PathBuf, BTreeMap<usize, bool>>,
) {
    for line in report.lines() {
        let mut columns = line.splitn(3, '|');
        let Some(generated_line) = columns
            .next()
            .and_then(|value| value.trim().parse::<usize>().ok())
        else {
            continue;
        };
        let Some(count) = columns.next().map(str::trim) else {
            continue;
        };
        if count.is_empty() {
            continue;
        }
        let Some(Some((source_path, source_line))) = locations.get(generated_line) else {
            continue;
        };
        let covered = count.bytes().any(|value| matches!(value, b'1'..=b'9'));
        source_lines
            .entry(source_path.clone())
            .or_default()
            .entry(*source_line)
            .and_modify(|existing| *existing |= covered)
            .or_insert(covered);
    }
}

fn print_flux_coverage_summary(source_lines: &BTreeMap<PathBuf, BTreeMap<usize, bool>>) {
    let display_paths = source_lines
        .keys()
        .map(|path| {
            env::current_dir()
                .ok()
                .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
                .unwrap_or_else(|| path.clone())
        })
        .collect::<Vec<_>>();
    let width = display_paths
        .iter()
        .map(|path| path.display().to_string().len())
        .max()
        .unwrap_or(8)
        .max(8);
    println!("coverage: Flux source lines");
    println!(
        "{:<width$}  {:>8}  {:>8}  {:>8}",
        "Filename", "Lines", "Missed", "Cover%"
    );
    let mut total_lines = 0usize;
    let mut total_covered = 0usize;
    for ((_, lines), display_path) in source_lines.iter().zip(&display_paths) {
        let covered = lines.values().filter(|covered| **covered).count();
        let count = lines.len();
        total_lines += count;
        total_covered += covered;
        let percent = if count == 0 {
            100.0
        } else {
            covered as f64 * 100.0 / count as f64
        };
        println!(
            "{:<width$}  {:>8}  {:>8}  {:>7.2}",
            display_path.display(),
            count,
            count - covered,
            percent
        );
    }
    let total_percent = if total_lines == 0 {
        100.0
    } else {
        total_covered as f64 * 100.0 / total_lines as f64
    };
    println!(
        "{:<width$}  {:>8}  {:>8}  {:>7.2}",
        "TOTAL",
        total_lines,
        total_lines - total_covered,
        total_percent
    );
}

fn discover_test_targets(target: &Path) -> Result<Vec<PathBuf>, CliError> {
    if target.extension().and_then(|value| value.to_str()) == Some("flux") && target.is_file() {
        return Ok(vec![fs::canonicalize(target).map_err(|error| {
            format!("failed to resolve test '{}': {error}", target.display())
        })?]);
    }
    let root = if target.is_dir() {
        fs::canonicalize(target)
            .map_err(|error| format!("failed to resolve package '{}': {error}", target.display()))?
    } else if target.file_name().and_then(|value| value.to_str()) == Some("flux.toml") {
        fs::canonicalize(target)
            .map_err(|error| format!("failed to resolve manifest '{}': {error}", target.display()))?
            .parent()
            .expect("canonical manifest has a parent")
            .to_path_buf()
    } else {
        return Err(CliError::Message(
            "test target must be a .flux file, package directory, or flux.toml".to_string(),
        ));
    };
    let tests_dir = root.join("tests");
    let entries = fs::read_dir(&tests_dir).map_err(|error| {
        format!(
            "failed to discover Flux tests in '{}': {error}",
            tests_dir.display()
        )
    })?;
    let mut tests = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("flux")
        })
        .collect::<Vec<_>>();
    tests.sort();
    if tests.is_empty() {
        return Err(CliError::Message(format!(
            "no .flux integration tests found in '{}'",
            tests_dir.display()
        )));
    }
    Ok(tests)
}

fn test_binary_path(index: usize) -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    env::temp_dir().join(format!("flux-test-{}-{index}{suffix}", std::process::id()))
}

fn debug_options(args: &[String]) -> Result<DebugOptions, String> {
    let Some(target) = args.first() else {
        return Err(
            "debug syntax is 'debug <file.flux|package-dir|flux.toml> [--break <file:line|function>] [--run]'"
                .to_string(),
        );
    };
    if target.starts_with('-') {
        return Err("debug requires a Flux source or package target before options".to_string());
    }

    let mut breakpoints = Vec::new();
    let mut run_immediately = false;
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "--break" | "-b" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("'--break' requires a Flux file:line or function name".to_string());
                };
                if value.is_empty() || value.starts_with('-') {
                    return Err("'--break' requires a Flux file:line or function name".to_string());
                }
                breakpoints.push(value.clone());
                index += 2;
            }
            "--run" => {
                if run_immediately {
                    return Err("'--run' may only be supplied once".to_string());
                }
                run_immediately = true;
                index += 1;
            }
            flag => {
                return Err(format!(
                    "unknown debug option '{flag}'; expected '--break <file:line|function>' or '--run'"
                ));
            }
        }
    }

    Ok(DebugOptions {
        target: PathBuf::from(target),
        breakpoints,
        run_immediately,
    })
}

fn debug_target(options: &DebugOptions) -> Result<(), CliError> {
    command_first_line("gdb", &["--version"])
        .map_err(|message| CliError::Message(format!("Flux debugger requires GDB: {message}")))?;

    let sources = validate_project(&options.target)?;
    let generated = match fluxc::project::compile_to_c(&options.target) {
        Ok(generated) => generated,
        Err(diagnostic) => {
            report_diagnostics(&options.target, &[diagnostic], &sources);
            return Err(CliError::Reported);
        }
    };
    let binary = debug_binary_path();
    build_native(&generated, &binary, BuildMode::Debug)?;
    let support = debug_support_path();
    if let Err(error) = fs::write(&support, FLUX_GDB_SUPPORT) {
        let _ = fs::remove_file(&binary);
        return Err(CliError::Message(format!(
            "failed to prepare Flux debugger support '{}': {error}",
            support.display()
        )));
    }

    let mut command = Command::new("gdb");
    command.arg("--quiet").arg("-x").arg(&support);
    for breakpoint in &options.breakpoints {
        command.arg("-ex").arg(format!("break {breakpoint}"));
    }
    if options.run_immediately {
        command.arg("-ex").arg("run");
    }
    command.arg(&binary);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    eprintln!("debug: built native debug binary; Flux source paths and lines are available to GDB");
    if !options.run_immediately {
        eprintln!("debug: use 'run', 'break file.flux:line', 'next', 'step', and 'bt' normally");
    }
    let status = command
        .status()
        .map_err(|error| format!("failed to launch GDB: {error}"));
    let _ = fs::remove_file(&binary);
    let _ = fs::remove_file(&support);
    let status = status?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "GDB exited with status {}",
            status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string())
        )))
    }
}

fn symbolize_options(args: &[String]) -> Result<SymbolizeOptions, String> {
    let Some(binary) = args.first() else {
        return Err(
            "symbolize syntax is 'symbolize <native-binary> <address> [address ...]'".to_string(),
        );
    };
    if binary.starts_with('-') || args.len() < 2 {
        return Err(
            "symbolize syntax is 'symbolize <native-binary> <address> [address ...]'".to_string(),
        );
    }
    let mut addresses = Vec::with_capacity(args.len() - 1);
    for address in &args[1..] {
        let normalized = address.strip_prefix("0x").unwrap_or(address);
        if normalized.is_empty() || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(format!(
                "invalid native address '{address}'; expected hexadecimal such as 0x401234"
            ));
        }
        addresses.push(normalized.to_ascii_lowercase());
    }
    Ok(SymbolizeOptions {
        binary: PathBuf::from(binary),
        addresses,
    })
}

fn symbolize_binary(options: &SymbolizeOptions) -> Result<(), CliError> {
    if !options.binary.is_file() {
        return Err(CliError::Message(format!(
            "symbolization input '{}' is not a native binary file",
            options.binary.display()
        )));
    }
    command_first_line("addr2line", &["--version"]).map_err(|message| {
        CliError::Message(format!(
            "Flux crash symbolization requires addr2line: {message}"
        ))
    })?;

    let mut command = Command::new("addr2line");
    command.args(["-f", "-e"]).arg(&options.binary);
    for address in &options.addresses {
        command.arg(format!("0x{address}"));
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to launch addr2line: {error}"))?;
    if !output.status.success() {
        return Err(CliError::Message(format!(
            "addr2line failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if lines.len() < options.addresses.len() * 2 {
        return Err(CliError::Message(
            "addr2line returned incomplete symbolization output".to_string(),
        ));
    }
    for (index, address) in options.addresses.iter().enumerate() {
        let function = display_flux_symbol(lines[index * 2].trim());
        let location = lines[index * 2 + 1].trim();
        println!("0x{address}  {function}  {location}");
    }
    Ok(())
}

fn split_symbols_options(args: &[String]) -> Result<SplitSymbolsOptions, String> {
    let Some(binary) = args.first() else {
        return Err(
            "symbols split syntax is 'symbols split <native-binary> [-o directory]'".to_string(),
        );
    };
    if binary.starts_with('-') {
        return Err("symbols split requires a native binary before options".to_string());
    }
    let mut output_directory = None;
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "-o" => {
                if output_directory.is_some() {
                    return Err("symbol output directory may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'-o' requires an output directory".to_string());
                };
                if value.is_empty() || value.starts_with('-') {
                    return Err("'-o' requires an output directory".to_string());
                }
                output_directory = Some(PathBuf::from(value));
                index += 2;
            }
            flag => {
                return Err(format!(
                    "unknown symbols split option '{flag}'; expected '-o <directory>'"
                ));
            }
        }
    }
    Ok(SplitSymbolsOptions {
        binary: PathBuf::from(binary),
        output_directory,
    })
}

fn split_debug_symbols(options: &SplitSymbolsOptions) -> Result<(), CliError> {
    if !cfg!(target_os = "linux") {
        return Err(CliError::Message(
            "bootstrap debug-symbol separation currently supports Linux ELF binaries".to_string(),
        ));
    }
    if !options.binary.is_file() {
        return Err(CliError::Message(format!(
            "debug-symbol input '{}' is not a native binary file",
            options.binary.display()
        )));
    }
    command_first_line("objcopy", &["--version"]).map_err(|message| {
        CliError::Message(format!(
            "Flux debug-symbol separation requires objcopy: {message}"
        ))
    })?;
    let file_name = options
        .binary
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            CliError::Message("native binary must have a filesystem name".to_string())
        })?;
    let output_directory = options.output_directory.clone().unwrap_or_else(|| {
        options
            .binary
            .with_file_name(format!("{file_name}.symbols"))
    });
    if output_directory.exists() {
        return Err(CliError::Message(format!(
            "symbol output '{}' already exists; remove it or choose another path with -o",
            output_directory.display()
        )));
    }
    fs::create_dir_all(&output_directory).map_err(|error| {
        CliError::Message(format!(
            "failed to create symbol output '{}': {error}",
            output_directory.display()
        ))
    })?;
    let symbols = output_directory.join(format!("{file_name}.debug"));
    let stripped = output_directory.join(file_name);
    let result = (|| -> Result<(), CliError> {
        run_checked(
            Command::new("objcopy")
                .arg("--only-keep-debug")
                .arg(&options.binary)
                .arg(&symbols),
            "debug-symbol extraction",
        )?;
        run_checked(
            Command::new("objcopy")
                .arg("--strip-debug")
                .arg(&options.binary)
                .arg(&stripped),
            "debug-symbol stripping",
        )?;
        run_checked(
            Command::new("objcopy")
                .arg(format!("--add-gnu-debuglink={}", symbols.display()))
                .arg(&stripped),
            "debug-symbol link",
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&output_directory);
        return Err(error);
    }
    println!("stripped: {}", stripped.display());
    println!("symbols: {}", symbols.display());
    Ok(())
}

fn display_flux_symbol(symbol: &str) -> &str {
    symbol.strip_prefix("flux__fn_").unwrap_or(symbol)
}

fn debug_binary_path() -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    env::temp_dir().join(format!("flux-debug-{}{suffix}", std::process::id()))
}

fn debug_support_path() -> PathBuf {
    env::temp_dir().join(format!("flux-gdb-{}.py", std::process::id()))
}

fn profile_target(target: &Path) -> Result<(), CliError> {
    command_first_line("gprof", &["--version"]).map_err(|message| {
        CliError::Message(format!("Flux CPU profiling requires gprof: {message}"))
    })?;

    let sources = validate_project(target)?;
    let generated = match fluxc::project::compile_to_c(target) {
        Ok(generated) => generated,
        Err(diagnostic) => {
            report_diagnostics(target, &[diagnostic], &sources);
            return Err(CliError::Reported);
        }
    };
    let binary = profile_binary_path();
    let data_dir = profile_data_dir();
    if data_dir.exists() {
        fs::remove_dir_all(&data_dir).map_err(|error| {
            format!(
                "failed to clear profiling data '{}': {error}",
                data_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&data_dir).map_err(|error| {
        format!(
            "failed to create profiling data '{}': {error}",
            data_dir.display()
        )
    })?;
    build_native_instrumented(
        &generated,
        &binary,
        BuildMode::Profile,
        NativeInstrumentation::Gprof,
    )?;

    eprintln!("profile: running instrumented native binary");
    let run_status = Command::new(&binary)
        .env("GMON_OUT_PREFIX", data_dir.join("gmon"))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| {
            format!(
                "failed to launch profiled binary '{}': {error}",
                binary.display()
            )
        });

    let data_files = profile_data_files(&data_dir)?;
    let report_status = if data_files.is_empty() {
        Err(CliError::Message(
            "profiled process produced no gprof data; the process may have terminated before profiling data could be flushed"
                .to_string(),
        ))
    } else {
        eprintln!("profile: CPU report");
        let mut command = Command::new("gprof");
        command.args(["-b", "-l"]).arg(&binary);
        for data_file in &data_files {
            command.arg(data_file);
        }
        let output = command
            .output()
            .map_err(|error| format!("failed to launch gprof: {error}"))?;
        if output.status.success() {
            let report = String::from_utf8_lossy(&output.stdout);
            let report = symbolize_profile_report(&binary, &report);
            print!("{report}");
            io::stdout()
                .flush()
                .map_err(|error| format!("failed to flush profiler report: {error}"))?;
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(CliError::Message(format!(
                "gprof exited with status {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_string(), |code| code.to_string()),
                stderr.trim()
            )))
        }
    };

    let _ = fs::remove_file(&binary);
    let _ = fs::remove_dir_all(&data_dir);
    let run_status = run_status?;
    report_status?;
    if run_status.success() {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "profiled program exited with status {}",
            run_status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string())
        )))
    }
}

fn profile_binary_path() -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    env::temp_dir().join(format!("flux-profile-{}{suffix}", std::process::id()))
}

fn profile_data_dir() -> PathBuf {
    env::temp_dir().join(format!("flux-profile-data-{}", std::process::id()))
}

fn profile_data_files(directory: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut files = fs::read_dir(directory)
        .map_err(|error| {
            format!(
                "failed to read profiling data '{}': {error}",
                directory.display()
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == "gmon.out" || name.starts_with("gmon."))
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn symbolize_profile_report(binary: &Path, report: &str) -> String {
    let addresses = profile_report_addresses(report);
    let locations = resolve_profile_addresses(binary, &addresses);
    let mut out = report.to_string();
    for (address, location) in addresses.iter().zip(locations) {
        let Some(location) = location else {
            continue;
        };
        let marker = format!(" @ {address}");
        let mut search_start = 0usize;
        while let Some(relative) = out[search_start..].find(&marker) {
            let marker_start = search_start + relative;
            let Some(open_start) = out[..marker_start].rfind('(') else {
                search_start = marker_start + marker.len();
                continue;
            };
            let location_start = open_start + 1;
            if out[location_start..marker_start].contains('\n') {
                search_start = marker_start + marker.len();
                continue;
            }
            out.replace_range(location_start..marker_start, &location);
            search_start = location_start + location.len() + marker.len();
        }
    }
    demangle_profile_symbols(&out)
}

fn profile_report_addresses(report: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    let mut rest = report;
    while let Some(index) = rest.find(" @ ") {
        rest = &rest[index + 3..];
        let address = rest
            .chars()
            .take_while(|ch| ch.is_ascii_hexdigit())
            .collect::<String>();
        if !address.is_empty() && !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    addresses
}

fn resolve_profile_addresses(binary: &Path, addresses: &[String]) -> Vec<Option<String>> {
    if addresses.is_empty() {
        return Vec::new();
    }
    let mut command = Command::new("addr2line");
    command.arg("-e").arg(binary);
    for address in addresses {
        command.arg(format!("0x{address}"));
    }
    let Ok(output) = command.output() else {
        return vec![None; addresses.len()];
    };
    if !output.status.success() {
        return vec![None; addresses.len()];
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut locations = text
        .lines()
        .map(|line| {
            let location = line.trim();
            (!location.is_empty()
                && location != "??:0"
                && location != "??:?"
                && location.contains(".flux:"))
            .then(|| location.to_string())
        })
        .collect::<Vec<_>>();
    locations.resize(addresses.len(), None);
    locations.truncate(addresses.len());
    locations
}

fn demangle_profile_symbols(report: &str) -> String {
    let mut out = String::with_capacity(report.len());
    let mut rest = report;
    while let Some(index) = rest.find("flux__fn_") {
        out.push_str(&rest[..index]);
        rest = &rest[index + "flux__fn_".len()..];
        let name_len = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .map(char::len_utf8)
            .sum::<usize>();
        if name_len == 0 {
            out.push_str("flux__fn_");
            continue;
        }
        out.push_str(&rest[..name_len]);
        rest = &rest[name_len..];
    }
    out.push_str(rest);
    out
}

fn run_development(target: &Path, mode: BuildMode) -> Result<(), CliError> {
    let mut generation = 0usize;
    let mut analysis_cache = fluxc::project::ProjectAnalysisCache::default();
    write_development_status(
        target,
        "starting",
        generation,
        mode,
        "building initial process",
    );
    let (mut child, mut binary, mut watch_paths) =
        match start_development_build(target, generation, mode, &mut analysis_cache) {
            Ok(started) => started,
            Err(error) => {
                write_development_status(
                    target,
                    "compile_error",
                    generation,
                    mode,
                    "initial build failed",
                );
                return Err(error);
            }
        };
    let mut fingerprints = watch_fingerprints(&watch_paths);
    write_development_status(
        target,
        "running",
        generation,
        mode,
        "development process started",
    );
    let mut status_state = "running";
    eprintln!(
        "run: started ({}); watching {} source file{}",
        mode.name(),
        watch_paths.len(),
        if watch_paths.len() == 1 { "" } else { "s" }
    );

    loop {
        if child
            .as_mut()
            .is_some_and(|process| process.try_wait().ok().flatten().is_some())
        {
            child = None;
            if status_state != "compile_error" {
                write_development_status(
                    target,
                    "exited",
                    generation,
                    mode,
                    "application exited; waiting for source changes",
                );
                status_state = "exited";
            }
            eprintln!("run: app exited; waiting for source changes");
        }
        thread::sleep(Duration::from_millis(75));
        let current = watch_fingerprints(&watch_paths);
        if current == fingerprints {
            continue;
        }

        fingerprints = debounce_changes(&watch_paths, current);
        write_development_status(
            target,
            "compiling",
            generation + 1,
            mode,
            "source change detected; recompiling",
        );
        eprintln!("reload: source change detected; recompiling");
        let analysis = match analysis_cache.analyze_with_overlays(target, &HashMap::new()) {
            Ok(analysis) => analysis,
            Err(diagnostics) => {
                let (_, sources) = fluxc::project::check_with_sources(target);
                report_diagnostics(target, &diagnostics, &sources);
                watch_paths = merge_watch_paths(target, &watch_paths, &sources);
                fingerprints = watch_fingerprints(&watch_paths);
                write_development_status(
                    target,
                    "compile_error",
                    generation,
                    mode,
                    "compile failed; keeping the last good process",
                );
                status_state = "compile_error";
                eprintln!("reload: compile failed; keeping the last good process");
                continue;
            }
        };
        let sources = analysis.sources.clone();
        let generated = match analysis.emit_c() {
            Ok(generated) => generated,
            Err(diagnostic) => {
                report_diagnostics(target, &[diagnostic], &sources);
                watch_paths = merge_watch_paths(target, &watch_paths, &sources);
                fingerprints = watch_fingerprints(&watch_paths);
                write_development_status(
                    target,
                    "compile_error",
                    generation,
                    mode,
                    "compile failed; keeping the last good process",
                );
                status_state = "compile_error";
                eprintln!("reload: compile failed; keeping the last good process");
                continue;
            }
        };
        generation += 1;
        let next_binary = development_binary_path(generation);
        if let Err(message) = build_native(&generated, &next_binary, mode) {
            write_development_status(target, "compile_error", generation, mode, &message);
            status_state = "compile_error";
            eprintln!("reload: {message}");
            let _ = fs::remove_file(&next_binary);
            continue;
        }

        stop_child(&mut child);
        let previous_binary = std::mem::replace(&mut binary, next_binary);
        child = Some(spawn_development_binary(&binary)?);
        let _ = fs::remove_file(previous_binary);
        watch_paths = project_watch_paths(target, &sources);
        fingerprints = watch_fingerprints(&watch_paths);
        write_development_status(
            target,
            "restarted",
            generation,
            mode,
            "rebuilt and restarted after source change",
        );
        status_state = "restarted";
        eprintln!("reload: rebuilt and restarted after source change");
    }
}

fn write_development_status(
    target: &Path,
    state: &str,
    generation: usize,
    mode: BuildMode,
    message: &str,
) {
    let Ok(path) = fluxc::project::development_status_path(target) else {
        return;
    };
    let updated_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let payload = format!(
        "{{\"version\":1,\"state\":{},\"generation\":{generation},\"mode\":{},\"runner_pid\":{},\"updated_unix_ms\":{updated_unix_ms},\"message\":{}}}\n",
        json_string(state),
        json_string(mode.name()),
        std::process::id(),
        json_string(message),
    );
    let temp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("fluxc-run-status"),
        std::process::id()
    ));
    if fs::write(&temp, payload).is_err() {
        let _ = fs::remove_file(&temp);
        return;
    }
    if fs::rename(&temp, &path).is_err() {
        let _ = fs::remove_file(&path);
        let _ = fs::rename(&temp, &path);
    }
    let _ = fs::remove_file(&temp);
}

fn json_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 2);
    output.push('"');
    for ch in input.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch < ' ' => output.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

fn start_development_build(
    target: &Path,
    generation: usize,
    mode: BuildMode,
    analysis_cache: &mut fluxc::project::ProjectAnalysisCache,
) -> Result<(Option<Child>, PathBuf, Vec<PathBuf>), CliError> {
    let analysis = match analysis_cache.analyze_with_overlays(target, &HashMap::new()) {
        Ok(analysis) => analysis,
        Err(diagnostics) => {
            let (_, sources) = fluxc::project::check_with_sources(target);
            report_diagnostics(target, &diagnostics, &sources);
            return Err(CliError::Reported);
        }
    };
    let sources = analysis.sources.clone();
    let generated = match analysis.emit_c() {
        Ok(generated) => generated,
        Err(diagnostic) => {
            report_diagnostics(target, &[diagnostic], &sources);
            return Err(CliError::Reported);
        }
    };
    let binary = development_binary_path(generation);
    build_native(&generated, &binary, mode)?;
    let child = spawn_development_binary(&binary)?;
    let watch_paths = project_watch_paths(target, &sources);
    Ok((Some(child), binary, watch_paths))
}

fn spawn_development_binary(path: &Path) -> Result<Child, CliError> {
    Command::new(path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| {
            CliError::Message(format!(
                "failed to launch development binary '{}': {error}",
                path.display()
            ))
        })
}

fn stop_child(child: &mut Option<Child>) {
    let Some(mut process) = child.take() else {
        return;
    };
    if process.try_wait().ok().flatten().is_none() {
        let _ = process.kill();
    }
    let _ = process.wait();
}

fn development_binary_path(generation: usize) -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    env::temp_dir().join(format!(
        "fluxc-run-{}-{generation}{suffix}",
        std::process::id()
    ))
}

fn project_watch_paths(target: &Path, sources: &[fluxc::project::ProjectSource]) -> Vec<PathBuf> {
    let mut paths = sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<HashSet<_>>();
    let manifest = if target.is_dir() {
        Some(target.join("flux.toml"))
    } else if target.file_name().and_then(|name| name.to_str()) == Some("flux.toml") {
        Some(target.to_path_buf())
    } else {
        None
    };
    if let Some(manifest) = manifest {
        paths.insert(fs::canonicalize(&manifest).unwrap_or(manifest));
    }
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    paths
}

fn merge_watch_paths(
    target: &Path,
    previous: &[PathBuf],
    sources: &[fluxc::project::ProjectSource],
) -> Vec<PathBuf> {
    let mut paths = previous.iter().cloned().collect::<HashSet<_>>();
    paths.extend(project_watch_paths(target, sources));
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    paths
}

fn watch_fingerprints(paths: &[PathBuf]) -> HashMap<PathBuf, Option<u64>> {
    paths
        .iter()
        .map(|path| (path.clone(), file_fingerprint(path)))
        .collect()
}

fn debounce_changes(
    paths: &[PathBuf],
    mut latest: HashMap<PathBuf, Option<u64>>,
) -> HashMap<PathBuf, Option<u64>> {
    let mut stable_since = Instant::now();
    loop {
        thread::sleep(Duration::from_millis(40));
        let current = watch_fingerprints(paths);
        if current != latest {
            latest = current;
            stable_since = Instant::now();
            continue;
        }
        if stable_since.elapsed() >= Duration::from_millis(120) {
            return latest;
        }
    }
}

fn file_fingerprint(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Some(hash)
}

fn validate_project(path: &Path) -> Result<Vec<fluxc::project::ProjectSource>, CliError> {
    let (diagnostics, sources) = fluxc::project::check_with_sources(path);
    if diagnostics.is_empty() {
        return Ok(sources);
    }
    report_diagnostics(path, &diagnostics, &sources);
    Err(CliError::Reported)
}

fn report_diagnostics(
    target: &Path,
    diagnostics: &[Diagnostic],
    project_sources: &[fluxc::project::ProjectSource],
) {
    let mut sources = project_sources
        .iter()
        .map(|source| {
            DiagnosticSource::new(
                source.source_id,
                source.path.display().to_string(),
                source.text.clone(),
            )
        })
        .collect::<Vec<_>>();
    add_target_source_context(target, &mut sources);
    report_diagnostic_context(diagnostics, &sources);
}

fn report_diagnostic_context(diagnostics: &[Diagnostic], sources: &[DiagnosticSource]) {
    let rendered = fluxc::render_diagnostics(
        diagnostics,
        sources,
        TerminalRenderOptions {
            width: terminal_width(),
            color: terminal_color_enabled(),
        },
    );
    eprint!("{rendered}");
}

fn add_target_source_context(target: &Path, sources: &mut Vec<DiagnosticSource>) {
    let candidate = if target.is_dir() {
        target.join("flux.toml")
    } else {
        target.to_path_buf()
    };
    let Ok(canonical) = fs::canonicalize(&candidate) else {
        return;
    };
    let source_id = fluxc::SourceId::from_name(canonical.to_string_lossy().as_ref());
    if sources.iter().any(|source| source.source_id == source_id) {
        return;
    }
    let Ok(text) = fs::read_to_string(&canonical) else {
        return;
    };
    sources.push(DiagnosticSource::new(
        source_id,
        canonical.display().to_string(),
        text,
    ));
}

fn canonical_source_id(path: &Path) -> fluxc::SourceId {
    fs::canonicalize(path)
        .ok()
        .map(|path| fluxc::SourceId::from_name(path.to_string_lossy().as_ref()))
        .unwrap_or(fluxc::SourceId::UNKNOWN)
}

fn terminal_width() -> usize {
    if let Ok(columns) = env::var("COLUMNS")
        && let Ok(width) = columns.parse::<usize>()
        && width >= 24
    {
        return width.clamp(24, 240);
    }
    if io::stderr().is_terminal()
        && let Ok(output) = Command::new("tput").arg("cols").output()
        && output.status.success()
        && let Ok(text) = std::str::from_utf8(&output.stdout)
        && let Ok(width) = text.trim().parse::<usize>()
    {
        return width.clamp(24, 240);
    }
    100
}

fn terminal_color_enabled() -> bool {
    if env::var_os("NO_COLOR").is_some() || env::var("TERM").is_ok_and(|term| term == "dumb") {
        return false;
    }
    if env::var("FORCE_COLOR").is_ok_and(|value| value != "0" && !value.is_empty())
        || env::var("CLICOLOR_FORCE").is_ok_and(|value| value != "0" && !value.is_empty())
    {
        return true;
    }
    io::stderr().is_terminal()
}

fn analysis_json_mode(command: &str, args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--json" => Ok(true),
        _ => Err(format!(
            "{command} syntax is '{command} <file.flux|package-dir|flux.toml> [--json]'"
        )),
    }
}

fn waydroid_running() -> Result<bool, String> {
    let output = output_with_timeout(
        Command::new("waydroid").arg("status"),
        Duration::from_secs(3),
    )
    .map_err(|error| format!("Waydroid unavailable ({error})"))?
    .ok_or_else(|| "Waydroid status timed out after 3 seconds".to_string())?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(waydroid_status_is_running(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn waydroid_status_is_running(output: &str) -> bool {
    let session_running = output.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(name, value)| name.trim() == "Session" && value.trim() == "RUNNING")
    });
    let container_running = output.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(name, value)| name.trim() == "Container" && value.trim() == "RUNNING")
    });
    session_running && container_running
}

fn run_devices() -> Result<(), CliError> {
    println!("Flux devices");
    if cfg!(target_os = "linux") {
        let (display, status) = if let Some(display) = env::var_os("WAYLAND_DISPLAY") {
            (format!("Wayland ({})", display.to_string_lossy()), "ready")
        } else if let Some(display) = env::var_os("DISPLAY") {
            (format!("X11 ({})", display.to_string_lossy()), "ready")
        } else {
            ("no active graphical display".to_string(), "unavailable")
        };
        println!("  linux-desktop  {status:11} GTK4 · {display}");
    }

    let waydroid = waydroid_running().unwrap_or(false);
    match adb_devices() {
        Ok(devices) if devices.is_empty() && waydroid => {
            println!("  android        ready       waydroid · local container");
        }
        Ok(devices) if devices.is_empty() => {
            println!("  android        unavailable no connected adb device");
        }
        Ok(devices) => {
            for device in devices {
                println!(
                    "  android        {:11} {}{}",
                    device.status,
                    device.serial,
                    if device.description.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", device.description)
                    }
                );
            }
            if waydroid {
                println!("  android        ready       waydroid · local container");
            }
        }
        Err(message) if waydroid => {
            println!("  android        ready       waydroid · local container ({message})");
        }
        Err(message) => println!("  android        unavailable {message}"),
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct AdbDevice {
    serial: String,
    status: String,
    description: String,
}

fn adb_devices() -> Result<Vec<AdbDevice>, String> {
    let output = output_with_timeout(
        Command::new(adb_path()).args(["devices", "-l"]),
        Duration::from_secs(3),
    )
    .map_err(|error| format!("adb unavailable ({error})"))?
    .ok_or_else(|| "adb device discovery timed out after 3 seconds".to_string())?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("adb devices failed")
            .trim()
            .to_string();
        return Err(detail);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_adb_devices(&stdout))
}

fn parse_adb_devices(output: &str) -> Vec<AdbDevice> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            let status = fields.next()?;
            let description = fields.collect::<Vec<_>>().join(" ");
            Some(AdbDevice {
                serial: serial.to_string(),
                status: status.to_string(),
                description,
            })
        })
        .collect()
}

fn adb_path() -> PathBuf {
    env::var_os("FLUX_ADB")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("adb"))
}

fn android_abi_from_runtime(value: &str) -> Option<AndroidAbi> {
    match value.trim() {
        "arm64-v8a" => Some(AndroidAbi::Arm64V8a),
        "x86_64" => Some(AndroidAbi::X86_64),
        "armeabi-v7a" | "armeabi" => Some(AndroidAbi::ArmeabiV7a),
        _ => None,
    }
}

fn adb_device_abi(serial: &str) -> Option<AndroidAbi> {
    let output = output_with_timeout(
        Command::new(adb_path()).args(["-s", serial, "shell", "getprop", "ro.product.cpu.abi"]),
        Duration::from_secs(3),
    )
    .ok()??;
    output
        .status
        .success()
        .then(|| android_abi_from_runtime(&String::from_utf8_lossy(&output.stdout)))
        .flatten()
}

fn waydroid_runtime_abi() -> Option<AndroidAbi> {
    let output = output_with_timeout(
        Command::new("waydroid").args(["prop", "get", "ro.product.cpu.abi"]),
        Duration::from_secs(3),
    )
    .ok()??;
    output
        .status
        .success()
        .then(|| android_abi_from_runtime(&String::from_utf8_lossy(&output.stdout)))
        .flatten()
}

fn detect_android_run_abi(requested_device: Option<&str>) -> Option<AndroidAbi> {
    if let Some(requested) = requested_device {
        if requested.eq_ignore_ascii_case("waydroid") {
            return waydroid_running()
                .ok()
                .filter(|running| *running)
                .and_then(|_| waydroid_runtime_abi());
        }
        return adb_devices()
            .ok()?
            .into_iter()
            .find(|device| device.status == "device" && device.serial == requested)
            .and_then(|device| adb_device_abi(&device.serial));
    }

    let ready = adb_devices()
        .unwrap_or_default()
        .into_iter()
        .filter(|device| device.status == "device")
        .collect::<Vec<_>>();
    if let [device] = ready.as_slice()
        && let Some(abi) = adb_device_abi(&device.serial)
    {
        return Some(abi);
    }
    if waydroid_running().unwrap_or(false)
        && let Some(abi) = waydroid_runtime_abi()
    {
        return Some(abi);
    }
    None
}

fn clean_target(target: &Path) -> Result<(), CliError> {
    let entry = fluxc::project::resolve_entry(target).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let binary = default_binary_path(&entry);
    let status = fluxc::project::development_status_path(target).ok();
    let mut removed = Vec::new();
    for path in std::iter::once(binary).chain(status) {
        if !path.exists() {
            continue;
        }
        fs::remove_file(&path)
            .map_err(|error| format!("failed to remove '{}': {error}", path.display()))?;
        removed.push(path);
    }
    if removed.is_empty() {
        println!("clean: nothing to remove for {}", target.display());
    } else {
        for path in removed {
            println!("removed: {}", path.display());
        }
    }
    Ok(())
}

fn run_doctor() -> Result<(), CliError> {
    println!("Flux doctor");
    let mut required_ok = true;

    if cfg!(target_os = "linux") {
        println!("  [ok] host: Linux");
    } else {
        println!("  [fail] host: current bootstrap GUI target requires Linux");
        required_ok = false;
    }

    match command_first_line("clang", &["--version"]) {
        Ok(version) => println!("  [ok] clang: {version}"),
        Err(message) => {
            println!("  [fail] clang: {message}");
            required_ok = false;
        }
    }

    match command_first_line("clang", &["-print-prog-name=ld"]) {
        Ok(linker) => match command_first_line(&linker, &["--version"]) {
            Ok(version) => println!("  [ok] linker: {linker} ({version})"),
            Err(message) => {
                println!("  [fail] linker: {linker} ({message})");
                required_ok = false;
            }
        },
        Err(message) => {
            println!("  [fail] linker: {message}");
            required_ok = false;
        }
    }

    match command_first_line("pkg-config", &["--modversion", "gtk4"]) {
        Ok(version) => println!("  [ok] GTK4: {version}"),
        Err(message) => {
            println!("  [fail] GTK4: {message}");
            required_ok = false;
        }
    }

    match find_android_sdk() {
        Ok(sdk) => {
            println!("  [ok] Android SDK: {}", sdk.display());
            match find_android_ndk(&sdk) {
                Ok(ndk) => println!("  [ok] Android NDK: {}", ndk.display()),
                Err(CliError::Message(message)) => println!("  [warn] Android NDK: {message}"),
                Err(CliError::Reported) => unreachable!(),
            }
            match find_android_build_tools(&sdk) {
                Ok(tools) => println!(
                    "  [ok] Android build-tools (aapt2/zipalign/apksigner/d8): {}",
                    tools.display()
                ),
                Err(CliError::Message(message)) => {
                    println!("  [warn] Android build-tools: {message}")
                }
                Err(CliError::Reported) => unreachable!(),
            }
            match command_first_line("javac", &["-version"]) {
                Ok(version) => println!("  [ok] javac (generated Android runtime glue): {version}"),
                Err(message) => println!("  [warn] javac: {message}"),
            }
        }
        Err(CliError::Message(message)) => println!("  [warn] Android SDK: {message}"),
        Err(CliError::Reported) => unreachable!(),
    }
    match command_first_line(adb_path().to_string_lossy().as_ref(), &["version"]) {
        Ok(version) => println!("  [ok] adb: {version}"),
        Err(message) => println!("  [warn] adb: {message}"),
    }
    if waydroid_running().unwrap_or(false) {
        println!("  [ok] Waydroid: session and container running");
    } else {
        println!("  [info] Waydroid: not running (optional Android device transport)");
    }

    match command_first_line("nvim", &["--version"]) {
        Ok(version) => println!("  [ok] Neovim editor dogfood: {version}"),
        Err(message) => println!("  [warn] Neovim editor dogfood unavailable: {message}"),
    }
    match command_first_line("gdb", &["--version"]) {
        Ok(version) => println!("  [ok] GDB debugger: {version}"),
        Err(message) => println!("  [warn] GDB debugger unavailable: {message}"),
    }
    match command_first_line("gprof", &["--version"]) {
        Ok(version) => println!("  [ok] gprof CPU profiler: {version}"),
        Err(message) => println!("  [warn] gprof CPU profiler unavailable: {message}"),
    }
    match command_first_line("addr2line", &["--version"]) {
        Ok(version) => println!("  [ok] addr2line crash symbolizer: {version}"),
        Err(message) => println!("  [warn] addr2line crash symbolizer unavailable: {message}"),
    }
    match command_first_line("objcopy", &["--version"]) {
        Ok(version) => println!("  [ok] objcopy debug-symbol tool: {version}"),
        Err(message) => println!("  [warn] objcopy debug-symbol tool unavailable: {message}"),
    }
    match command_first_line("tar", &["--version"]) {
        Ok(version) => println!("  [ok] tar package archive tool: {version}"),
        Err(message) => println!("  [warn] tar package archive tool unavailable: {message}"),
    }
    match command_first_line("llvm-cov", &["--version"]) {
        Ok(version) => println!("  [ok] LLVM coverage reporter: {version}"),
        Err(message) => println!("  [warn] LLVM coverage reporter unavailable: {message}"),
    }
    match command_first_line("llvm-profdata", &["--version"]) {
        Ok(version) => println!("  [ok] LLVM coverage profile merger: {version}"),
        Err(message) => println!("  [warn] LLVM coverage profile merger unavailable: {message}"),
    }

    if let Some(display) = env::var_os("WAYLAND_DISPLAY") {
        println!("  [ok] display: Wayland ({})", display.to_string_lossy());
    } else if let Some(display) = env::var_os("DISPLAY") {
        println!("  [ok] display: X11 ({})", display.to_string_lossy());
    } else {
        println!(
            "  [warn] display: no active Wayland/X11 session detected; GUI builds still work but cannot be launched here"
        );
    }

    if required_ok {
        println!("ready: native Flux Linux build prerequisites are available");
        Ok(())
    } else {
        Err(CliError::Message(
            "doctor found missing prerequisites for native Linux Flux apps".to_string(),
        ))
    }
}

fn command_first_line(command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|error| format!("not available ({error})"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().next().unwrap_or("command failed").trim();
        return Err(detail.to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .next()
        .unwrap_or("available")
        .trim()
        .to_string())
}

fn require_target(args: &[String]) -> Result<&Path, String> {
    if args.len() < 2 {
        return Err(usage());
    }
    Ok(Path::new(&args[1]))
}

fn require_source(args: &[String]) -> Result<&Path, String> {
    require_target(args)
}

fn output_path(args: &[String]) -> Result<Option<PathBuf>, String> {
    if args.is_empty() {
        return Ok(None);
    }
    if args.len() == 2 && args[0] == "-o" {
        return Ok(Some(PathBuf::from(&args[1])));
    }
    Err("output syntax is '-o <path>'".to_string())
}

fn android_build_options(
    args: &[String],
    default_mode: BuildMode,
) -> Result<AndroidBuildOptions, String> {
    let Some(target) = args.first() else {
        return Err(
            "Android build syntax is 'build android <package-dir|flux.toml> [-o artifact] [--mode debug|profile|release] [--abi arm64-v8a|x86_64|armeabi-v7a] [--format apk|aab]'"
                .to_string(),
        );
    };
    let mut output = None;
    let mut mode = default_mode;
    let mut mode_seen = false;
    let mut abi = AndroidAbi::Arm64V8a;
    let mut abi_seen = false;
    let mut device = None;
    let mut kind = AndroidArtifactKind::Apk;
    let mut format_seen = false;
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "-o" => {
                if output.is_some() {
                    return Err("output path may only be specified once".to_string());
                }
                let Some(path) = args.get(index + 1) else {
                    return Err("'-o' requires an output path".to_string());
                };
                output = Some(PathBuf::from(path));
                index += 2;
            }
            "--mode" => {
                if mode_seen {
                    return Err("build mode may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--mode' requires debug, profile, or release".to_string());
                };
                mode = BuildMode::parse(value)?;
                mode_seen = true;
                index += 2;
            }
            "--abi" => {
                if abi_seen {
                    return Err("Android ABI may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--abi' requires an Android ABI".to_string());
                };
                abi = AndroidAbi::parse(value)?;
                abi_seen = true;
                index += 2;
            }
            "--device" => {
                if device.is_some() {
                    return Err("Android device may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--device' requires an adb serial or 'waydroid'".to_string());
                };
                if value.is_empty() {
                    return Err("'--device' cannot be empty".to_string());
                }
                device = Some(value.clone());
                index += 2;
            }
            "--format" => {
                if format_seen {
                    return Err("Android artifact format may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--format' requires apk or aab".to_string());
                };
                kind = AndroidArtifactKind::parse(value)?;
                format_seen = true;
                index += 2;
            }
            flag => {
                return Err(format!(
                    "unknown Android option '{flag}'; expected '-o', '--mode', '--abi', '--device', or '--format'"
                ));
            }
        }
    }
    Ok(AndroidBuildOptions {
        target: PathBuf::from(target),
        output,
        mode,
        abi,
        abi_explicit: abi_seen,
        device,
        kind,
    })
}

fn android_publish_options(args: &[String]) -> Result<AndroidPublishOptions, String> {
    let Some(target) = args.first() else {
        return Err(
            "Android publish syntax is 'publish android <package-dir|flux.toml> [-o artifact.aab] [--json]'"
                .to_string(),
        );
    };
    let mut output = None;
    let mut json = false;
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "-o" => {
                if output.is_some() {
                    return Err("output path may only be specified once".to_string());
                }
                let Some(path) = args.get(index + 1) else {
                    return Err("'-o' requires an output path".to_string());
                };
                if Path::new(path).extension().and_then(|value| value.to_str()) != Some("aab") {
                    return Err("Android publishing output must end in '.aab'".to_string());
                }
                output = Some(PathBuf::from(path));
                index += 2;
            }
            "--json" => {
                if json {
                    return Err("'--json' may only be specified once".to_string());
                }
                json = true;
                index += 1;
            }
            flag => {
                return Err(format!(
                    "unknown Android publish option '{flag}'; expected '-o <artifact.aab>' or '--json'"
                ));
            }
        }
    }
    Ok(AndroidPublishOptions {
        target: PathBuf::from(target),
        output,
        json,
    })
}

fn test_options(args: &[String]) -> Result<TestOptions, String> {
    let mut mode = BuildMode::Debug;
    let mut mode_seen = false;
    let mut coverage = false;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--mode" => {
                if mode_seen {
                    return Err("test mode may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--mode' requires debug, profile, or release".to_string());
                };
                mode = BuildMode::parse(value)?;
                mode_seen = true;
                index += 2;
            }
            "--coverage" => {
                if coverage {
                    return Err("'--coverage' may only be specified once".to_string());
                }
                coverage = true;
                index += 1;
            }
            flag => {
                return Err(format!(
                    "unknown test option '{flag}'; expected '--mode <debug|profile|release>' or '--coverage'"
                ));
            }
        }
    }
    Ok(TestOptions { mode, coverage })
}

fn package_options(args: &[String]) -> Result<PackageOptions, String> {
    let mut output = None;
    let mut mode = BuildMode::Release;
    let mut mode_seen = false;
    let mut format = PackageFormat::Directory;
    let mut format_seen = false;
    let mut native_target = NativeTargetOptions::default();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "-o" => {
                if output.is_some() {
                    return Err("output path may only be specified once".to_string());
                }
                let Some(path) = args.get(index + 1) else {
                    return Err("'-o' requires an output path".to_string());
                };
                output = Some(PathBuf::from(path));
                index += 2;
            }
            "--mode" => {
                if mode_seen {
                    return Err("build mode may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--mode' requires debug, profile, or release".to_string());
                };
                mode = BuildMode::parse(value)?;
                mode_seen = true;
                index += 2;
            }
            "--format" => {
                if format_seen {
                    return Err("package format may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err(
                        "'--format' requires directory, tar.gz, container, or systemd".to_string(),
                    );
                };
                format = PackageFormat::parse(value)?;
                format_seen = true;
                index += 2;
            }
            "--target" => {
                if native_target.triple.is_some() {
                    return Err("native target may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--target' requires a Clang target triple".to_string());
                };
                native_target.triple = Some(parse_native_target_triple(value)?);
                index += 2;
            }
            "--sysroot" => {
                if native_target.sysroot.is_some() {
                    return Err("native sysroot may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--sysroot' requires a directory".to_string());
                };
                native_target.sysroot = Some(PathBuf::from(value));
                index += 2;
            }
            flag => {
                return Err(format!(
                    "unknown package option '{flag}'; expected '-o <path>', '--mode <debug|profile|release>', '--format <directory|tar.gz|container|systemd>', '--target <triple>', or '--sysroot <directory>'"
                ));
            }
        }
    }
    Ok(PackageOptions {
        output,
        mode,
        format,
        native_target,
    })
}

fn parse_native_target_triple(value: &str) -> Result<String, String> {
    if value.is_empty()
        || !value.contains('-')
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(format!(
            "invalid native target '{value}'; expected a Clang target triple such as x86_64-unknown-linux-gnu"
        ));
    }
    Ok(value.to_string())
}

fn build_options(args: &[String], default_mode: BuildMode) -> Result<BuildOptions, String> {
    let mut output = None;
    let mut mode = default_mode;
    let mut mode_seen = false;
    let mut native_target = NativeTargetOptions::default();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "-o" => {
                if output.is_some() {
                    return Err("output path may only be specified once".to_string());
                }
                let Some(path) = args.get(index + 1) else {
                    return Err("'-o' requires an output path".to_string());
                };
                output = Some(PathBuf::from(path));
                index += 2;
            }
            "--mode" => {
                if mode_seen {
                    return Err("build mode may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--mode' requires debug, profile, or release".to_string());
                };
                mode = BuildMode::parse(value)?;
                mode_seen = true;
                index += 2;
            }
            "--target" => {
                if native_target.triple.is_some() {
                    return Err("native target may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--target' requires a Clang target triple".to_string());
                };
                native_target.triple = Some(parse_native_target_triple(value)?);
                index += 2;
            }
            "--sysroot" => {
                if native_target.sysroot.is_some() {
                    return Err("native sysroot may only be specified once".to_string());
                }
                let Some(value) = args.get(index + 1) else {
                    return Err("'--sysroot' requires a directory".to_string());
                };
                native_target.sysroot = Some(PathBuf::from(value));
                index += 2;
            }
            flag => {
                return Err(format!(
                    "unknown build option '{flag}'; expected '-o <path>', '--mode <debug|profile|release>', '--target <triple>', or '--sysroot <directory>'"
                ));
            }
        }
    }
    Ok(BuildOptions {
        output,
        mode,
        native_target,
    })
}

#[derive(Debug)]
struct AndroidToolchain {
    ndk: PathBuf,
    build_tools: PathBuf,
    android_jar: PathBuf,
}

fn android_toolchain(
    manifest: &fluxc::project::PackageManifest,
) -> Result<AndroidToolchain, CliError> {
    let sdk = find_android_sdk()?;
    let ndk = find_android_ndk(&sdk)?;
    let build_tools = find_android_build_tools(&sdk)?;
    let android_jar = sdk
        .join("platforms")
        .join(format!("android-{}", manifest.android.target_sdk))
        .join("android.jar");
    if !android_jar.is_file() {
        return Err(CliError::Message(format!(
            "Android platform {} is not installed under '{}'",
            manifest.android.target_sdk,
            sdk.display()
        )));
    }
    Ok(AndroidToolchain {
        ndk,
        build_tools,
        android_jar,
    })
}

fn android_staging_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("flux-android-{}-{nonce}", std::process::id()))
}

fn compile_android_native_library(
    c_source: &str,
    manifest: &fluxc::project::PackageManifest,
    mode: BuildMode,
    abi: AndroidAbi,
    ndk: &Path,
    staging: &Path,
) -> Result<PathBuf, CliError> {
    let clang = ndk
        .join("toolchains/llvm/prebuilt/linux-x86_64/bin")
        .join(abi.clang_name(manifest.android.min_sdk));
    if !clang.is_file() {
        return Err(CliError::Message(format!(
            "Android NDK compiler '{}' is unavailable; check min_sdk and NDK installation",
            clang.display()
        )));
    }
    let lib_dir = staging.join("lib").join(abi.name());
    fs::create_dir_all(&lib_dir)
        .map_err(|error| format!("failed to create Android staging directory: {error}"))?;
    let source = staging.join(format!("app-{}.c", abi.name()));
    fs::write(&source, c_source)
        .map_err(|error| format!("failed to write generated Android C: {error}"))?;
    let native_library = lib_dir.join("libflux.so");
    let mut clang_command = Command::new(&clang);
    clang_command
        .args(["-std=c17", "-fwrapv", "-shared", "-fPIC"])
        .args(mode.clang_args())
        .arg(&source)
        .args(["-landroid", "-llog", "-Wl,-soname,libflux.so", "-o"])
        .arg(&native_library);
    let native = clang_command
        .output()
        .map_err(|error| format!("failed to launch Android NDK clang: {error}"))?;
    if !native.status.success() {
        return Err(CliError::Message(format!(
            "Android native backend failed for {}:\n{}",
            abi.name(),
            String::from_utf8_lossy(&native.stderr)
        )));
    }
    Ok(native_library)
}

fn android_has_generated_activity(c_source: &str) -> bool {
    c_source.contains("Java_app_flux_runtime_FluxActivity_nativeBuildUi")
}

fn android_activity_java_source() -> &'static str {
    r#"package app.flux.runtime;

import android.app.Activity;
import android.app.UiModeManager;
import android.content.Context;
import android.content.res.ColorStateList;
import android.content.res.Configuration;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.ColorFilter;
import android.graphics.DashPathEffect;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputFilter;
import android.text.TextUtils;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.CompoundButton;
import android.widget.EditText;
import android.widget.ImageView;
import android.widget.TextView;
import java.io.File;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;

public final class FluxActivity extends Activity implements View.OnClickListener, CompoundButton.OnCheckedChangeListener, View.OnFocusChangeListener, View.OnHoverListener, View.OnLongClickListener, View.OnKeyListener {
    private static final String FLUX_STATE_KEY = "app.flux.runtime.savedState";

    static {
        System.loadLibrary("flux");
    }

    private final Map<Integer, String> textValues = new HashMap<>();
    private final Map<Integer, Integer> selectionStarts = new HashMap<>();
    private final Map<Integer, Integer> selectionEnds = new HashMap<>();
    private final Map<Integer, Integer> composingStarts = new HashMap<>();
    private final Map<Integer, Integer> composingEnds = new HashMap<>();
    private boolean restoringInput;
    private boolean restoringFocus;
    private boolean restoringCheckedState;
    private int fluxThemeMode;
    private float fluxContrast;
    private native int nativeThemeMode();
    private native String nativeThemeColor(String token);
    private native void nativeCreate(String restoredState);
    private native void nativeBuildUi();
    private native void nativeStart();
    private native void nativeResume();
    private native void nativePause();
    private native void nativeStop();
    private native void nativeLowMemory();
    private native void nativeConfigurationChanged();
    private native String nativeSaveState();
    private native void nativeDestroy();
    private static native void nativeOnClick(int viewId);
    private static native void nativeOnTap(int viewId);
    private static native void nativeOnLongPress(int viewId);
    private static native void nativeOnChecked(int viewId, boolean checked);
    private static native void nativeOnFocus(int viewId, boolean focused);
    private static native void nativeOnHover(int viewId, boolean hovered);
    private static native void nativeOnKey(int viewId, String key);
    private static native void nativeOnTextChanged(int viewId, String text);
    private static native void nativeOnSubmit(int viewId, String text);

    private void applyFluxTheme() {
        boolean dark = fluxThemeMode == 2;
        if (fluxThemeMode == 0) {
            int nightMode = getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK;
            dark = nightMode == Configuration.UI_MODE_NIGHT_YES;
        }
        setTheme(dark
                ? android.R.style.Theme_DeviceDefault_NoActionBar
                : android.R.style.Theme_DeviceDefault_Light_NoActionBar);
    }

    private float readFluxContrast() {
        if (Build.VERSION.SDK_INT < 34) return 0.0f;
        UiModeManager manager = (UiModeManager) getSystemService(UI_MODE_SERVICE);
        return manager == null ? 0.0f : manager.getContrast();
    }

    private boolean isFluxHighContrast() {
        return fluxContrast > 0.0f;
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        fluxThemeMode = nativeThemeMode();
        applyFluxTheme();
        super.onCreate(savedInstanceState);
        fluxContrast = readFluxContrast();
        String restoredState = savedInstanceState == null ? null : savedInstanceState.getString(FLUX_STATE_KEY);
        nativeCreate(restoredState);
        nativeBuildUi();
    }

    @Override
    protected void onStart() {
        super.onStart();
        nativeStart();
    }

    @Override
    protected void onResume() {
        super.onResume();
        float nextContrast = readFluxContrast();
        if (Float.compare(nextContrast, fluxContrast) != 0) {
            fluxContrast = nextContrast;
            nativeBuildUi();
        }
        nativeResume();
    }

    @Override
    protected void onPause() {
        nativePause();
        super.onPause();
    }

    @Override
    protected void onStop() {
        nativeStop();
        super.onStop();
    }

    @Override
    public void onLowMemory() {
        nativeLowMemory();
        super.onLowMemory();
    }

    @Override
    public void onConfigurationChanged(Configuration configuration) {
        super.onConfigurationChanged(configuration);
        if (fluxThemeMode == 0) applyFluxTheme();
        nativeConfigurationChanged();
        nativeBuildUi();
    }

    @Override
    protected void onSaveInstanceState(Bundle state) {
        String savedState = nativeSaveState();
        if (savedState != null) state.putString(FLUX_STATE_KEY, savedState);
        super.onSaveInstanceState(state);
    }

    @Override
    protected void onDestroy() {
        nativeDestroy();
        super.onDestroy();
    }

    @Override
    public void onClick(View view) {
        int viewId = view.getId();
        nativeOnClick(viewId);
        nativeOnTap(viewId);
    }

    @Override
    public boolean onLongClick(View view) {
        nativeOnLongPress(view.getId());
        return true;
    }

    @Override
    public void onCheckedChanged(CompoundButton button, boolean checked) {
        if (!restoringCheckedState) nativeOnChecked(button.getId(), checked);
    }

    @Override
    public void setContentView(View view) {
        View current = getCurrentFocus();
        int previousFocusId = current == null ? View.NO_ID : current.getId();
        restoringFocus = true;
        try {
            super.setContentView(view);
            if (previousFocusId != View.NO_ID) {
                View replacement = view.findViewById(previousFocusId);
                if (replacement != null && replacement.isFocusable()) replacement.requestFocus();
            }
        } finally {
            restoringFocus = false;
        }
    }

    @Override
    public void onFocusChange(View view, boolean focused) {
        if (!restoringFocus) nativeOnFocus(view.getId(), focused);
    }

    @Override
    public boolean onHover(View view, MotionEvent event) {
        if (event.getActionMasked() == MotionEvent.ACTION_HOVER_ENTER) nativeOnHover(view.getId(), true);
        else if (event.getActionMasked() == MotionEvent.ACTION_HOVER_EXIT) nativeOnHover(view.getId(), false);
        return false;
    }

    private static String fluxKeyName(int keyCode, KeyEvent event) {
        switch (keyCode) {
            case KeyEvent.KEYCODE_ENTER:
            case KeyEvent.KEYCODE_NUMPAD_ENTER: return "Enter";
            case KeyEvent.KEYCODE_ESCAPE: return "Escape";
            case KeyEvent.KEYCODE_TAB: return "Tab";
            case KeyEvent.KEYCODE_DEL: return "Backspace";
            case KeyEvent.KEYCODE_FORWARD_DEL: return "Delete";
            case KeyEvent.KEYCODE_DPAD_LEFT: return "ArrowLeft";
            case KeyEvent.KEYCODE_DPAD_RIGHT: return "ArrowRight";
            case KeyEvent.KEYCODE_DPAD_UP: return "ArrowUp";
            case KeyEvent.KEYCODE_DPAD_DOWN: return "ArrowDown";
            case KeyEvent.KEYCODE_MOVE_HOME: return "Home";
            case KeyEvent.KEYCODE_MOVE_END: return "End";
            case KeyEvent.KEYCODE_PAGE_UP: return "PageUp";
            case KeyEvent.KEYCODE_PAGE_DOWN: return "PageDown";
            default:
                int codePoint = event.getUnicodeChar();
                if (codePoint > 0 && !Character.isISOControl(codePoint)) {
                    return new String(Character.toChars(codePoint));
                }
                String fallback = KeyEvent.keyCodeToString(keyCode);
                return fallback.startsWith("KEYCODE_") ? fallback.substring(8) : fallback;
        }
    }

    @Override
    public boolean onKey(View view, int keyCode, KeyEvent event) {
        if (event.getAction() == KeyEvent.ACTION_DOWN) nativeOnKey(view.getId(), fluxKeyName(keyCode, event));
        return false;
    }

    public static final class FluxEditText extends EditText {
        public FluxEditText(Context context) {
            super(context);
        }

        @Override
        protected void onSelectionChanged(int start, int end) {
            super.onSelectionChanged(start, end);
            Context context = getContext();
            if (context instanceof FluxActivity) {
                ((FluxActivity)context).rememberSelection(getId(), start, end);
            }
        }
    }

    private void rememberSelection(int viewId, int start, int end) {
        if (restoringInput || viewId == View.NO_ID) return;
        selectionStarts.put(viewId, start);
        selectionEnds.put(viewId, end);
    }

    public void restoreTextInput(EditText view, int viewId, String fallback) {
        String saved = textValues.get(viewId);
        String value = saved == null ? fallback : saved;
        restoringInput = true;
        try {
            view.setId(viewId);
            view.setText(value);
            int length = view.getText().length();
            int start = Math.max(0, Math.min(length, selectionStarts.getOrDefault(viewId, length)));
            int end = Math.max(start, Math.min(length, selectionEnds.getOrDefault(viewId, start)));
            view.setSelection(start, end);
            Integer composingStart = composingStarts.get(viewId);
            Integer composingEnd = composingEnds.get(viewId);
            if (composingStart != null && composingEnd != null
                    && composingStart >= 0 && composingEnd > composingStart && composingEnd <= length) {
                new BaseInputConnection(view, true).setComposingRegion(composingStart, composingEnd);
            }
        } finally {
            restoringInput = false;
        }
    }

    public void setMaxLength(EditText view, int maxLength) {
        view.setFilters(new InputFilter[] { new InputFilter.LengthFilter(maxLength) });
    }

    public void setCheckedSilently(CompoundButton button, boolean checked) {
        restoringCheckedState = true;
        try {
            button.setChecked(checked);
        } finally {
            restoringCheckedState = false;
        }
    }

    private boolean isFluxDarkTheme() {
        if (fluxThemeMode == 2) return true;
        if (fluxThemeMode == 1) return false;
        int nightMode = getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK;
        return nightMode == Configuration.UI_MODE_NIGHT_YES;
    }

    private int parseFluxColor(String value) {
        String custom = nativeThemeColor(value);
        if (custom != null) value = custom;
        boolean dark = isFluxDarkTheme();
        boolean highContrast = custom == null && isFluxHighContrast();
        switch (value) {
            case "surface": return highContrast ? (dark ? 0xFF000000 : 0xFFFFFFFF) : (dark ? 0xFF0F172A : 0xFFF8FAFC);
            case "surfaceRaised": return highContrast ? (dark ? 0xFF111111 : 0xFFFFFFFF) : (dark ? 0xFF1E293B : 0xFFFFFFFF);
            case "text": return highContrast ? (dark ? 0xFFFFFFFF : 0xFF000000) : (dark ? 0xFFF8FAFC : 0xFF0F172A);
            case "textMuted": return highContrast ? (dark ? 0xFFE2E8F0 : 0xFF334155) : (dark ? 0xFF94A3B8 : 0xFF64748B);
            case "accent": return highContrast ? (dark ? 0xFFA5B4FC : 0xFF3730A3) : (dark ? 0xFF818CF8 : 0xFF4F46E5);
            case "onAccent": return highContrast ? (dark ? 0xFF000000 : 0xFFFFFFFF) : (dark ? 0xFF0F172A : 0xFFFFFFFF);
            case "outline": return highContrast ? (dark ? 0xFFE2E8F0 : 0xFF334155) : (dark ? 0xFF475569 : 0xFFCBD5E1);
            case "danger": return highContrast ? (dark ? 0xFFFCA5A5 : 0xFF991B1B) : (dark ? 0xFFF87171 : 0xFFDC2626);
            case "success": return highContrast ? (dark ? 0xFF86EFAC : 0xFF166534) : (dark ? 0xFF4ADE80 : 0xFF16A34A);
            case "warning": return highContrast ? (dark ? 0xFFFDE047 : 0xFF92400E) : (dark ? 0xFFFBBF24 : 0xFFD97706);
            case "shadow": return highContrast ? (dark ? 0x99000000 : 0x66000000) : (dark ? 0x66000000 : 0x33000000);
            default:
                if (value.length() == 9 && value.charAt(0) == '#') {
                    value = String.valueOf('#') + value.substring(7, 9) + value.substring(1, 7);
                }
                return Color.parseColor(value);
        }
    }

    private final class FluxStyleDrawable extends Drawable {
        private final Integer background;
        private final Integer borderTop;
        private final Integer borderEnd;
        private final Integer borderBottom;
        private final Integer borderStart;
        private final int borderTopWidth;
        private final int borderEndWidth;
        private final int borderBottomWidth;
        private final int borderStartWidth;
        private final float[] radii;
        private final String borderStyle;
        private final Integer shadowColor;
        private final float shadowBlur;
        private final float shadowOffsetX;
        private final float shadowOffsetY;
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private int alpha = 255;
        private ColorFilter colorFilter;

        FluxStyleDrawable(
                String background,
                String borderTop,
                String borderEnd,
                String borderBottom,
                String borderStart,
                int borderTopWidth,
                int borderEndWidth,
                int borderBottomWidth,
                int borderStartWidth,
                float topLeft,
                float topRight,
                float bottomRight,
                float bottomLeft,
                String borderStyle,
                String shadowColor,
                float shadowBlur,
                float shadowOffsetX,
                float shadowOffsetY) {
            this.background = background == null ? null : parseFluxColor(background);
            this.borderTop = borderTop == null ? null : parseFluxColor(borderTop);
            this.borderEnd = borderEnd == null ? null : parseFluxColor(borderEnd);
            this.borderBottom = borderBottom == null ? null : parseFluxColor(borderBottom);
            this.borderStart = borderStart == null ? null : parseFluxColor(borderStart);
            this.borderTopWidth = Math.max(0, borderTopWidth);
            this.borderEndWidth = Math.max(0, borderEndWidth);
            this.borderBottomWidth = Math.max(0, borderBottomWidth);
            this.borderStartWidth = Math.max(0, borderStartWidth);
            this.radii = new float[] {
                Math.max(0.0f, topLeft), Math.max(0.0f, topLeft),
                Math.max(0.0f, topRight), Math.max(0.0f, topRight),
                Math.max(0.0f, bottomRight), Math.max(0.0f, bottomRight),
                Math.max(0.0f, bottomLeft), Math.max(0.0f, bottomLeft)
            };
            this.borderStyle = borderStyle == null ? "solid" : borderStyle;
            this.shadowColor = shadowColor == null ? null : parseFluxColor(shadowColor);
            this.shadowBlur = Math.max(0.0f, shadowBlur);
            this.shadowOffsetX = shadowOffsetX;
            this.shadowOffsetY = shadowOffsetY;
        }

        private void preparePaint(int color, Paint.Style style) {
            paint.reset();
            paint.setAntiAlias(true);
            paint.setStyle(style);
            paint.setColor(color);
            paint.setAlpha(alpha);
            paint.setColorFilter(colorFilter);
        }

        private void prepareBorderPaint(int color, float width) {
            preparePaint(color, Paint.Style.STROKE);
            paint.setStrokeWidth(width);
            paint.setStrokeCap(Paint.Cap.BUTT);
            if ("dashed".equals(borderStyle)) {
                paint.setPathEffect(new DashPathEffect(new float[] {
                    Math.max(width * 3.0f, 1.0f), Math.max(width * 2.0f, 1.0f)
                }, 0.0f));
            } else if ("dotted".equals(borderStyle)) {
                paint.setStrokeCap(Paint.Cap.ROUND);
                paint.setPathEffect(new DashPathEffect(new float[] {
                    Math.max(width * 0.1f, 0.1f), Math.max(width * 2.0f, 1.0f)
                }, 0.0f));
            }
        }

        private void drawHorizontalBorder(Canvas canvas, RectF rect, boolean top, int color, int width) {
            if (width <= 0 || "none".equals(borderStyle)) return;
            if ("double".equals(borderStyle) && width >= 3) {
                float line = Math.max(1.0f, width / 3.0f);
                prepareBorderPaint(color, line);
                float first = top ? rect.top + line / 2.0f : rect.bottom - line / 2.0f;
                float second = top ? rect.top + width - line / 2.0f : rect.bottom - width + line / 2.0f;
                canvas.drawLine(rect.left, first, rect.right, first, paint);
                canvas.drawLine(rect.left, second, rect.right, second, paint);
                return;
            }
            prepareBorderPaint(color, width);
            float y = top ? rect.top + width / 2.0f : rect.bottom - width / 2.0f;
            canvas.drawLine(rect.left, y, rect.right, y, paint);
        }

        private void drawVerticalBorder(Canvas canvas, RectF rect, boolean start, int color, int width) {
            if (width <= 0 || "none".equals(borderStyle)) return;
            if ("double".equals(borderStyle) && width >= 3) {
                float line = Math.max(1.0f, width / 3.0f);
                prepareBorderPaint(color, line);
                float first = start ? rect.left + line / 2.0f : rect.right - line / 2.0f;
                float second = start ? rect.left + width - line / 2.0f : rect.right - width + line / 2.0f;
                canvas.drawLine(first, rect.top, first, rect.bottom, paint);
                canvas.drawLine(second, rect.top, second, rect.bottom, paint);
                return;
            }
            prepareBorderPaint(color, width);
            float x = start ? rect.left + width / 2.0f : rect.right - width / 2.0f;
            canvas.drawLine(x, rect.top, x, rect.bottom, paint);
        }

        @Override
        public void draw(Canvas canvas) {
            RectF rect = new RectF(getBounds());
            if (rect.isEmpty()) return;
            Path shape = new Path();
            shape.addRoundRect(rect, radii, Path.Direction.CW);
            if (shadowColor != null && (shadowBlur > 0.0f || shadowOffsetX != 0.0f || shadowOffsetY != 0.0f)) {
                preparePaint(background == null ? Color.argb(1, 0, 0, 0) : background, Paint.Style.FILL);
                paint.setShadowLayer(shadowBlur, shadowOffsetX, shadowOffsetY, shadowColor);
                canvas.drawPath(shape, paint);
                paint.clearShadowLayer();
            }
            if (background != null) {
                preparePaint(background, Paint.Style.FILL);
                canvas.drawPath(shape, paint);
            }
            int save = canvas.save();
            canvas.clipPath(shape);
            if (borderTop != null) drawHorizontalBorder(canvas, rect, true, borderTop, borderTopWidth);
            if (borderEnd != null) drawVerticalBorder(canvas, rect, false, borderEnd, borderEndWidth);
            if (borderBottom != null) drawHorizontalBorder(canvas, rect, false, borderBottom, borderBottomWidth);
            if (borderStart != null) drawVerticalBorder(canvas, rect, true, borderStart, borderStartWidth);
            canvas.restoreToCount(save);
        }

        @Override
        public void setAlpha(int alpha) {
            this.alpha = Math.max(0, Math.min(255, alpha));
            invalidateSelf();
        }

        @Override
        public void setColorFilter(ColorFilter colorFilter) {
            this.colorFilter = colorFilter;
            invalidateSelf();
        }

        @Override
        public int getOpacity() {
            return PixelFormat.TRANSLUCENT;
        }
    }

    public void styleView(
            View view,
            String background,
            String borderTop,
            String borderEnd,
            String borderBottom,
            String borderStart,
            int borderTopWidth,
            int borderEndWidth,
            int borderBottomWidth,
            int borderStartWidth,
            float topLeft,
            float topRight,
            float bottomRight,
            float bottomLeft,
            String borderStyle,
            String shadowColor,
            float shadowBlur,
            float shadowOffsetX,
            float shadowOffsetY) {
        if (background == null
                && borderTop == null && borderEnd == null && borderBottom == null && borderStart == null
                && topLeft <= 0 && topRight <= 0 && bottomRight <= 0 && bottomLeft <= 0
                && shadowColor == null) return;
        FluxStyleDrawable drawable = new FluxStyleDrawable(
                background, borderTop, borderEnd, borderBottom, borderStart,
                borderTopWidth, borderEndWidth, borderBottomWidth, borderStartWidth,
                topLeft, topRight, bottomRight, bottomLeft, borderStyle,
                shadowColor, shadowBlur, shadowOffsetX, shadowOffsetY);
        if (shadowColor != null && (shadowBlur > 0.0f || shadowOffsetX != 0.0f || shadowOffsetY != 0.0f)) {
            view.setLayerType(View.LAYER_TYPE_SOFTWARE, null);
        }
        view.setBackgroundTintList(null);
        view.setBackground(drawable);
    }


    private static int withAlpha(int color, int alpha) {
        return Color.argb(alpha, Color.red(color), Color.green(color), Color.blue(color));
    }

    public void styleRoot(View view) {
        view.setBackgroundColor(parseFluxColor("surface"));
    }

    private void applyFluxTextLocales(TextView view) {
        if (Build.VERSION.SDK_INT >= 24) {
            view.setTextLocales(getResources().getConfiguration().getLocales());
            if (view instanceof EditText) {
                ((EditText)view).setImeHintLocales(getResources().getConfiguration().getLocales());
            }
        } else {
            view.setTextLocale(Locale.getDefault());
        }
    }

    public void styleButton(Button view, boolean primary) {
        applyFluxTextLocales(view);
        view.setAllCaps(false);
        int background = parseFluxColor(primary ? "accent" : "surfaceRaised");
        int label = parseFluxColor(primary ? "onAccent" : "text");
        int[][] states = new int[][] {
            new int[] { -android.R.attr.state_enabled },
            new int[] { android.R.attr.state_pressed },
            new int[] {}
        };
        int pressed = isFluxDarkTheme()
                ? Color.rgb(Math.min(255, Color.red(background) + 16), Math.min(255, Color.green(background) + 16), Math.min(255, Color.blue(background) + 16))
                : Color.rgb(Math.max(0, Color.red(background) - 16), Math.max(0, Color.green(background) - 16), Math.max(0, Color.blue(background) - 16));
        view.setBackgroundTintList(new ColorStateList(states, new int[] { withAlpha(background, 96), pressed, background }));
        view.setTextColor(new ColorStateList(states, new int[] { withAlpha(label, 144), label, label }));
        view.setStateListAnimator(null);
        view.setElevation(0.0f);
    }

    public void styleTextInput(EditText view, String validationState) {
        applyFluxTextLocales(view);
        int accent = parseFluxColor("accent");
        int outline = parseFluxColor("outline");
        int semantic = outline;
        boolean hasValidation = true;
        if ("error".equals(validationState)) semantic = parseFluxColor("danger");
        else if ("success".equals(validationState)) semantic = parseFluxColor("success");
        else if ("warning".equals(validationState)) semantic = parseFluxColor("warning");
        else hasValidation = false;
        int focused = hasValidation ? semantic : accent;
        int resting = hasValidation ? semantic : outline;
        int[][] states = new int[][] {
            new int[] { -android.R.attr.state_enabled },
            new int[] { android.R.attr.state_focused },
            new int[] {}
        };
        view.setBackgroundTintList(new ColorStateList(states, new int[] { withAlpha(resting, 96), focused, resting }));
        view.setTextColor(parseFluxColor("text"));
        view.setHintTextColor(parseFluxColor("textMuted"));
    }

    public void styleCheckable(CompoundButton view) {
        applyFluxTextLocales(view);
        int accent = parseFluxColor("accent");
        int outline = parseFluxColor("outline");
        int[][] states = new int[][] {
            new int[] { -android.R.attr.state_enabled },
            new int[] { android.R.attr.state_checked },
            new int[] {}
        };
        view.setButtonTintList(new ColorStateList(states, new int[] { withAlpha(outline, 96), accent, outline }));
        view.setTextColor(parseFluxColor("text"));
    }

    public void styleText(TextView view, String color, float size, boolean bold, boolean italic, boolean underline, boolean strike) {
        applyFluxTextLocales(view);
        if (color != null) view.setTextColor(parseFluxColor(color));
        if (size > 0) view.setTextSize(size);
        int typefaceStyle = (bold ? Typeface.BOLD : 0) | (italic ? Typeface.ITALIC : 0);
        view.setTypeface(view.getTypeface(), typefaceStyle);
        int flags = view.getPaintFlags();
        if (underline) flags |= Paint.UNDERLINE_TEXT_FLAG;
        if (strike) flags |= Paint.STRIKE_THRU_TEXT_FLAG;
        view.setPaintFlags(flags);
    }

    public void styleTextLayout(TextView view, String fontFamily, int letterSpacing, int lineHeightPercent, String align, String wrapMode, String ellipsize, int maxLines, int maxWidthChars) {
        if (fontFamily != null && !fontFamily.isEmpty()) {
            int style = view.getTypeface() == null ? Typeface.NORMAL : view.getTypeface().getStyle();
            view.setTypeface(Typeface.create(fontFamily, style));
        }
        if (letterSpacing != Integer.MIN_VALUE) {
            float density = getResources().getDisplayMetrics().density;
            float textSize = Math.max(view.getTextSize(), 1.0f);
            view.setLetterSpacing((letterSpacing * density) / textSize);
        }
        if (lineHeightPercent > 0) view.setLineSpacing(0.0f, lineHeightPercent / 100.0f);
        if (align != null) {
            int vertical = view.getGravity() & Gravity.VERTICAL_GRAVITY_MASK;
            int horizontal = Gravity.LEFT;
            if ("center".equals(align)) horizontal = Gravity.CENTER_HORIZONTAL;
            else if ("right".equals(align)) horizontal = Gravity.RIGHT;
            else if ("fill".equals(align)) horizontal = Gravity.FILL_HORIZONTAL;
            view.setGravity(vertical | horizontal);
        }
        if (wrapMode != null && Build.VERSION.SDK_INT >= 23) {
            if ("char".equals(wrapMode)) view.setBreakStrategy(android.text.Layout.BREAK_STRATEGY_SIMPLE);
            else view.setBreakStrategy(android.text.Layout.BREAK_STRATEGY_HIGH_QUALITY);
        }
        if (ellipsize != null) {
            if ("none".equals(ellipsize)) view.setEllipsize(null);
            else if ("start".equals(ellipsize)) view.setEllipsize(TextUtils.TruncateAt.START);
            else if ("middle".equals(ellipsize)) view.setEllipsize(TextUtils.TruncateAt.MIDDLE);
            else if ("end".equals(ellipsize)) view.setEllipsize(TextUtils.TruncateAt.END);
        }
        if (maxLines > 0) view.setMaxLines(maxLines);
        if (maxWidthChars > 0) view.setMaxEms(maxWidthChars);
    }

    public void configureImage(ImageView view, String source, String fit, String alt, boolean canShrink) {
        view.setAdjustViewBounds(canShrink);
        if ("fill".equals(fit)) view.setScaleType(ImageView.ScaleType.FIT_XY);
        else if ("cover".equals(fit)) view.setScaleType(ImageView.ScaleType.CENTER_CROP);
        else if ("scaleDown".equals(fit) || "scale_down".equals(fit)) view.setScaleType(ImageView.ScaleType.CENTER_INSIDE);
        else view.setScaleType(ImageView.ScaleType.FIT_CENTER);
        view.setContentDescription(alt);
        if (source == null || source.isEmpty()) {
            view.setImageDrawable(null);
            return;
        }
        Uri uri = source.contains("://") ? Uri.parse(source) : Uri.fromFile(new File(source));
        view.setImageURI(uri);
    }

    public void transformView(View view, float translateX, float translateY, float rotation, float scaleX, float scaleY, float originXPercent, float originYPercent) {
        view.setTranslationX(translateX);
        view.setTranslationY(translateY);
        view.setRotation(rotation);
        view.setScaleX(scaleX);
        view.setScaleY(scaleY);
        view.post(() -> {
            view.setPivotX(view.getWidth() * originXPercent / 100.0f);
            view.setPivotY(view.getHeight() * originYPercent / 100.0f);
        });
    }

    public void setTooltip(View view, String text) {
        if (Build.VERSION.SDK_INT >= 26) view.setTooltipText(text);
    }

    public void setAccessibility(View view, String label, String description) {
        if (label == null || label.isEmpty()) {
            view.setContentDescription(description);
        } else if (description == null || description.isEmpty()) {
            view.setContentDescription(label);
        } else {
            view.setContentDescription(label + ". " + description);
        }
    }

    public void setAccessibilityRole(View view, String role) {
        if (role == null) return;
        if (Build.VERSION.SDK_INT >= 28) view.setAccessibilityHeading("heading".equals(role));
        final String className;
        if ("button".equals(role)) className = "android.widget.Button";
        else if ("textBox".equals(role)) className = "android.widget.EditText";
        else if ("checkbox".equals(role)) className = "android.widget.CheckBox";
        else if ("radio".equals(role)) className = "android.widget.RadioButton";
        else if ("image".equals(role)) className = "android.widget.ImageView";
        else if ("switch".equals(role)) className = "android.widget.Switch";
        else className = "android.widget.TextView";
        view.setAccessibilityDelegate(new View.AccessibilityDelegate() {
            @Override public void onInitializeAccessibilityNodeInfo(View host, AccessibilityNodeInfo info) {
                super.onInitializeAccessibilityNodeInfo(host, info);
                info.setClassName(className);
                if (Build.VERSION.SDK_INT >= 28) info.setHeading("heading".equals(role));
            }
        });
    }

    public void setAccessibilityHidden(View view, boolean hidden) {
        view.setImportantForAccessibility(hidden ? View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS : View.IMPORTANT_FOR_ACCESSIBILITY_AUTO);
    }

    public void wireTextInput(EditText view, boolean onChange, boolean onSubmit, boolean multiline, boolean submitOnEnter) {
        final int viewId = view.getId();
        view.addTextChangedListener(new TextWatcher() {
            @Override public void beforeTextChanged(CharSequence s, int start, int count, int after) {}
            @Override public void onTextChanged(CharSequence s, int start, int before, int count) {
                String text = s.toString();
                textValues.put(viewId, text);
                if (onChange) nativeOnTextChanged(viewId, text);
            }
            @Override public void afterTextChanged(Editable s) {
                rememberSelection(viewId, view.getSelectionStart(), view.getSelectionEnd());
                int composingStart = BaseInputConnection.getComposingSpanStart(s);
                int composingEnd = BaseInputConnection.getComposingSpanEnd(s);
                if (composingStart >= 0 && composingEnd > composingStart) {
                    composingStarts.put(viewId, composingStart);
                    composingEnds.put(viewId, composingEnd);
                } else {
                    composingStarts.remove(viewId);
                    composingEnds.remove(viewId);
                }
            }
        });
        if (onSubmit && submitOnEnter) {
            view.setImeOptions(EditorInfo.IME_ACTION_DONE);
            view.setOnEditorActionListener((editor, actionId, event) -> {
                boolean enter = event != null
                    && event.getKeyCode() == KeyEvent.KEYCODE_ENTER
                    && event.getAction() == KeyEvent.ACTION_DOWN;
                if (actionId == EditorInfo.IME_ACTION_DONE || enter) {
                    nativeOnSubmit(viewId, editor.getText().toString());
                    return true;
                }
                return false;
            });
        } else if (multiline) {
            view.setImeOptions(EditorInfo.IME_FLAG_NO_ENTER_ACTION);
        }
    }
}
"#
}

fn compile_android_activity_dex(
    c_source: &str,
    manifest: &fluxc::project::PackageManifest,
    toolchain: &AndroidToolchain,
    staging: &Path,
    dex_output: &Path,
) -> Result<(), CliError> {
    if !android_has_generated_activity(c_source) {
        return Ok(());
    }
    let d8 = toolchain.build_tools.join("d8");
    if !d8.is_file() {
        return Err(CliError::Message(format!(
            "Android build-tools '{}' do not include d8, which is required for compiler-generated native UI glue",
            toolchain.build_tools.display()
        )));
    }
    let java_dir = staging.join("java/app/flux/runtime");
    let classes_dir = staging.join("java-classes");
    fs::create_dir_all(&java_dir)
        .map_err(|error| format!("failed to create generated Android Java directory: {error}"))?;
    fs::create_dir_all(&classes_dir)
        .map_err(|error| format!("failed to create generated Android class directory: {error}"))?;
    fs::create_dir_all(dex_output)
        .map_err(|error| format!("failed to create generated Android dex directory: {error}"))?;
    let java_source = java_dir.join("FluxActivity.java");
    fs::write(&java_source, android_activity_java_source())
        .map_err(|error| format!("failed to write compiler-generated Android activity: {error}"))?;
    run_checked(
        Command::new("javac")
            .args(["-source", "8", "-target", "8", "-classpath"])
            .arg(&toolchain.android_jar)
            .arg("-d")
            .arg(&classes_dir)
            .arg(&java_source),
        "javac compiler-generated Android activity",
    )?;
    let runtime_classes = classes_dir.join("app/flux/runtime");
    let mut generated_classes = fs::read_dir(&runtime_classes)
        .map_err(|error| {
            format!(
                "failed to read compiler-generated Android classes '{}': {error}",
                runtime_classes.display()
            )
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "class")
        })
        .collect::<Vec<_>>();
    generated_classes.sort();
    let activity_class = runtime_classes.join("FluxActivity.class");
    if !generated_classes.iter().any(|path| path == &activity_class) {
        return Err(CliError::Message(format!(
            "javac did not produce the compiler-generated Android activity class '{}'",
            activity_class.display()
        )));
    }
    let mut d8_command = Command::new(&d8);
    d8_command
        .arg("--min-api")
        .arg(manifest.android.min_sdk.to_string())
        .arg("--output")
        .arg(dex_output);
    d8_command.args(&generated_classes);
    run_checked(&mut d8_command, "d8 compiler-generated Android activity")?;
    Ok(())
}

fn ensure_parent_directory(output: &Path) -> Result<(), CliError> {
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create Android output directory: {error}"))?;
    }
    Ok(())
}

fn build_android_aab(
    c_source: &str,
    manifest: &fluxc::project::PackageManifest,
    output: &Path,
    mode: BuildMode,
) -> Result<(), CliError> {
    let toolchain = android_toolchain(manifest)?;
    let bundletool = find_android_bundletool()?;
    let staging = android_staging_dir();
    let base = staging.join("base");
    fs::create_dir_all(base.join("manifest")).map_err(|error| {
        format!("failed to create Android App Bundle staging directory: {error}")
    })?;

    for abi in [
        AndroidAbi::Arm64V8a,
        AndroidAbi::X86_64,
        AndroidAbi::ArmeabiV7a,
    ] {
        compile_android_native_library(c_source, manifest, mode, abi, &toolchain.ndk, &base)?;
        let _ = fs::remove_file(base.join(format!("app-{}.c", abi.name())));
    }
    compile_android_activity_dex(c_source, manifest, &toolchain, &staging, &base.join("dex"))?;

    let manifest_path = staging.join("AndroidManifest.xml");
    fs::write(
        &manifest_path,
        android_manifest_xml(manifest, mode, c_source),
    )
    .map_err(|error| format!("failed to write Android manifest: {error}"))?;
    let proto_apk = staging.join("manifest-proto.apk");
    run_checked(
        Command::new(toolchain.build_tools.join("aapt2"))
            .arg("link")
            .arg("--proto-format")
            .arg("-o")
            .arg(&proto_apk)
            .arg("-I")
            .arg(&toolchain.android_jar)
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--min-sdk-version")
            .arg(manifest.android.min_sdk.to_string())
            .arg("--target-sdk-version")
            .arg(manifest.android.target_sdk.to_string()),
        "aapt2 App Bundle manifest link",
    )?;

    let proto_contents = staging.join("proto-contents");
    run_checked(
        Command::new("unzip")
            .args(["-q"])
            .arg(&proto_apk)
            .arg("-d")
            .arg(&proto_contents),
        "unzip App Bundle manifest",
    )?;
    fs::copy(
        proto_contents.join("AndroidManifest.xml"),
        base.join("manifest/AndroidManifest.xml"),
    )
    .map_err(|error| format!("failed to stage protobuf Android manifest: {error}"))?;
    let resources = proto_contents.join("resources.pb");
    if resources.is_file() {
        fs::copy(&resources, base.join("resources.pb"))
            .map_err(|error| format!("failed to stage Android resource table: {error}"))?;
    }

    let base_zip = staging.join("base.zip");
    run_checked(
        Command::new("zip")
            .current_dir(&base)
            .args(["-q", "-r"])
            .arg(&base_zip)
            .arg("."),
        "zip Android App Bundle base module",
    )?;
    let unsigned = staging.join("unsigned.aab");
    run_checked(
        Command::new("java")
            .arg("-jar")
            .arg(&bundletool)
            .arg("build-bundle")
            .arg(format!("--modules={}", base_zip.display()))
            .arg(format!("--output={}", unsigned.display()))
            .arg("--overwrite"),
        "bundletool build-bundle",
    )?;

    ensure_parent_directory(output)?;
    fs::copy(&unsigned, output)
        .map_err(|error| format!("failed to write Android App Bundle: {error}"))?;
    let signing = android_signing_config(manifest, mode)?;
    let mut signer = Command::new("jarsigner");
    if signing.release {
        signer
            .arg("-storepass:env")
            .arg("FLUX_ANDROID_KEYSTORE_PASSWORD")
            .arg("-keypass:env")
            .arg("FLUX_ANDROID_KEY_PASSWORD")
            .env("FLUX_ANDROID_KEYSTORE_PASSWORD", &signing.store_password)
            .env("FLUX_ANDROID_KEY_PASSWORD", &signing.key_password);
    } else {
        signer.args(["-storepass", "android", "-keypass", "android"]);
    }
    signer
        .arg("-keystore")
        .arg(&signing.keystore)
        .arg(output)
        .arg(&signing.key_alias);
    run_checked(&mut signer, "jarsigner")?;
    run_checked(
        Command::new("jarsigner").arg("-verify").arg(output),
        "jarsigner verify",
    )?;
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

fn build_android_apk(
    c_source: &str,
    manifest: &fluxc::project::PackageManifest,
    output: &Path,
    mode: BuildMode,
    abi: AndroidAbi,
) -> Result<(), CliError> {
    let toolchain = android_toolchain(manifest)?;
    let staging = android_staging_dir();
    compile_android_native_library(c_source, manifest, mode, abi, &toolchain.ndk, &staging)?;
    let dex_dir = staging.join("dex");
    compile_android_activity_dex(c_source, manifest, &toolchain, &staging, &dex_dir)?;
    if android_has_generated_activity(c_source) {
        fs::copy(dex_dir.join("classes.dex"), staging.join("classes.dex"))
            .map_err(|error| format!("failed to stage compiler-generated Android dex: {error}"))?;
    }

    let manifest_xml = android_manifest_xml(manifest, mode, c_source);
    let manifest_path = staging.join("AndroidManifest.xml");
    fs::write(&manifest_path, manifest_xml)
        .map_err(|error| format!("failed to write Android manifest: {error}"))?;
    let unsigned = staging.join("unsigned.apk");
    let aapt2 = toolchain.build_tools.join("aapt2");
    run_checked(
        Command::new(&aapt2)
            .arg("link")
            .arg("-o")
            .arg(&unsigned)
            .arg("-I")
            .arg(&toolchain.android_jar)
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--min-sdk-version")
            .arg(manifest.android.min_sdk.to_string())
            .arg("--target-sdk-version")
            .arg(manifest.android.target_sdk.to_string()),
        "aapt2 link",
    )?;
    let mut zip = Command::new("zip");
    zip.current_dir(&staging)
        .args(["-q", "-r"])
        .arg(&unsigned)
        .arg("lib");
    if android_has_generated_activity(c_source) {
        zip.arg("classes.dex");
    }
    run_checked(&mut zip, "zip Android application payload")?;
    let aligned = staging.join("aligned.apk");
    run_checked(
        Command::new(toolchain.build_tools.join("zipalign"))
            .args(["-f", "4"])
            .arg(&unsigned)
            .arg(&aligned),
        "zipalign",
    )?;
    let signing = android_signing_config(manifest, mode)?;
    ensure_parent_directory(output)?;
    let mut signer = Command::new(toolchain.build_tools.join("apksigner"));
    signer
        .arg("sign")
        .arg("--ks")
        .arg(&signing.keystore)
        .arg("--ks-key-alias")
        .arg(&signing.key_alias);
    if signing.release {
        signer
            .args([
                "--ks-pass",
                "env:FLUX_ANDROID_KEYSTORE_PASSWORD",
                "--key-pass",
                "env:FLUX_ANDROID_KEY_PASSWORD",
            ])
            .env("FLUX_ANDROID_KEYSTORE_PASSWORD", &signing.store_password)
            .env("FLUX_ANDROID_KEY_PASSWORD", &signing.key_password);
    } else {
        signer.args(["--ks-pass", "pass:android", "--key-pass", "pass:android"]);
    }
    signer.arg("--out").arg(output).arg(&aligned);
    run_checked(&mut signer, "apksigner")?;
    run_checked(
        Command::new(toolchain.build_tools.join("apksigner"))
            .arg("verify")
            .arg(output),
        "apksigner verify",
    )?;
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

fn android_manifest_xml(
    manifest: &fluxc::project::PackageManifest,
    mode: BuildMode,
    c_source: &str,
) -> String {
    let application_id = xml_escape(&manifest.android.application_id);
    let label = xml_escape(&manifest.name);
    let version = xml_escape(manifest.version.as_deref().unwrap_or("0.0.0"));
    let mut permissions = manifest.android.permissions.clone();
    if c_source.contains("flux__android_vibrate(") {
        permissions.push("android.permission.VIBRATE".to_string());
    }
    if c_source.contains("flux__android_notify(")
        || c_source.contains("flux__android_notify_url_action(")
        || c_source.contains("flux__android_notification_permission_granted(")
        || c_source.contains("flux__android_request_notification_permission(")
    {
        permissions.push("android.permission.POST_NOTIFICATIONS".to_string());
    }
    permissions.sort();
    permissions.dedup();
    let permission_xml = permissions
        .iter()
        .map(|permission| {
            format!(
                "    <uses-permission android:name=\"{}\" />\n",
                xml_escape(permission)
            )
        })
        .collect::<String>();
    let generated_activity = android_has_generated_activity(c_source);
    let has_code = if generated_activity { "true" } else { "false" };
    let activity_name = if generated_activity {
        "app.flux.runtime.FluxActivity"
    } else {
        "android.app.NativeActivity"
    };
    let activity_config = if generated_activity {
        " android:configChanges=\"orientation|screenSize|smallestScreenSize|screenLayout|density|uiMode|fontScale|keyboard|keyboardHidden|navigation\""
    } else {
        ""
    };
    let native_activity_metadata = if generated_activity {
        ""
    } else {
        "            <meta-data android:name=\"android.app.lib_name\" android:value=\"flux\" />\n"
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\" package=\"{application_id}\" android:versionCode=\"{}\" android:versionName=\"{version}\">\n    <uses-sdk android:minSdkVersion=\"{}\" android:targetSdkVersion=\"{}\" />\n{permission_xml}    <application android:label=\"{label}\" android:hasCode=\"{has_code}\" android:extractNativeLibs=\"true\" android:debuggable=\"{}\">\n        <activity android:name=\"{activity_name}\" android:exported=\"true\"{activity_config}>\n{native_activity_metadata}            <intent-filter>\n                <action android:name=\"android.intent.action.MAIN\" />\n                <category android:name=\"android.intent.category.LAUNCHER\" />\n            </intent-filter>\n        </activity>\n    </application>\n</manifest>\n",
        manifest.android.version_code,
        manifest.android.min_sdk,
        manifest.android.target_sdk,
        if mode == BuildMode::Debug {
            "true"
        } else {
            "false"
        },
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn find_android_sdk() -> Result<PathBuf, CliError> {
    for variable in ["FLUX_ANDROID_SDK", "ANDROID_SDK_ROOT", "ANDROID_HOME"] {
        if let Some(path) = env::var_os(variable).map(PathBuf::from)
            && path.join("platforms").is_dir()
            && path.join("build-tools").is_dir()
        {
            return Ok(path);
        }
    }
    let fallback = PathBuf::from("/opt/android-sdk");
    if fallback.join("platforms").is_dir() && fallback.join("build-tools").is_dir() {
        return Ok(fallback);
    }
    Err(CliError::Message(
        "Android SDK not found; set FLUX_ANDROID_SDK or ANDROID_SDK_ROOT".to_string(),
    ))
}

fn find_android_ndk(sdk: &Path) -> Result<PathBuf, CliError> {
    for variable in ["FLUX_ANDROID_NDK", "ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"] {
        if let Some(path) = env::var_os(variable).map(PathBuf::from)
            && path
                .join("toolchains/llvm/prebuilt/linux-x86_64/bin")
                .is_dir()
        {
            return Ok(path);
        }
    }
    let mut roots = vec![sdk.join("ndk")];
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".config/android/ndk"));
    }
    for root in roots {
        if let Some(path) = latest_directory_matching(&root, |path| {
            path.join("toolchains/llvm/prebuilt/linux-x86_64/bin")
                .is_dir()
        }) {
            return Ok(path);
        }
    }
    Err(CliError::Message(
        "Android NDK not found; set FLUX_ANDROID_NDK or ANDROID_NDK_HOME".to_string(),
    ))
}

fn find_android_build_tools(sdk: &Path) -> Result<PathBuf, CliError> {
    latest_directory_matching(&sdk.join("build-tools"), |path| {
        path.join("aapt2").is_file()
            && path.join("zipalign").is_file()
            && path.join("apksigner").is_file()
            && path.join("d8").is_file()
    })
    .ok_or_else(|| {
        CliError::Message(
            "Android build-tools with aapt2/zipalign/apksigner/d8 were not found".to_string(),
        )
    })
}

fn find_android_bundletool() -> Result<PathBuf, CliError> {
    for variable in ["FLUX_ANDROID_BUNDLETOOL", "BUNDLETOOL_JAR"] {
        if let Some(path) = env::var_os(variable).map(PathBuf::from)
            && path.is_file()
        {
            return Ok(path);
        }
    }
    let mut roots = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("flux"));
    }
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/flux"));
    }
    for root in roots {
        if let Some(path) = latest_file_matching(&root, |path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("bundletool") && name.ends_with(".jar"))
        }) {
            return Ok(path);
        }
    }
    Err(CliError::Message(
        "Android AAB builds require bundletool; set FLUX_ANDROID_BUNDLETOOL to bundletool-all-<version>.jar"
            .to_string(),
    ))
}

fn latest_directory_matching(root: &Path, predicate: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut entries = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && predicate(path))
        .collect::<Vec<_>>();
    entries.sort();
    entries.pop()
}

fn latest_file_matching(root: &Path, predicate: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut entries = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && predicate(path))
        .collect::<Vec<_>>();
    entries.sort();
    entries.pop()
}

fn android_signing_config(
    manifest: &fluxc::project::PackageManifest,
    mode: BuildMode,
) -> Result<AndroidSigningConfig, CliError> {
    if mode == BuildMode::Release
        && let (Some(keystore), Some(key_alias)) = (
            manifest.android.keystore.as_ref(),
            manifest.android.key_alias.as_ref(),
        )
    {
        if !keystore.is_file() {
            return Err(CliError::Message(format!(
                "Android release keystore '{}' does not exist or is not a file",
                keystore.display()
            )));
        }
        let store_password = env::var_os("FLUX_ANDROID_KEYSTORE_PASSWORD").ok_or_else(|| {
            CliError::Message(
                "Android release signing requires FLUX_ANDROID_KEYSTORE_PASSWORD; signing credentials are not stored in flux.toml"
                    .to_string(),
            )
        })?;
        let key_password =
            env::var_os("FLUX_ANDROID_KEY_PASSWORD").unwrap_or_else(|| store_password.clone());
        return Ok(AndroidSigningConfig {
            keystore: keystore.clone(),
            key_alias: key_alias.clone(),
            store_password,
            key_password,
            release: true,
        });
    }

    Ok(AndroidSigningConfig {
        keystore: android_debug_keystore()?,
        key_alias: "flux".to_string(),
        store_password: std::ffi::OsString::from("android"),
        key_password: std::ffi::OsString::from("android"),
        release: false,
    })
}

fn android_debug_keystore() -> Result<PathBuf, CliError> {
    let root = native_build_cache_dir()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("android");
    fs::create_dir_all(&root)
        .map_err(|error| format!("failed to create Android signing cache: {error}"))?;
    let path = root.join("debug.keystore");
    if path.is_file() {
        return Ok(path);
    }
    run_checked(
        Command::new("keytool")
            .args([
                "-genkeypair",
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-alias",
                "flux",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-dname",
                "CN=Flux Development,O=Flux,C=NZ",
            ])
            .arg("-keystore")
            .arg(&path),
        "keytool",
    )?;
    Ok(path)
}

fn output_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> io::Result<Option<std::process::Output>> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map(Some);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_checked_with_timeout(
    command: &mut Command,
    label: &str,
    timeout: Duration,
) -> Result<(), CliError> {
    let output = output_with_timeout(command, timeout)
        .map_err(|error| format!("failed to launch {label}: {error}"))?
        .ok_or_else(|| {
            CliError::Message(format!(
                "{label} timed out after {} seconds",
                timeout.as_secs()
            ))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(CliError::Message(format!(
        "{label} failed:\n{}{}",
        stderr,
        if stdout.trim().is_empty() {
            String::new()
        } else {
            format!("\n{stdout}")
        }
    )))
}

fn run_checked(command: &mut Command, label: &str) -> Result<(), CliError> {
    let output = command
        .output()
        .map_err(|error| format!("failed to launch {label}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(CliError::Message(format!(
        "{label} failed:\n{}{}",
        stderr,
        if stdout.trim().is_empty() {
            String::new()
        } else {
            format!("\n{stdout}")
        }
    )))
}

fn default_binary_path(source: &Path) -> PathBuf {
    let mut path = source.with_extension("");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn build_native(c_source: &str, output: &Path, mode: BuildMode) -> Result<(), String> {
    build_native_configured(
        c_source,
        output,
        mode,
        NativeInstrumentation::None,
        &NativeTargetOptions::default(),
    )
}

fn build_native_instrumented(
    c_source: &str,
    output: &Path,
    mode: BuildMode,
    instrumentation: NativeInstrumentation,
) -> Result<(), String> {
    build_native_configured(
        c_source,
        output,
        mode,
        instrumentation,
        &NativeTargetOptions::default(),
    )
}

fn build_native_configured(
    c_source: &str,
    output: &Path,
    mode: BuildMode,
    instrumentation: NativeInstrumentation,
    native_target: &NativeTargetOptions,
) -> Result<(), String> {
    if let Some(sysroot) = native_target.sysroot.as_deref()
        && !sysroot.is_dir()
    {
        return Err(format!(
            "native sysroot '{}' is not a directory",
            sysroot.display()
        ));
    }
    let gtk = c_source.contains("#include <gtk/gtk.h>");
    let gtk_cflags = if gtk {
        pkg_config_flags("--cflags", "gtk4")?
    } else {
        Vec::new()
    };
    let gtk_libs = if gtk {
        pkg_config_flags("--libs", "gtk4")?
    } else {
        Vec::new()
    };
    let toolchain_identity = native_toolchain_cache_identity(gtk, native_target)?;
    let cache = native_build_cache_path_configured(
        c_source,
        mode,
        instrumentation,
        native_target,
        &toolchain_identity,
        &gtk_cflags,
        &gtk_libs,
    );
    if cache.is_file() && native_cache_entry_is_valid(&cache) {
        fs::copy(&cache, output).map_err(|error| {
            format!(
                "failed to restore native build cache '{}' to '{}': {error}",
                cache.display(),
                output.display()
            )
        })?;
        return Ok(());
    }
    if cache.exists() {
        let _ = fs::remove_file(&cache);
        let _ = fs::remove_file(native_cache_metadata_path(&cache));
    }

    let mut command = Command::new("clang");
    command
        .args(["-std=c17", "-fwrapv"])
        .args(mode.clang_args());
    if let Some(target) = native_target.triple.as_deref() {
        command.arg(format!("--target={target}"));
    }
    if let Some(sysroot) = native_target.sysroot.as_deref() {
        command.arg(format!("--sysroot={}", sysroot.display()));
    }
    match instrumentation {
        NativeInstrumentation::None => {}
        NativeInstrumentation::Gprof => {
            command.arg("-pg");
        }
        NativeInstrumentation::Coverage => {
            command.args(["-fprofile-instr-generate", "-fcoverage-mapping"]);
            command.arg(format!(
                "-fcoverage-compilation-dir={COVERAGE_COMPILATION_DIR}"
            ));
        }
    }
    if gtk {
        command.args(&gtk_cflags);
    }
    command.args(["-x", "c", "-"]);
    if gtk {
        command.args(&gtk_libs);
    }
    command.arg("-o").arg(output);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to launch clang: {error}"))?;
    child
        .stdin
        .take()
        .expect("clang stdin was configured as piped")
        .write_all(c_source.as_bytes())
        .map_err(|error| format!("failed to send generated C to clang: {error}"))?;
    let output_result = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for clang: {error}"))?;
    if !output_result.status.success() {
        return Err(format!(
            "native backend failed:\n{}",
            String::from_utf8_lossy(&output_result.stderr)
        ));
    }
    if let Some(parent) = cache.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let temporary_cache = cache.with_extension(format!("tmp-{}", std::process::id()));
        if fs::copy(output, &temporary_cache).is_ok() {
            if fs::rename(&temporary_cache, &cache).is_err() {
                let _ = fs::remove_file(&temporary_cache);
            } else if write_native_cache_metadata(&cache).is_err() {
                let _ = fs::remove_file(&cache);
                let _ = fs::remove_file(native_cache_metadata_path(&cache));
            }
        }
    }
    Ok(())
}

fn native_cache_metadata_path(cache: &Path) -> PathBuf {
    cache.with_extension("meta")
}

fn native_cache_file_identity(path: &Path) -> io::Result<(u64, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hash = 0xcbf29ce484222325u64;
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        for byte in &buffer[..read] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    Ok((size, hash))
}

fn native_cache_entry_is_valid(cache: &Path) -> bool {
    let Ok(metadata) = fs::read_to_string(native_cache_metadata_path(cache)) else {
        return false;
    };
    let mut fields = metadata.split_whitespace();
    let Some("v1") = fields.next() else {
        return false;
    };
    let Some(expected_size) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
        return false;
    };
    let Some(expected_hash) = fields
        .next()
        .and_then(|value| u64::from_str_radix(value, 16).ok())
    else {
        return false;
    };
    if fields.next().is_some() {
        return false;
    }
    matches!(
        native_cache_file_identity(cache),
        Ok((actual_size, actual_hash)) if actual_size == expected_size && actual_hash == expected_hash
    )
}

fn write_native_cache_metadata(cache: &Path) -> io::Result<()> {
    let (size, hash) = native_cache_file_identity(cache)?;
    let metadata = native_cache_metadata_path(cache);
    let temporary = metadata.with_extension(format!("meta.tmp-{}", std::process::id()));
    fs::write(&temporary, format!("v1 {size} {hash:016x}\n"))?;
    if let Err(error) = fs::rename(&temporary, &metadata) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn native_toolchain_cache_identity(
    gtk: bool,
    native_target: &NativeTargetOptions,
) -> Result<String, String> {
    let clang = command_first_line("clang", &["--version"])?;
    let target_arg = native_target
        .triple
        .as_deref()
        .map(|target| format!("--target={target}"));
    let mut linker_args = Vec::new();
    if let Some(target_arg) = target_arg.as_deref() {
        linker_args.push(target_arg);
    }
    linker_args.push("-print-prog-name=ld");
    let linker = command_first_line("clang", &linker_args)?;
    let linker_version =
        command_first_line(&linker, &["--version"]).unwrap_or_else(|_| "unknown".to_string());
    let mut identity = format!("clang={clang}\nlinker={linker}\nlinkerVersion={linker_version}");
    if gtk {
        let gtk_version = command_first_line("pkg-config", &["--modversion", "gtk4"])?;
        identity.push_str(&format!("\ngtk4={gtk_version}"));
    }
    Ok(identity)
}

fn native_build_cache_path_configured(
    c_source: &str,
    mode: BuildMode,
    instrumentation: NativeInstrumentation,
    native_target: &NativeTargetOptions,
    toolchain_identity: &str,
    gtk_cflags: &[String],
    gtk_libs: &[String],
) -> PathBuf {
    let target = native_target.triple.as_deref().unwrap_or("");
    let sysroot = native_target
        .sysroot
        .as_deref()
        .map(|path| path.to_string_lossy())
        .unwrap_or_default();
    let mut hash = 0xcbf29ce484222325u64;
    for bytes in [
        b"flux-native-cache-v4".as_slice(),
        env!("CARGO_PKG_VERSION").as_bytes(),
        toolchain_identity.as_bytes(),
        gtk_cflags.join("\u{1f}").as_bytes(),
        gtk_libs.join("\u{1f}").as_bytes(),
        mode.name().as_bytes(),
        instrumentation.cache_tag().as_bytes(),
        env::consts::OS.as_bytes(),
        env::consts::ARCH.as_bytes(),
        target.as_bytes(),
        sysroot.as_bytes(),
        c_source.as_bytes(),
    ] {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    native_build_cache_dir().join(format!("{hash:016x}"))
}

fn native_build_cache_dir() -> PathBuf {
    if let Some(path) = env::var_os("FLUX_CACHE_DIR") {
        return PathBuf::from(path).join("native");
    }
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("flux/native");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".cache/flux/native");
    }
    env::temp_dir().join("flux-cache/native")
}

fn pkg_config_flags(kind: &str, package: &str) -> Result<Vec<String>, String> {
    let output = Command::new("pkg-config")
        .args([kind, package])
        .output()
        .map_err(|error| format!("failed to launch pkg-config for {package}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "native backend requires {package}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect())
}

fn usage() -> String {
    let command = command_name();
    format!(
        "usage: {command} new <directory> | {command} check <file.flux|package-dir|flux.toml> [--json] | {command} analyze <file.flux|package-dir|flux.toml> [--json] | {command} format <file.flux> [--check] | {command} format --version | {command} emit-c <file.flux|package-dir|flux.toml> [-o file.c] | {command} build <file.flux|package-dir|flux.toml> [-o binary] [--mode debug|profile|release] [--target <clang-triple>] [--sysroot <directory>] | {command} build android <package-dir|flux.toml> [-o artifact] [--mode debug|profile|release] [--abi arm64-v8a|x86_64|armeabi-v7a] [--format apk|aab] | {command} package <package-dir|flux.toml> [-o path] [--mode debug|profile|release] [--format directory|tar.gz|container|systemd] [--target <clang-triple>] [--sysroot <directory>] | {command} publish android <package-dir|flux.toml> [-o artifact.aab] [--json] | {command} run <file.flux|package-dir|flux.toml> [--mode debug|profile|release] | {command} run android <package-dir|flux.toml> [--mode debug|profile|release] [--abi arm64-v8a|x86_64|armeabi-v7a] [--device <adb-serial>|waydroid] | {command} test <test.flux|package-dir|flux.toml> [--mode debug|profile|release] [--coverage] | {command} debug <file.flux|package-dir|flux.toml> [--break <file:line|function>] [--run] | {command} profile <file.flux|package-dir|flux.toml> | {command} symbolize <native-binary> <address> [address ...] | {command} symbols split <native-binary> [-o directory] | {command} devices | {command} doctor | {command} clean <file.flux|package-dir|flux.toml> | {command} lsp"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        AdbDevice, AndroidAbi, AndroidArtifactKind, AndroidRunTarget, BuildMode,
        NativeInstrumentation, NativeTargetOptions, PackageFormat, android_abi_from_runtime,
        android_activity_java_source, android_build_options, android_manifest_xml,
        android_publish_options, build_options, debug_options, demangle_profile_symbols,
        display_flux_symbol, json_string, native_build_cache_path_configured,
        native_cache_entry_is_valid, output_with_timeout, package_artifact_name, package_options,
        parse_adb_devices, profile_report_addresses, select_android_run_target,
        split_symbols_options, symbolize_options, test_options, validate_android_publish_manifest,
        waydroid_status_is_running, write_native_cache_metadata,
    };

    #[test]
    fn test_options_accept_coverage_without_confusing_build_output() {
        let options = test_options(&[
            "--coverage".to_string(),
            "--mode".to_string(),
            "profile".to_string(),
        ])
        .expect("test options should parse");
        assert!(options.coverage);
        assert_eq!(options.mode, BuildMode::Profile);
        assert!(test_options(&["-o".to_string(), "test-bin".to_string()]).is_err());
        assert!(test_options(&["--coverage".to_string(), "--coverage".to_string()]).is_err());
    }

    #[test]
    fn profiler_extracts_addresses_and_demangles_flux_functions() {
        let report = " 50.0  0.01  0.01  1  10  10 flux__fn_sumRange (<stdin>:4 @ 12af)\n 50.0 0.02 0.01 main (<stdin>:9 @ 13B0)\n";
        assert_eq!(
            profile_report_addresses(report),
            ["12af".to_string(), "13B0".to_string()]
        );
        let demangled = demangle_profile_symbols(report);
        assert!(demangled.contains("sumRange (<stdin>:4 @ 12af)"));
        assert!(!demangled.contains("flux__fn_sumRange"));
    }

    #[test]
    fn symbol_tools_parse_addresses_outputs_and_flux_names() {
        let symbolize = symbolize_options(&[
            "app".to_string(),
            "0x401ABC".to_string(),
            "deadbeef".to_string(),
        ])
        .expect("symbolize options should parse");
        assert_eq!(symbolize.binary, std::path::Path::new("app"));
        assert_eq!(symbolize.addresses, ["401abc", "deadbeef"]);
        assert!(symbolize_options(&["app".to_string(), "xyz".to_string()]).is_err());
        assert_eq!(display_flux_symbol("flux__fn_calculate"), "calculate");
        assert_eq!(display_flux_symbol("main"), "main");

        let split =
            split_symbols_options(&["app".to_string(), "-o".to_string(), "symbols".to_string()])
                .expect("symbols split options should parse");
        assert_eq!(split.binary, std::path::Path::new("app"));
        assert_eq!(
            split.output_directory.as_deref(),
            Some(std::path::Path::new("symbols"))
        );
        assert!(split_symbols_options(&["app".to_string(), "--bad".to_string()]).is_err());
    }

    #[test]
    fn package_options_support_directory_archive_and_native_target_options() {
        let defaults = package_options(&[]).expect("default package options should parse");
        assert_eq!(defaults.mode, BuildMode::Release);
        assert_eq!(defaults.format, PackageFormat::Directory);
        assert!(defaults.output.is_none());
        assert_eq!(defaults.native_target, NativeTargetOptions::default());

        let archive = package_options(&[
            "--format".to_string(),
            "tar.gz".to_string(),
            "--mode".to_string(),
            "profile".to_string(),
            "-o".to_string(),
            "app.tar.gz".to_string(),
            "--target".to_string(),
            "aarch64-unknown-linux-gnu".to_string(),
            "--sysroot".to_string(),
            "/opt/aarch64-sysroot".to_string(),
        ])
        .expect("archive package options should parse");
        assert_eq!(archive.mode, BuildMode::Profile);
        assert_eq!(archive.format, PackageFormat::TarGz);
        assert_eq!(
            archive.output.as_deref(),
            Some(std::path::Path::new("app.tar.gz"))
        );
        assert_eq!(
            archive.native_target.triple.as_deref(),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(
            archive.native_target.sysroot.as_deref(),
            Some(std::path::Path::new("/opt/aarch64-sysroot"))
        );
        let container = package_options(&["--format".to_string(), "container".to_string()])
            .expect("container package options should parse");
        assert_eq!(container.format, PackageFormat::Container);
        let systemd = package_options(&["--format".to_string(), "systemd".to_string()])
            .expect("systemd package options should parse");
        assert_eq!(systemd.format, PackageFormat::Systemd);
        assert!(package_options(&["--format".to_string(), "zip".to_string()]).is_err());
        assert!(package_options(&["--target".to_string(), "not a triple".to_string()]).is_err());
    }

    #[test]
    fn native_build_cache_separates_instrumentation_target_and_toolchain_configuration() {
        let source = "int main(void) { return 0; }";
        let toolchain = "clang=clang version test";
        let plain = native_build_cache_path_configured(
            source,
            BuildMode::Profile,
            NativeInstrumentation::None,
            &NativeTargetOptions::default(),
            toolchain,
            &[],
            &[],
        );
        let instrumented = native_build_cache_path_configured(
            source,
            BuildMode::Profile,
            NativeInstrumentation::Gprof,
            &NativeTargetOptions::default(),
            toolchain,
            &[],
            &[],
        );
        let targeted = native_build_cache_path_configured(
            source,
            BuildMode::Profile,
            NativeInstrumentation::None,
            &NativeTargetOptions {
                triple: Some("aarch64-unknown-linux-gnu".to_string()),
                sysroot: None,
            },
            toolchain,
            &[],
            &[],
        );
        let sysrooted = native_build_cache_path_configured(
            source,
            BuildMode::Profile,
            NativeInstrumentation::None,
            &NativeTargetOptions {
                triple: None,
                sysroot: Some(std::path::PathBuf::from("/opt/sysroot")),
            },
            toolchain,
            &[],
            &[],
        );
        let different_clang = native_build_cache_path_configured(
            source,
            BuildMode::Profile,
            NativeInstrumentation::None,
            &NativeTargetOptions::default(),
            "clang=clang version newer",
            &[],
            &[],
        );
        let different_gtk = native_build_cache_path_configured(
            source,
            BuildMode::Profile,
            NativeInstrumentation::None,
            &NativeTargetOptions::default(),
            toolchain,
            &["-I/opt/gtk/include".to_string()],
            &["-lgtk-4".to_string()],
        );
        assert_ne!(plain, instrumented);
        assert_ne!(plain, targeted);
        assert_ne!(plain, sysrooted);
        assert_ne!(plain, different_clang);
        assert_ne!(plain, different_gtk);
    }

    #[test]
    fn native_build_cache_metadata_rejects_corrupt_entries() {
        let root = std::env::temp_dir().join(format!(
            "flux-native-cache-integrity-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&root).expect("cache test directory should be writable");
        let cache = root.join("artifact");
        std::fs::write(&cache, b"native-binary-v1").expect("cache artifact should be writable");
        write_native_cache_metadata(&cache).expect("cache metadata should be writable");
        assert!(native_cache_entry_is_valid(&cache));

        std::fs::write(&cache, b"native-binary-v2").expect("cache artifact should be replaceable");
        assert!(!native_cache_entry_is_valid(&cache));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_artifact_names_include_explicit_native_target() {
        assert_eq!(
            package_artifact_name("demo", Some("1.2.3"), Some("aarch64-unknown-linux-gnu"))
                .unwrap_or_else(|_| panic!("target-labelled package name should be valid")),
            "demo-1.2.3-aarch64-unknown-linux-gnu"
        );
    }

    #[test]
    fn debug_options_accept_repeated_breakpoints_and_run() {
        let options = debug_options(&[
            "examples/branches.flux".to_string(),
            "--break".to_string(),
            "examples/branches.flux:3".to_string(),
            "-b".to_string(),
            "classify".to_string(),
            "--run".to_string(),
        ])
        .expect("debug options should parse");
        assert_eq!(
            options.target,
            std::path::Path::new("examples/branches.flux")
        );
        assert_eq!(
            options.breakpoints,
            [
                "examples/branches.flux:3".to_string(),
                "classify".to_string()
            ]
        );
        assert!(options.run_immediately);
        assert!(debug_options(&["--run".to_string()]).is_err());
        assert!(
            debug_options(&["examples/branches.flux".to_string(), "--break".to_string(),]).is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_command_output_returns_ready_processes_and_times_out_stalls() {
        let ready = output_with_timeout(
            std::process::Command::new("sh").args(["-c", "printf ready"]),
            std::time::Duration::from_secs(1),
        )
        .expect("ready command should launch")
        .expect("ready command should finish before its timeout");
        assert!(ready.status.success());
        assert_eq!(String::from_utf8_lossy(&ready.stdout), "ready");

        let started = std::time::Instant::now();
        let stalled = output_with_timeout(
            std::process::Command::new("sh").args(["-c", "sleep 1"]),
            std::time::Duration::from_millis(30),
        )
        .expect("stalled command should launch");
        assert!(stalled.is_none());
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn build_modes_have_distinct_native_optimization_profiles() {
        let debug = BuildMode::Debug.clang_args();
        assert!(debug.contains(&"-O0"));
        assert!(debug.contains(&"-g3"));
        assert!(debug.contains(&"-fno-omit-frame-pointer"));
        assert!(!debug.contains(&"-DNDEBUG"));

        let profile = BuildMode::Profile.clang_args();
        assert!(profile.contains(&"-O2"));
        assert!(profile.contains(&"-g"));
        assert!(profile.contains(&"-fno-omit-frame-pointer"));
        assert!(profile.contains(&"-DNDEBUG"));

        let release = BuildMode::Release.clang_args();
        assert!(release.contains(&"-O3"));
        assert!(release.contains(&"-flto"));
        assert!(release.contains(&"-DNDEBUG"));
        assert!(!release.contains(&"-g"));
    }

    #[test]
    fn build_options_accept_mode_output_and_native_target_in_any_order() {
        let args = vec![
            "--target".to_string(),
            "x86_64-unknown-linux-gnu".to_string(),
            "--mode".to_string(),
            "profile".to_string(),
            "--sysroot".to_string(),
            "/sdk/sysroot".to_string(),
            "-o".to_string(),
            "app".to_string(),
        ];
        let options = build_options(&args, BuildMode::Release).expect("options should parse");
        assert_eq!(options.mode, BuildMode::Profile);
        assert_eq!(options.output.as_deref(), Some(std::path::Path::new("app")));
        assert_eq!(
            options.native_target.triple.as_deref(),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            options.native_target.sysroot.as_deref(),
            Some(std::path::Path::new("/sdk/sysroot"))
        );

        let default = build_options(&[], BuildMode::Debug).expect("defaults should parse");
        assert_eq!(default.mode, BuildMode::Debug);
        assert!(default.output.is_none());
        assert_eq!(default.native_target, NativeTargetOptions::default());
        assert!(
            build_options(
                &["--target".to_string(), "linux".to_string()],
                BuildMode::Release
            )
            .is_err()
        );
    }

    #[test]
    fn android_build_options_parse_target_mode_output_and_abi() {
        let args = vec![
            "package".to_string(),
            "--abi".to_string(),
            "x86_64".to_string(),
            "-o".to_string(),
            "app.apk".to_string(),
            "--mode".to_string(),
            "profile".to_string(),
        ];
        let options =
            android_build_options(&args, BuildMode::Release).expect("Android options should parse");
        assert_eq!(options.target, std::path::Path::new("package"));
        assert_eq!(
            options.output.as_deref(),
            Some(std::path::Path::new("app.apk"))
        );
        assert_eq!(options.mode, BuildMode::Profile);
        assert_eq!(options.abi, AndroidAbi::X86_64);
        assert!(options.abi_explicit);
        assert!(options.device.is_none());
        assert_eq!(options.kind, AndroidArtifactKind::Apk);

        let selected = android_build_options(
            &[
                "package".to_string(),
                "--device".to_string(),
                "192.168.1.125:5555".to_string(),
            ],
            BuildMode::Debug,
        )
        .expect("explicit Android device should parse");
        assert_eq!(selected.device.as_deref(), Some("192.168.1.125:5555"));
        assert_eq!(selected.abi, AndroidAbi::Arm64V8a);
        assert!(!selected.abi_explicit);

        let aab = android_build_options(
            &[
                "package".to_string(),
                "--format".to_string(),
                "aab".to_string(),
            ],
            BuildMode::Release,
        )
        .expect("AAB format should parse");
        assert_eq!(aab.kind, AndroidArtifactKind::Aab);

        let defaults = android_build_options(&["package".to_string()], BuildMode::Debug)
            .expect("Android defaults should parse");
        assert_eq!(defaults.mode, BuildMode::Debug);
        assert_eq!(defaults.abi, AndroidAbi::Arm64V8a);
        assert!(!defaults.abi_explicit);
        assert_eq!(defaults.kind, AndroidArtifactKind::Apk);
        assert!(defaults.device.is_none());
        assert!(defaults.output.is_none());

        let duplicate_device = android_build_options(
            &[
                "package".to_string(),
                "--device".to_string(),
                "phone".to_string(),
                "--device".to_string(),
                "other".to_string(),
            ],
            BuildMode::Debug,
        )
        .expect_err("duplicate Android device selection should fail");
        assert!(duplicate_device.contains("only be specified once"));
    }

    #[test]
    fn android_publish_options_and_play_policy_are_strict() {
        let options = android_publish_options(&[
            "package".to_string(),
            "-o".to_string(),
            "release.aab".to_string(),
            "--json".to_string(),
        ])
        .expect("publish options should parse");
        assert_eq!(options.target, std::path::Path::new("package"));
        assert_eq!(
            options.output.as_deref(),
            Some(std::path::Path::new("release.aab"))
        );
        assert!(options.json);
        assert!(
            android_publish_options(&[
                "package".to_string(),
                "-o".to_string(),
                "release.apk".to_string(),
            ])
            .expect_err("publish output must be an AAB")
            .contains("must end in '.aab'")
        );
        assert!(
            android_publish_options(&["package".to_string(), "--mode".to_string()])
                .expect_err("publish mode is fixed to release")
                .contains("unknown Android publish option")
        );

        let mut manifest = crate::project::PackageManifest {
            format_version: crate::project::PACKAGE_FORMAT_VERSION,
            name: "example".to_string(),
            version: None,
            entry: std::path::PathBuf::from("src/main.flux"),
            path: std::path::PathBuf::from("flux.toml"),
            android: crate::project::AndroidPackageConfig {
                application_id: "app.flux.example".to_string(),
                version_code: 42,
                min_sdk: 23,
                target_sdk: 36,
                permissions: vec![],
                keystore: None,
                key_alias: None,
            },
        };
        assert!(
            validate_android_publish_manifest(&manifest)
                .expect_err("publishing requires an explicit version")
                .contains("[package].version")
        );
        manifest.version = Some("1.2.3".to_string());
        manifest.android.target_sdk = 35;
        assert!(
            validate_android_publish_manifest(&manifest)
                .expect_err("phone/tablet publishing requires current target API")
                .contains("target_sdk >= 36")
        );
        manifest.android.target_sdk = 36;
        assert!(
            validate_android_publish_manifest(&manifest)
                .expect_err("publishing may not fall back to the development key")
                .contains("development signing keys are never accepted")
        );
        manifest.android.keystore = Some(std::path::PathBuf::from("upload.jks"));
        manifest.android.key_alias = Some("upload".to_string());
        validate_android_publish_manifest(&manifest)
            .expect("complete publishing metadata should pass static policy checks");

        assert_eq!(json_string("a\\b\"c\n"), "\"a\\\\b\\\"c\\n\"");
    }

    #[test]
    fn android_manifest_adds_vibrate_permission_only_when_needed() {
        let manifest = crate::project::PackageManifest {
            format_version: crate::project::PACKAGE_FORMAT_VERSION,
            name: "example".to_string(),
            version: Some("1.0.0".to_string()),
            entry: std::path::PathBuf::from("src/main.flux"),
            path: std::path::PathBuf::from("flux.toml"),
            android: crate::project::AndroidPackageConfig {
                application_id: "app.flux.example".to_string(),
                version_code: 42,
                min_sdk: 23,
                target_sdk: 36,
                permissions: vec!["android.permission.CAMERA".to_string()],
                keystore: None,
                key_alias: None,
            },
        };

        let plain = android_manifest_xml(
            &manifest,
            BuildMode::Release,
            "int main(void) { return 0; }",
        );
        assert!(plain.contains("android:versionCode=\"42\""));
        assert!(plain.contains("android:targetSdkVersion=\"36\""));
        assert!(plain.contains("android.permission.CAMERA"));
        assert!(!plain.contains("android.permission.VIBRATE"));
        assert!(!plain.contains("android.permission.POST_NOTIFICATIONS"));
        assert!(plain.contains("android:hasCode=\"false\""));
        assert!(plain.contains("android:name=\"android.app.NativeActivity\""));

        let generated_ui = android_manifest_xml(
            &manifest,
            BuildMode::Release,
            "JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeBuildUi(void);",
        );
        assert!(generated_ui.contains("android:hasCode=\"true\""));
        assert!(generated_ui.contains("android:name=\"app.flux.runtime.FluxActivity\""));
        assert!(
            generated_ui
                .contains("android:configChanges=\"orientation|screenSize|smallestScreenSize")
        );
        assert!(!generated_ui.contains("android.app.lib_name"));
        let activity = android_activity_java_source();
        assert!(activity.contains("extends Activity implements View.OnClickListener, CompoundButton.OnCheckedChangeListener"));
        assert!(
            activity.contains("View.OnHoverListener, View.OnLongClickListener, View.OnKeyListener")
        );
        assert!(!activity.contains("View.OnTouchListener"));
        assert!(!activity.contains("extends NativeActivity"));
        assert!(activity.contains("System.loadLibrary(\"flux\");"));
        assert!(activity.contains("private native int nativeThemeMode();"));
        assert!(activity.contains("private native String nativeThemeColor(String token);"));
        assert!(activity.contains("view.setContentDescription"));
        assert!(activity.contains("setAccessibilityRole(View view, String role)"));
        assert!(activity.contains("info.setClassName(className)"));
        assert!(activity.contains("info.setHeading(\"heading\".equals(role))"));
        assert!(activity.contains("String custom = nativeThemeColor(value);"));
        assert!(activity.contains("private native void nativeCreate(String restoredState);"));
        assert!(activity.contains("private native void nativeBuildUi();"));
        assert!(activity.contains("private native String nativeSaveState();"));
        assert!(activity.contains("fluxThemeMode = nativeThemeMode();"));
        assert!(activity.contains("Configuration.UI_MODE_NIGHT_MASK"));
        assert!(activity.contains("Theme_DeviceDefault_NoActionBar"));
        assert!(activity.contains("Theme_DeviceDefault_Light_NoActionBar"));
        assert!(activity.contains("if (fluxThemeMode == 0) applyFluxTheme();"));
        assert!(activity.contains("nativeCreate(restoredState);"));
        assert!(activity.contains("nativeConfigurationChanged();"));
        assert!(activity.contains("nativeDestroy();"));
        assert!(activity.contains("private static native void nativeOnClick(int viewId);"));
        assert!(activity.contains("private static native void nativeOnTap(int viewId);"));
        assert!(
            activity.contains("private static native void nativeOnKey(int viewId, String key);")
        );
        assert!(activity.contains("return \"ArrowLeft\";"));
        assert!(activity.contains("nativeOnKey(view.getId(), fluxKeyName(keyCode, event));"));
        assert!(activity.contains("private static native void nativeOnLongPress(int viewId);"));
        assert!(activity.contains("nativeOnClick(viewId);"));
        assert!(activity.contains("nativeOnTap(viewId);"));
        assert!(activity.contains("nativeOnLongPress(view.getId());"));
        assert!(
            activity.contains(
                "private static native void nativeOnChecked(int viewId, boolean checked);"
            )
        );
        assert!(
            activity.contains(
                "private static native void nativeOnTextChanged(int viewId, String text);"
            )
        );
        assert!(
            activity
                .contains("private static native void nativeOnSubmit(int viewId, String text);")
        );
        assert!(activity.contains("public static final class FluxEditText extends EditText"));
        assert!(
            activity.contains(
                "public void restoreTextInput(EditText view, int viewId, String fallback)"
            )
        );
        assert!(activity.contains("selectionStarts"));
        assert!(activity.contains("rememberSelection"));
        assert!(activity.contains("restoringFocus"));
        assert!(activity.contains("restoringCheckedState"));
        assert!(
            activity
                .contains("public void setCheckedSilently(CompoundButton button, boolean checked)")
        );
        assert!(activity.contains("if (!restoringCheckedState) nativeOnChecked"));
        assert!(activity.contains("view.findViewById(previousFocusId)"));
        assert!(activity.contains("replacement.requestFocus()"));
        assert!(activity.contains("composingStarts"));
        assert!(activity.contains("BaseInputConnection.getComposingSpanStart"));
        assert!(activity.contains("setComposingRegion"));
        assert!(activity.contains("private final class FluxStyleDrawable extends Drawable"));
        assert!(activity.contains("new DashPathEffect"));
        assert!(activity.contains("drawHorizontalBorder"));
        assert!(activity.contains("drawVerticalBorder"));
        assert!(activity.contains("public void styleView("));
        assert!(activity.contains("String borderTop"));
        assert!(activity.contains("String shadowColor"));
        assert!(activity.contains("view.setBackgroundTintList(null);"));
        assert!(activity.contains("public void styleRoot(View view)"));
        assert!(activity.contains("case \"surface\":"));
        assert!(activity.contains("case \"accent\":"));
        assert!(activity.contains("case \"textMuted\":"));
        assert!(activity.contains(
            "UiModeManager manager = (UiModeManager) getSystemService(UI_MODE_SERVICE);"
        ));
        assert!(activity.contains("manager.getContrast()"));
        assert!(activity.contains("custom == null && isFluxHighContrast()"));
        assert!(activity.contains("if (Float.compare(nextContrast, fluxContrast) != 0)"));
        assert!(activity.contains("private void applyFluxTextLocales(TextView view)"));
        assert!(
            activity
                .contains("view.setTextLocales(getResources().getConfiguration().getLocales())")
        );
        assert!(activity.contains(
            "((EditText)view).setImeHintLocales(getResources().getConfiguration().getLocales())"
        ));
        assert!(activity.contains("view.setTextLocale(Locale.getDefault())"));
        assert!(activity.contains("public void styleButton(Button view, boolean primary)"));
        assert!(activity.contains("applyFluxTextLocales(view);"));
        assert!(activity.contains("view.setAllCaps(false);"));
        assert!(activity.contains("-android.R.attr.state_enabled"));
        assert!(activity.contains("android.R.attr.state_pressed"));
        assert!(
            activity.contains("public void styleTextInput(EditText view, String validationState)")
        );
        assert!(activity.contains("\"error\".equals(validationState)"));
        assert!(activity.contains("parseFluxColor(\"danger\")"));
        assert!(activity.contains("parseFluxColor(\"success\")"));
        assert!(activity.contains("parseFluxColor(\"warning\")"));
        assert!(activity.contains("android.R.attr.state_focused"));
        assert!(activity.contains("if (size > 0) view.setTextSize(size);"));
        assert!(
            !activity.contains("COMPLEX_UNIT_PX"),
            "generated Android typography must keep TextView's scaled-pixel semantics so user font scaling remains active"
        );
        assert!(activity.contains("public void styleCheckable(CompoundButton view)"));
        assert!(activity.contains("android.R.attr.state_checked"));
        assert!(activity.contains("public void setTooltip(View view, String text)"));
        assert!(
            activity.contains(
                "public void setAccessibility(View view, String label, String description)"
            )
        );
        assert!(activity.contains("public void setAccessibilityHidden(View view, boolean hidden)"));
        assert!(activity.contains("View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS"));
        assert!(activity.contains(
            "public void wireTextInput(EditText view, boolean onChange, boolean onSubmit, boolean multiline, boolean submitOnEnter)"
        ));
        assert!(activity.contains("EditorInfo.IME_FLAG_NO_ENTER_ACTION"));
        assert!(activity.contains("nativeBuildUi();"));

        let vibrating = android_manifest_xml(
            &manifest,
            BuildMode::Release,
            "static void f(void) { flux__android_vibrate(25); }",
        );
        assert!(vibrating.contains("android.permission.VIBRATE"));
        assert!(!vibrating.contains("android.permission.POST_NOTIFICATIONS"));

        for source in [
            "static void f(void) { flux__android_notify(\"c\", 1, \"t\", \"b\"); }",
            "static void f(void) { flux__android_notify_url_action(\"c\", 1, \"t\", \"b\", \"open\", \"https://example.com\"); }",
            "static bool f(void) { return flux__android_notification_permission_granted(); }",
            "static void f(void) { flux__android_request_notification_permission(); }",
        ] {
            let notifying = android_manifest_xml(&manifest, BuildMode::Release, source);
            assert!(notifying.contains("android.permission.POST_NOTIFICATIONS"));
        }
    }

    #[test]
    fn android_runtime_abi_names_map_to_supported_build_abis() {
        assert_eq!(
            android_abi_from_runtime("arm64-v8a\n"),
            Some(AndroidAbi::Arm64V8a)
        );
        assert_eq!(android_abi_from_runtime("x86_64"), Some(AndroidAbi::X86_64));
        assert_eq!(
            android_abi_from_runtime("armeabi-v7a"),
            Some(AndroidAbi::ArmeabiV7a)
        );
        assert_eq!(android_abi_from_runtime("x86"), None);
    }

    #[test]
    fn android_run_target_matches_built_abi_across_adb_and_waydroid() {
        let arm_phone = AdbDevice {
            serial: "phone".to_string(),
            status: "device".to_string(),
            description: String::new(),
        };
        let x86_emulator = AdbDevice {
            serial: "emulator".to_string(),
            status: "device".to_string(),
            description: String::new(),
        };

        let target = select_android_run_target(
            AndroidAbi::X86_64,
            &[(arm_phone, Some(AndroidAbi::Arm64V8a))],
            true,
            Some(AndroidAbi::X86_64),
            None,
        )
        .expect("x86_64 artifact should prefer matching Waydroid over arm64 adb");
        assert_eq!(target, AndroidRunTarget::Waydroid);

        let target = select_android_run_target(
            AndroidAbi::X86_64,
            &[(x86_emulator, Some(AndroidAbi::X86_64))],
            true,
            Some(AndroidAbi::X86_64),
            None,
        )
        .expect("matching adb device should remain the first runtime choice");
        assert_eq!(target, AndroidRunTarget::Adb("emulator".to_string()));

        let mismatch = select_android_run_target(
            AndroidAbi::X86_64,
            &[(
                (AdbDevice {
                    serial: "phone".to_string(),
                    status: "device".to_string(),
                    description: String::new(),
                }),
                Some(AndroidAbi::Arm64V8a),
            )],
            false,
            None,
            None,
        )
        .expect_err("known ABI mismatch should fail before adb install");
        assert!(mismatch.contains("does not match an available runtime"));
        assert!(mismatch.contains("phone (arm64-v8a)"));

        let phone = AdbDevice {
            serial: "phone".to_string(),
            status: "device".to_string(),
            description: String::new(),
        };
        let other = AdbDevice {
            serial: "other".to_string(),
            status: "device".to_string(),
            description: String::new(),
        };
        let explicit = select_android_run_target(
            AndroidAbi::Arm64V8a,
            &[
                (phone, Some(AndroidAbi::Arm64V8a)),
                (other, Some(AndroidAbi::Arm64V8a)),
            ],
            true,
            Some(AndroidAbi::X86_64),
            Some("other"),
        )
        .expect("explicit serial should disambiguate matching adb devices");
        assert_eq!(explicit, AndroidRunTarget::Adb("other".to_string()));

        let explicit_waydroid = select_android_run_target(
            AndroidAbi::X86_64,
            &[],
            true,
            Some(AndroidAbi::X86_64),
            Some("waydroid"),
        )
        .expect("explicit Waydroid selection should be supported");
        assert_eq!(explicit_waydroid, AndroidRunTarget::Waydroid);

        let explicit_mismatch = select_android_run_target(
            AndroidAbi::X86_64,
            &[(
                AdbDevice {
                    serial: "phone".to_string(),
                    status: "device".to_string(),
                    description: String::new(),
                },
                Some(AndroidAbi::Arm64V8a),
            )],
            false,
            None,
            Some("phone"),
        )
        .expect_err("explicit device ABI mismatch should fail before install");
        assert!(explicit_mismatch.contains("requested adb device phone ABI arm64-v8a"));
    }

    #[test]
    fn waydroid_status_requires_running_session_and_container() {
        assert!(waydroid_status_is_running(
            "Session: RUNNING\nContainer: RUNNING\nVendor type: MAINLINE\n"
        ));
        assert!(!waydroid_status_is_running(
            "Session: STOPPED\nContainer: RUNNING\n"
        ));
        assert!(!waydroid_status_is_running(
            "Session: RUNNING\nContainer: STOPPED\n"
        ));
    }

    #[test]
    fn adb_device_parser_preserves_connection_state_and_details() {
        let devices = parse_adb_devices(
            "List of devices attached\nemulator-5554 device product:sdk model:Pixel transport_id:1\nphone offline usb:1-1\nunauth unauthorized usb:2-1\n\n",
        );
        assert_eq!(
            devices,
            vec![
                AdbDevice {
                    serial: "emulator-5554".to_string(),
                    status: "device".to_string(),
                    description: "product:sdk model:Pixel transport_id:1".to_string(),
                },
                AdbDevice {
                    serial: "phone".to_string(),
                    status: "offline".to_string(),
                    description: "usb:1-1".to_string(),
                },
                AdbDevice {
                    serial: "unauth".to_string(),
                    status: "unauthorized".to_string(),
                    description: "usb:2-1".to_string(),
                },
            ]
        );
    }
}
