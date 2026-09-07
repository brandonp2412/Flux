use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fluxc: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err(usage());
    }

    match args[0].as_str() {
        "check" => {
            let path = require_source(&args)?;
            let source = fs::read_to_string(path)
                .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
            fluxc::check_source(&source).map_err(|diagnostic| diagnostic.to_string())?;
            println!("ok: {}", path.display());
            Ok(())
        }
        "emit-c" => {
            let path = require_source(&args)?;
            let source = fs::read_to_string(path)
                .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
            let generated =
                fluxc::compile_to_c(&source).map_err(|diagnostic| diagnostic.to_string())?;
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
            let source = fs::read_to_string(path)
                .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
            let generated =
                fluxc::compile_to_c(&source).map_err(|diagnostic| diagnostic.to_string())?;
            let output = output_path(&args[2..])?.unwrap_or_else(|| default_binary_path(path));
            build_native(&generated, &output)?;
            println!("built: {}", output.display());
            Ok(())
        }
        _ => Err(usage()),
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
    "usage: fluxc check <file.flux> | fluxc emit-c <file.flux> [-o file.c] | fluxc build <file.flux> [-o binary]".to_string()
}
