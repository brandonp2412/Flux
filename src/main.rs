use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Message(message)) => {
            eprintln!("fluxc: {message}");
            ExitCode::FAILURE
        }
        Err(CliError::Reported) => ExitCode::FAILURE,
    }
}

fn run() -> Result<(), CliError> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err(CliError::Message(usage()));
    }

    match args[0].as_str() {
        "check" => {
            let path = require_target(&args)?;
            let json = check_json_mode(&args[2..])?;
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

fn run_development(target: &Path, mode: BuildMode) -> Result<(), CliError> {
    let mut generation = 0usize;
    write_development_status(
        target,
        "starting",
        generation,
        mode,
        "building initial process",
    );
    let (mut child, mut binary, mut watch_paths) =
        match start_development_build(target, generation, mode) {
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
        let (diagnostics, sources) = fluxc::project::check_with_sources(target);
        if !diagnostics.is_empty() {
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

        let generated = match fluxc::project::compile_to_c(target) {
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
) -> Result<(Option<Child>, PathBuf, Vec<PathBuf>), CliError> {
    let sources = validate_project(target)?;
    let generated = match fluxc::project::compile_to_c(target) {
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

fn check_json_mode(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--json" => Ok(true),
        _ => Err("check syntax is 'check <file.flux> [--json]'".to_string()),
    }
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
    let temp = env::temp_dir().join(format!("fluxc-{}.c", std::process::id()));
    fs::write(&temp, c_source)
        .map_err(|error| format!("failed to write temporary C source: {error}"))?;

    let result = Command::new("clang")
        .args(["-std=c17", "-fwrapv"])
        .args(mode.clang_args())
        .arg(&temp)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|error| format!("failed to launch clang: {error}"));

    let _ = fs::remove_file(&temp);
    let output_result = result?;
    if !output_result.status.success() {
        return Err(format!(
            "native backend failed:\n{}",
            String::from_utf8_lossy(&output_result.stderr)
        ));
    }
    Ok(())
}

fn usage() -> String {
    "usage: fluxc check <file.flux|package-dir|flux.toml> [--json] | fluxc format <file.flux> [--check] | fluxc emit-c <file.flux|package-dir|flux.toml> [-o file.c] | fluxc build <file.flux|package-dir|flux.toml> [-o binary] [--mode debug|profile|release] | fluxc run <file.flux|package-dir|flux.toml> [--mode debug|profile|release] | fluxc lsp".to_string()
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
