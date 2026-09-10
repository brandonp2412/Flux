# Cross compilation

Flux native builds can pass an explicit Clang target triple and sysroot through the compiler-owned build pipeline. This keeps target selection in the Flux CLI while still using the current bootstrap C/Clang backend.

## Build for a native target

Use `--target` with any Clang-compatible target triple:

```sh
flux build app.flux --target aarch64-unknown-linux-gnu -o app-aarch64
```

When the target needs headers, libraries, or a linker environment that are not part of the host toolchain, point Flux at a prepared target sysroot:

```sh
flux build app.flux \
  --target aarch64-unknown-linux-gnu \
  --sysroot /opt/sysroots/aarch64-linux-gnu \
  -o app-aarch64
```

`--sysroot` must name an existing directory. Flux forwards the normalized target selection directly to Clang as `--target=<triple>` and `--sysroot=<directory>`; it does not emulate the target, download SDKs, or silently invent platform libraries.

The same options are accepted by manifest-backed packaging:

```sh
flux package . \
  --target aarch64-unknown-linux-gnu \
  --sysroot /opt/sysroots/aarch64-linux-gnu \
  --format tar.gz
```

Default package artifact names include the explicit target triple, so a cross-target package is distinguishable from the normal host-labelled bundle.

## What Flux does and does not provide

Cross compilation selects the native code-generation target. The selected Clang installation still needs a usable toolchain for that target: compatible headers, libraries, startup objects, linker, and any platform SDK required by the generated program. A headless program may need only a conventional libc/sysroot; a GUI program additionally needs the native UI dependencies for the selected target.

Android remains a separate first-class target command (`flux build android ...`) because Android builds require the Android SDK/NDK, generated Activity/JNI glue, ABI selection, packaging, and signing behavior that do not map cleanly to a generic Clang triple.

`flux run`, `flux test`, `flux debug`, and `flux profile` execute binaries on the development host and therefore intentionally do not accept generic cross-target execution options. Build the cross-target artifact with `flux build` or `flux package`, then deploy/run it using the target environment's normal tooling.

## Build cache behavior

The native build cache includes the target triple and sysroot path in its cache identity. It also fingerprints the active Clang and linker identity, and GTK-backed builds include the resolved GTK version plus compiler/linker flags. Artifacts produced for a different configured target, toolchain, or GTK configuration are therefore not reused accidentally. Cached binaries carry sidecar size/content hashes; a missing or mismatched sidecar causes Flux to discard the entry and rebuild it instead of restoring potentially corrupt output.

Full sysroot/SDK content fingerprinting and cross-machine reproducibility remain separate roadmap work. Changing files inside a sysroot without changing its path is not yet sufficient to invalidate an existing cache entry automatically. `flux doctor` reports the active Clang, linker, and GTK versions so release infrastructure can record the native toolchain identity alongside its pinned Flux revision.

For same-host packaging details, see [`linux-packaging.md`](linux-packaging.md). For Android release artifacts, see [`android-release.md`](android-release.md).
