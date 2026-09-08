use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

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
            let path = require_source(&args)?;
            let json = check_json_mode(&args[2..])?;
            let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let source_id = fluxc::SourceId::from_name(canonical.to_string_lossy().as_ref());
            match fluxc::project::check(path) {
                Ok(()) if json => {
                    println!(
                        "{{\"ok\":true,\"source_id\":{},\"diagnostics\":[]}}",
                        source_id.value()
                    );
                    Ok(())
                }
                Ok(()) => {
                    println!("ok: {}", path.display());
                    Ok(())
                }
                Err(diagnostics) if json => {
                    println!(
                        "{{\"ok\":false,\"source_id\":{},\"diagnostics\":{}}}",
                        source_id.value(),
                        fluxc::diagnostics_to_json(&diagnostics)
                    );
                    Err(CliError::Reported)
                }
                Err(diagnostics) => Err(CliError::Message(
                    diagnostics
                        .into_iter()
                        .map(|diagnostic| diagnostic.to_string())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )),
            }
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
            let formatted = fluxc::formatter::format_source(&source).map_err(|diagnostics| {
                diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
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
            let path = require_source(&args)?;
            let generated =
                fluxc::project::compile_to_c(path).map_err(|diagnostic| diagnostic.to_string())?;
            if let Some(output) = output_path(&args[2..])? {
                fs::write(&output, generated)
                    .map_err(|error| format!("failed to write '{}': {error}", output.display()))?;
            } else {
                print!("{generated}");
            }
            Ok(())
        }
        "build" => {
            let path = require_source(&args)?;
            let generated =
                fluxc::project::compile_to_c(path).map_err(|diagnostic| diagnostic.to_string())?;
            let output = output_path(&args[2..])?.unwrap_or_else(|| default_binary_path(path));
            build_native(&generated, &output)?;
            println!("built: {}", output.display());
            Ok(())
        }
        _ => Err(CliError::Message(usage())),
    }
}

fn check_json_mode(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--json" => Ok(true),
        _ => Err("check syntax is 'check <file.flux> [--json]'".to_string()),
    }
}

fn require_source(args: &[String]) -> Result<&Path, String> {
    if args.len() < 2 {
        return Err(usage());
    }
    Ok(Path::new(&args[1]))
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
    "usage: fluxc check <file.flux> [--json] | fluxc format <file.flux> [--check] | fluxc emit-c <file.flux> [-o file.c] | fluxc build <file.flux> [-o binary]".to_string()
}
