<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/readme/flux-logo-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="assets/readme/flux-logo.svg">
  <img src="assets/readme/flux-logo.svg" alt="Flux" width="420">
</picture>

### Python-like syntax. Rust-class performance. Native apps everywhere.

[![Performance](https://github.com/brandonp2412/Flux/actions/workflows/performance.yml/badge.svg)](https://github.com/brandonp2412/Flux/actions/workflows/performance.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Flux is a statically typed, ahead-of-time compiled language for native software without classes, a VM, or a mandatory framework runtime.**

</div>

## Start a Flux project on Linux

Install the compiler and VS Code extension from a Flux checkout:

```sh
git clone https://github.com/brandonp2412/Flux.git
cd Flux
cargo install --path . --locked
./tools/flux-vscode install
flux doctor
```

Create and open an app:

```sh
cd ..
flux new hello-flux
code hello-flux
```

Run **Flux: Run** from the VS Code Command Palette, or use:

```sh
cd hello-flux
flux run .
```

The extension uses `flux lsp` for diagnostics, completion, hover, navigation, rename, formatting, fixes, signature help, inlay hints, and semantic highlighting. If the `code` CLI is unavailable, run `./tools/flux-vscode package` and install the generated `.vsix` from VS Code.

## Status

Flux already builds native GTK4 Linux apps and Android APK/AABs, with the compiler, LSP, debugger, profiler, tests, package tooling, and development runner under active development. It is not yet a stable 1.0 language.

See [`ROADMAP.MD`](ROADMAP.MD), [`docs/language.md`](docs/language.md), and [`docs/manual-e2e.md`](docs/manual-e2e.md).
