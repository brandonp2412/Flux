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

## Quick start

```sh
flux new my-flux-app
flux run my-flux-app
```

## Built with Flux

<div align="center">

<img src="assets/readme/flux-mobile-showcase.png" alt="Horse Tinder dating app for horses compiled with Flux and running on Waydroid" width="360">

<sub>Real Android screenshot of the checked-in Horse Tinder <code>examples/mobile_showcase</code> app, compiled by Flux and captured from Waydroid. Reproduce it with <code>./tools/capture-readme-showcase</code>. Horse photo: <a href="https://commons.wikimedia.org/wiki/File:Funny_Horse_Faces.jpg">Funny Horse Faces</a> by Jussi You-S-See, <a href="https://creativecommons.org/licenses/by-sa/2.0/">CC BY-SA 2.0</a>.</sub>

</div>

## Why Flux

Flux combines readable function-first syntax, Rust-class safety/performance goals, direct native platform access, and a flat declarative UI model.

- Python-like control flow, comprehensions, slicing, patterns, and low ceremony.
- Strict static typing; unused bindings are compile errors.
- No classes, inheritance, mixins, source generics, exceptions, or ternaries.
- Values, functions, interfaces/capabilities, ownership, and explicit errors instead of OOP.
- Native AOT output with compiler-owned platform interop instead of app-written bridge code.
- Flat grid-first UI that targets native controls and is designed to look beautiful by default.
- Formatter, LSP, tests, diagnostics, hot-reload work, and performance gates are part of the language product.

## Current status

Flux is an active Rust bootstrap compiler. It already supports substantial typed language semantics, modules/packages, exhaustive matching, first-class functions, typed collections/pipelines, semantic CFG and typed-IR foundations, tree shaking/interface specialization, a native GTK4 Linux GUI backend, and compiler-owned Android APK/AAB generation.

Ownership/borrowing, owned strings/collections, broader platform coverage, production hot reload, the full beautiful-by-default UI system, and the long-term optimizing native backend remain roadmap work.

The classic compiler example and highlighted source are in [`examples/hello.flux`](examples/hello.flux). The native GUI dogfood path is documented in [`docs/manual-e2e.md`](docs/manual-e2e.md), testing and dependency fakes in [`docs/testing.md`](docs/testing.md), the stable machine diagnostics contract in [`docs/diagnostics.md`](docs/diagnostics.md), source debugging in [`docs/debugging.md`](docs/debugging.md), CPU profiling in [`docs/profiling.md`](docs/profiling.md), generic native cross compilation in [`docs/cross-compilation.md`](docs/cross-compilation.md), Linux distribution guidance in [`docs/linux-packaging.md`](docs/linux-packaging.md), and Google Play upload-bundle publishing in [`docs/android-release.md`](docs/android-release.md).

## Development

[`ROADMAP.MD`](ROADMAP.MD) is the single source of truth for both implementation work and persistent overall progress. [`docs/language.md`](docs/language.md) is the evolving language reference, and [`benchmarks/perf`](benchmarks/perf) contains the optimized Flux/Rust/C comparison workloads.

Run `./tools/flux-self-test` for the compiler's supported-target regression gate. It runs the complete Rust/compiler test suite, executes an optimized Linux Flux program, and builds a release Android x86_64 APK through the same user-facing toolchain used in CI.

Flux is under active development and is not yet a stable 1.0 language.
