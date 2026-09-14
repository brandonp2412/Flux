# Reproducible native builds

Flux native builds are byte-reproducible for identical inputs on the same pinned build environment. For release/CI work that must be reproduced on another machine, `flux build` can also write and later verify a versioned reproducibility record.

```sh
flux build . --mode release --locked \
  --write-reproducibility flux.repro \
  -o dist/app

flux build . --mode release --locked \
  --verify-reproducibility flux.repro \
  -o dist/app-rebuilt
```

The format-1 record is deterministic and path-independent. It pins the exact Flux compiler binary, build mode, effective target triple, generated native source, package manifest and `flux.lock` when present, the Clang and linker binaries/versions, relevant GTK/SQLite runtime versions, the host runtime identity, and a normalized allowlist of environment inputs that can affect native header/library discovery. Verification happens before native compilation and fails on the first mismatched field.

The environment identity currently covers `C_INCLUDE_PATH`, `CPATH`, `LANG`, `LC_ALL`, `LIBRARY_PATH`, `PKG_CONFIG_LIBDIR`, `PKG_CONFIG_PATH`, `PKG_CONFIG_SYSROOT_DIR`, `SOURCE_DATE_EPOCH`, and `TZ`. Cache locations and output paths are deliberately excluded because they do not define artifact contents.

## Cross-target and explicit sysroot builds

A filesystem path is not a portable SDK identity. When `--target` selects a different target from the host, or when `--sysroot` is supplied, reproducibility metadata therefore requires `FLUX_SDK_ID`:

```sh
FLUX_SDK_ID='debian-bookworm-aarch64@sha256:…' \
  flux build . \
  --target aarch64-unknown-linux-gnu \
  --sysroot /opt/sysroots/aarch64-linux-gnu \
  --write-reproducibility flux-aarch64.repro \
  -o dist/app-aarch64
```

`FLUX_SDK_ID` must name an immutable SDK/sysroot or container identity managed by the build environment, such as a package-set lock plus content digest or an OCI image digest. Flux hashes the identifier into the record and requires the same identifier during verification; it does not treat a mutable sysroot path as proof of SDK equivalence.

For an ordinary host build, Flux derives the SDK identity from the host target, `/etc/os-release`, and libc loader version, while separately pinning the exact compiler/linker/runtime identities. Package builds should use `--locked` so the verified record also pins the dependency lockfile consumed by analysis.

## Verification contract

A successful `--verify-reproducibility` means the compiler and native build inputs covered by format 1 match the recorded build environment. It does not download a toolchain or SDK, and it does not silently relax a mismatch. CI should retain the `.repro` file next to the released artifact and independently compare the rebuilt artifact bytes or release checksum when auditing a release.
