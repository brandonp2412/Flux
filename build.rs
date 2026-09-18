use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_sources(directory: &Path, paths: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        .map(|entry| {
            entry
                .expect("compiler source directory entry should be readable")
                .path()
        })
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_rust_sources(&path, paths);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            paths.push(path);
        }
    }
}

fn add_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x100000001b3);
}

fn main() {
    let mut sources = Vec::new();
    collect_rust_sources(Path::new("src"), &mut sources);

    let mut hash = 0xcbf29ce484222325u64;
    for path in sources {
        println!("cargo:rerun-if-changed={}", path.display());
        add_bytes(&mut hash, path.to_string_lossy().as_bytes());
        let source = fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        add_bytes(&mut hash, &source);
    }

    println!("cargo:rustc-env=FLUX_COMPILER_SOURCE_FINGERPRINT={hash:016x}");
}
