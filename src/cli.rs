use crate as fluxc;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fluxc::{Diagnostic, DiagnosticSource, TerminalRenderOptions};

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

#[derive(Debug)]
struct BuildOptions {
    output: Option<PathBuf>,
    mode: BuildMode,
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
                        "{{\"ok\":true,\"source_id\":{},\"diagnostics\":[]}}",
                        source_id.value()
                    );
                } else {
                    println!("ok: {}", path.display());
                }
                return Ok(());
            }
            if json {
                println!(
                    "{{\"ok\":false,\"source_id\":{},\"diagnostics\":{}}}",
                    source_id.value(),
                    fluxc::diagnostics_to_json(&diagnostics)
                );
            } else {
                report_diagnostics(path, &diagnostics, &sources);
            }
            Err(CliError::Reported)
        }
        "format" => {
            let path = require_source(&args)?;
            let check_only = match &args[2..] {
                [] => false,
                [flag] if flag == "--check" => true,
                _ => {
                    return Err(CliError::Message(
                        "format syntax is 'format <file.flux> [--check]'".to_string(),
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
            build_native(&generated, &output, options.mode)?;
            println!("built ({}): {}", options.mode.name(), output.display());
            Ok(())
        }
        "package" => {
            let path = require_target(&args)?;
            let options = build_options(&args[2..], BuildMode::Release)?;
            package_target(path, options)
        }
        "run" => {
            let path = require_target(&args)?;
            let options = build_options(&args[2..], BuildMode::Debug)?;
            if options.output.is_some() {
                return Err(CliError::Message(
                    "run does not accept '-o'; development binaries are managed automatically"
                        .to_string(),
                ));
            }
            run_development(path, options.mode)
        }
        "test" => {
            let path = require_target(&args)?;
            let options = build_options(&args[2..], BuildMode::Debug)?;
            if options.output.is_some() {
                return Err(CliError::Message(
                    "test does not accept '-o'; test binaries are temporary".to_string(),
                ));
            }
            run_tests(path, options.mode)
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

fn package_target(target: &Path, options: BuildOptions) -> Result<(), CliError> {
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
    let artifact_name = package_artifact_name(&manifest.name, manifest.version.as_deref())?;
    let output = options
        .output
        .unwrap_or_else(|| package_root.join("dist").join(&artifact_name));
    if output.exists() {
        return Err(CliError::Message(format!(
            "package output '{}' already exists; remove it or choose another path with -o",
            output.display()
        )));
    }

    let sources = validate_project(&manifest.path)?;
    let generated = match fluxc::project::compile_to_c(&manifest.path) {
        Ok(generated) => generated,
        Err(diagnostic) => {
            report_diagnostics(&manifest.path, &[diagnostic], &sources);
            return Err(CliError::Reported);
        }
    };
    fs::create_dir_all(&output)
        .map_err(|error| format!("failed to create package '{}': {error}", output.display()))?;
    let binary = output.join(&manifest.name);
    if let Err(error) = build_native(&generated, &binary, options.mode) {
        let _ = fs::remove_dir_all(&output);
        return Err(CliError::Message(error));
    }
    if let Err(error) = fs::copy(&manifest.path, output.join("flux.toml")) {
        let _ = fs::remove_dir_all(&output);
        return Err(CliError::Message(format!(
            "failed to copy package manifest: {error}"
        )));
    }
    println!("packaged ({}): {}", options.mode.name(), output.display());
    Ok(())
}

fn package_artifact_name(name: &str, version: Option<&str>) -> Result<String, CliError> {
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
    Ok(format!(
        "{name}-{version}-{}-{}",
        env::consts::OS,
        env::consts::ARCH
    ))
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
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nentry = \"src/main.flux\"\n"
    );
    let main = "view App {\n    grid columns: 1fr\n    grid rows: auto auto\n    grid gap: 12\n    state clicked: bool = false\n\n    Text title at 1,1\n        text: \"Hello, Flux!\"\n        visible: !clicked\n\n    Button action at 2,1\n        text: \"Toggle\"\n        on_press: clicked => !clicked\n}\n\napp App\n";
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

fn run_tests(target: &Path, mode: BuildMode) -> Result<(), CliError> {
    let tests = discover_test_targets(target)?;
    let mut passed = 0usize;
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
        let generated = match fluxc::codegen::emit_c(&analysis.program, &analysis.signatures) {
            Ok(generated) => generated,
            Err(diagnostic) => {
                eprintln!("test {} ... FAILED", test.display());
                report_diagnostics(test, &[diagnostic], &analysis.sources);
                return Err(CliError::Reported);
            }
        };
        let binary = test_binary_path(index);
        build_native(&generated, &binary, mode)?;
        let status = Command::new(&binary).status().map_err(|error| {
            format!(
                "failed to launch test binary '{}': {error}",
                binary.display()
            )
        })?;
        let _ = fs::remove_file(&binary);
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
        println!("test {} ... ok", test.display());
        passed += 1;
    }
    println!(
        "test result: ok. {passed} passed; 0 failed; mode {}",
        mode.name()
    );
    Ok(())
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
        let generated = match fluxc::codegen::emit_c(&analysis.program, &analysis.signatures) {
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
    let generated = match fluxc::codegen::emit_c(&analysis.program, &analysis.signatures) {
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

fn run_devices() -> Result<(), CliError> {
    println!("Flux devices");
    if !cfg!(target_os = "linux") {
        println!("  no runnable devices: the current bootstrap application backend targets Linux");
        return Ok(());
    }

    let (display, status) = if let Some(display) = env::var_os("WAYLAND_DISPLAY") {
        (format!("Wayland ({})", display.to_string_lossy()), "ready")
    } else if let Some(display) = env::var_os("DISPLAY") {
        (format!("X11 ({})", display.to_string_lossy()), "ready")
    } else {
        ("no active graphical display".to_string(), "unavailable")
    };
    println!("  linux-desktop  {status:11} GTK4 · {display}");
    Ok(())
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

    match command_first_line("pkg-config", &["--modversion", "gtk4"]) {
        Ok(version) => println!("  [ok] GTK4: {version}"),
        Err(message) => {
            println!("  [fail] GTK4: {message}");
            required_ok = false;
        }
    }

    match command_first_line("nvim", &["--version"]) {
        Ok(version) => println!("  [ok] Neovim editor dogfood: {version}"),
        Err(message) => println!("  [warn] Neovim editor dogfood unavailable: {message}"),
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

fn build_options(args: &[String], default_mode: BuildMode) -> Result<BuildOptions, String> {
    let mut output = None;
    let mut mode = default_mode;
    let mut mode_seen = false;
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
            flag => {
                return Err(format!(
                    "unknown build option '{flag}'; expected '-o <path>' or '--mode <debug|profile|release>'"
                ));
            }
        }
    }
    Ok(BuildOptions { output, mode })
}

fn default_binary_path(source: &Path) -> PathBuf {
    let mut path = source.with_extension("");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn build_native(c_source: &str, output: &Path, mode: BuildMode) -> Result<(), String> {
    let cache = native_build_cache_path(c_source, mode);
    if cache.is_file() {
        fs::copy(&cache, output).map_err(|error| {
            format!(
                "failed to restore native build cache '{}' to '{}': {error}",
                cache.display(),
                output.display()
            )
        })?;
        return Ok(());
    }

    let mut command = Command::new("clang");
    command
        .args(["-std=c17", "-fwrapv"])
        .args(mode.clang_args());
    let gtk = c_source.contains("#include <gtk/gtk.h>");
    if gtk {
        command.args(pkg_config_flags("--cflags", "gtk4")?);
    }
    command.args(["-x", "c", "-"]);
    if gtk {
        command.args(pkg_config_flags("--libs", "gtk4")?);
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
        if fs::copy(output, &temporary_cache).is_ok()
            && fs::rename(&temporary_cache, &cache).is_err()
        {
            let _ = fs::remove_file(&temporary_cache);
        }
    }
    Ok(())
}

fn native_build_cache_path(c_source: &str, mode: BuildMode) -> PathBuf {
    let mut hash = 0xcbf29ce484222325u64;
    for bytes in [
        b"flux-native-cache-v1".as_slice(),
        env!("CARGO_PKG_VERSION").as_bytes(),
        mode.name().as_bytes(),
        env::consts::OS.as_bytes(),
        env::consts::ARCH.as_bytes(),
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
        "usage: {command} new <directory> | {command} check <file.flux|package-dir|flux.toml> [--json] | {command} analyze <file.flux|package-dir|flux.toml> [--json] | {command} format <file.flux> [--check] | {command} emit-c <file.flux|package-dir|flux.toml> [-o file.c] | {command} build <file.flux|package-dir|flux.toml> [-o binary] [--mode debug|profile|release] | {command} package <package-dir|flux.toml> [-o directory] [--mode debug|profile|release] | {command} run <file.flux|package-dir|flux.toml> [--mode debug|profile|release] | {command} test <test.flux|package-dir|flux.toml> [--mode debug|profile|release] | {command} devices | {command} doctor | {command} clean <file.flux|package-dir|flux.toml> | {command} lsp"
    )
}

#[cfg(test)]
mod tests {
    use super::{BuildMode, build_options};

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
    fn build_options_accept_mode_and_output_in_either_order() {
        let args = vec![
            "--mode".to_string(),
            "profile".to_string(),
            "-o".to_string(),
            "app".to_string(),
        ];
        let options = build_options(&args, BuildMode::Release).expect("options should parse");
        assert_eq!(options.mode, BuildMode::Profile);
        assert_eq!(options.output.as_deref(), Some(std::path::Path::new("app")));

        let default = build_options(&[], BuildMode::Debug).expect("defaults should parse");
        assert_eq!(default.mode, BuildMode::Debug);
        assert!(default.output.is_none());
    }
}
