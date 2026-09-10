# Linux packaging

Flux's current Linux backend produces ordinary native executables that use GTK4 for application UI. `flux package` provides the canonical host bundle today; distro-native installers and self-contained desktop formats can wrap that bundle without changing Flux source.

## Build a release bundle

From a manifest-backed package:

```sh
flux package .
```

Release mode is the default. The output directory is versioned and host-labelled, for example `dist/my-app-1.2.3-linux-x86_64/`, and contains the native executable plus the exact `flux.toml` used for the build. When `--target <triple>` is supplied, the default artifact name uses that explicit target triple instead of the host label. Use `-o` for a deterministic staging path:

```sh
flux package . -o dist/stage
```

For a directly distributable archive, request the native tarball format:

```sh
flux package . --format tar.gz
flux package . --format tar.gz -o dist/my-app.tar.gz
```

The archive contains the same host-labelled bundle directory. Flux builds it with sorted entries, a fixed archive timestamp, and normalized numeric owner/group metadata; same-host/same-toolchain identical package inputs are regression-tested to produce byte-identical `.tar.gz` files. `tar` is only required when this archive format is requested.

For a headless `fn main() -> i64` service, Flux can also emit a ready-to-build container context:

```sh
flux package . --format container
podman build -f dist/my-service-1.2.3-linux-x86_64-container/Containerfile dist/my-service-1.2.3-linux-x86_64-container
```

The context contains the optimized native executable as `app` plus a minimal `Containerfile` that copies it into `debian:stable-slim` and uses it as the container entry point. Flux deliberately emits a build context rather than invoking Docker or Podman, so CI can choose its container engine, registry, platform flags, and image metadata explicitly. GUI `app` declarations are rejected for this format; container packaging is the headless/server path.

Current Linux headless packages can also request one statically linked native executable:

```sh
flux package . --format static
flux package . --format static -o dist/my-service
```

`--format static` passes static linkage through the same optimized native backend and keeps static and dynamically linked artifacts in separate build-cache identities. It is intentionally headless-only because the current GTK application backend is a host-runtime dependency. The selected Clang target/sysroot must provide a static C runtime; when it does not, packaging fails explicitly instead of silently producing a dynamically linked binary. This makes the format self-contained with respect to the C runtime on supported Linux toolchains, while application-owned files, certificates, databases, and other runtime data remain ordinary deployment inputs.

`flux package` refuses to overwrite an existing output directory, archive, or static executable. Build automation should remove or version old artifacts explicitly instead of relying on implicit replacement.

## Runtime dependencies

The current GUI backend links to the host GTK4 stack. A Flux Linux bundle is therefore not claimed to be a fully static or distribution-independent artifact. Package GTK4 and its normal transitive platform dependencies using the target distribution's dependency mechanism. Headless Flux programs do not require GTK merely because the compiler supports GUI applications; unused UI/runtime support is omitted from generated native output.

Use `flux doctor` on the build host to verify the native compiler and GTK development environment before packaging. Release binaries use the same optimized native build path as `flux build --mode release`.

## Filesystem integration

For conventional system-wide packaging, install the executable under `/usr/bin` or `/usr/libexec/<app>` according to the distribution's policy. Per-user installers should prefer the XDG user locations rather than writing system directories without the package manager.

A graphical application can provide a standard freedesktop desktop entry under `share/applications` and icons under the matching `share/icons/hicolor/<size>x<size>/apps` directories. Use the same stable application identity configured by the package/application metadata when choosing desktop-entry and icon names. Flux does not currently synthesize `.desktop` files or icon pyramids; those are packaging inputs owned by the application until resource/desktop-metadata support lands in the compiler.

## Archives and distro packages

The `flux package` directory remains the staging boundary for distro-specific Linux tooling, while `--format tar.gz` supplies the portable archive shape directly. The directory bundle can be used as input to deb/rpm/Arch/AppImage/Flatpak packaging. Flux does not currently pretend that one distro/container format is universally correct, and it does not silently vendor GTK into the bundle.

For distro packages, declare GTK4 through the distro's dependency metadata instead of copying shared libraries beside the executable. For sandboxed formats such as Flatpak, select a runtime that supplies a compatible GTK stack and keep application permissions explicit in the packaging manifest.

## Reproducibility and symbols

Current same-host/same-toolchain debug, profile, and release builds are regression-tested for byte reproducibility, as are current `.tar.gz` package archives. Generic native builds and packages can select an explicit Clang target/sysroot; see [`cross-compilation.md`](cross-compilation.md). Native cache keys include Clang/linker identity and GTK version/flags, and cached binaries are validated against sidecar size/content hashes before reuse. `flux doctor` prints the active Clang, linker, and GTK versions so release jobs can retain those identities with the exact Flux revision and `flux.toml` used for a build.

Reproducible release infrastructure should still pin the Flux compiler revision and native toolchain, isolate or pin environment inputs, and retain the exact `flux.toml` shipped in the bundle. Full sysroot/SDK content fingerprinting and cross-machine/toolchain reproducibility remain separate roadmap work. For detached native debug information and crash address resolution, see [`debugging.md`](debugging.md).

## Current boundary

Flux currently automates native compilation, the host directory bundle, reproducible host `.tar.gz` archives, headless container build contexts, and static headless executables when the selected Linux toolchain supplies static runtime libraries. It does not yet automate distro repository publication, AppImage/Flatpak construction, deb/rpm metadata, desktop-file/icon generation, signing, registry publication, or package-manager upload. Those steps should remain explicit downstream packaging operations rather than hidden compiler side effects until dedicated target support is implemented.
