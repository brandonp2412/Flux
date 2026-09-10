# Linux packaging

Flux's current Linux backend produces ordinary native executables that use GTK4 for application UI. `flux package` provides the canonical host bundle today; distro-native installers and self-contained desktop formats can wrap that bundle without changing Flux source.

## Build a release bundle

From a manifest-backed package:

```sh
flux package .
```

Release mode is the default. The output directory is versioned and host-labelled, for example `dist/my-app-1.2.3-linux-x86_64/`, and contains the native executable plus the exact `flux.toml` used for the build. Use `-o` for a deterministic staging path:

```sh
flux package . -o dist/stage
```

For a directly distributable archive, request the native tarball format:

```sh
flux package . --format tar.gz
flux package . --format tar.gz -o dist/my-app.tar.gz
```

The archive contains the same host-labelled bundle directory. Flux builds it with sorted entries, a fixed archive timestamp, and normalized numeric owner/group metadata; same-host/same-toolchain identical package inputs are regression-tested to produce byte-identical `.tar.gz` files. `tar` is only required when this archive format is requested.

`flux package` refuses to overwrite an existing output directory or archive. Build automation should remove or version old artifacts explicitly instead of relying on implicit replacement.

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

Current same-host/same-toolchain debug, profile, and release builds are regression-tested for byte reproducibility, as are current `.tar.gz` package archives. Reproducible release infrastructure should pin the Flux compiler revision and native toolchain, isolate or pin environment inputs, and retain the exact `flux.toml` shipped in the bundle. Cross-machine/toolchain reproducibility remains separate roadmap work. For detached native debug information and crash address resolution, see [`debugging.md`](debugging.md).

## Current boundary

Flux currently automates native compilation, the host directory bundle, and reproducible host `.tar.gz` archives. It does not yet automate distro repository publication, AppImage/Flatpak construction, deb/rpm metadata, desktop-file/icon generation, signing, or package-manager upload. Those steps should remain explicit downstream packaging operations rather than hidden compiler side effects until dedicated target support is implemented.
