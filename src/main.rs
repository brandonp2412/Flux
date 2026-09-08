use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fluxc::{Diagnostic, DiagnosticSource, TerminalRenderOptions};

enum CliError {
    Message(String),
    Reported,
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
            let sources = validate_project(path)?;
            let generated = match fluxc::project::compile_to_c(path) {
                Ok(generated) => generated,
                Err(diagnostic) => {
                    report_diagnostics(path, &[diagnostic], &sources);
                    return Err(CliError::Reported);
                }
            };
            let output = if let Some(output) = output_path(&args[2..])? {
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
            build_native(&generated, &output)?;
            println!("built: {}", output.display());
            Ok(())
        }
        _ => Err(CliError::Message(usage())),
    }
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

fn default_binary_path(source: &Path) -> PathBuf {
    let mut path = source.with_extension("");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn build_native(c_source: &str, output: &Path) -> Result<(), String> {
    let temp = env::temp_dir().join(format!("fluxc-{}.c", std::process::id()));
    fs::write(&temp, c_source)
        .map_err(|error| format!("failed to write temporary C source: {error}"))?;

    let result = Command::new("clang")
        .args(["-std=c17", "-O3", "-flto", "-fwrapv", "-DNDEBUG"])
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
    "usage: fluxc check <file.flux|package-dir|flux.toml> [--json] | fluxc format <file.flux> [--check] | fluxc emit-c <file.flux|package-dir|flux.toml> [-o file.c] | fluxc build <file.flux|package-dir|flux.toml> [-o binary]".to_string()
}
