<div align="center">

# Flux

### Python-like syntax. Rust-class performance. Native apps everywhere.

[![Performance](https://github.com/brandonp2412/Flux/actions/workflows/performance.yml/badge.svg)](https://github.com/brandonp2412/Flux/actions/workflows/performance.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Flux is a statically typed, ahead-of-time compiled language for native software without classes, a VM, or a mandatory framework runtime.**

</div>

## Goals

Flux combines readable function-first syntax with native compilation and a flat UI model. It aims for Rust-class safety/performance while keeping ordinary application code much less annotation-heavy.

- Python-like control flow, comprehensions, slicing, patterns, and low ceremony.
- Strict static typing; unused bindings are errors.
- No classes, inheritance, mixins, source generics, exceptions, or ternaries.
- Explicit errors and multi-value returns.
- Values, functions, interfaces/capabilities, and ownership instead of OOP.
- Flat grid-first UI compiled to native platform controls/backends.
- Compiler-owned platform interop rather than application-written bridge code.
- First-class formatter, LSP, testing, hot-reload work, diagnostics, and performance gates.

## Current status

Flux is an active bootstrap compiler written in Rust. The current implementation includes structs/enums, exhaustive matching, first-class and anonymous capture-free functions, modules/packages, constants, explicit mutation, typed list operations and pipelines, semantic CFG/typed-IR foundations, tree shaking/interface specialization, native Linux GUI output, and a compiler-owned Android backend.

Linux currently lowers flat Flux views to native GTK4 controls. Android builds native APK/AAB artifacts with compiler-generated Activity/JNI glue, native controls, state refresh, input/focus handling, permissions, notifications, intents, and other direct platform APIs. Application Flux code does not contain Java/Kotlin/JNI bridge boilerplate.

The C backend is a bootstrap path rather than the intended final semantic backend. Ownership/borrowing, owned strings/collections, broader platform support, production hot reload, and the long-term optimizing native backend remain major roadmap work.

## Quick start

```sh
./tools/flux new my-flux-app
./tools/flux run my-flux-app
```

Build, test, check, format, or start the language server:

```sh
./tools/flux build examples/hello.flux -o hello
./tools/flux test examples/package
./tools/flux check examples/hello.flux
./tools/flux format examples/hello.flux
./tools/flux lsp
```

Android example:

```sh
./tools/flux build android examples/android_app --mode debug --abi arm64-v8a
./tools/flux run android examples/android_app --abi arm64-v8a
./tools/flux build android examples/android_app --mode release --format aab
```

## Example

![Flux example with Flux-aware syntax highlighting](assets/readme/flux-hello.png)

[View the source](examples/hello.flux)

The native GUI dogfood example is [`examples/hello_app.flux`](examples/hello_app.flux). The editor → compile → native window → interaction → save/rebuild acceptance path is documented in [`docs/manual-e2e.md`](docs/manual-e2e.md).

## Development

- [`ROADMAP.MD`](ROADMAP.MD) is the source of truth for planned and completed work.
- [`PROGRESS.md`](PROGRESS.md) contains the persistent overall progress baseline used across development sessions.
- [`docs/language.md`](docs/language.md) is the evolving language reference.
- [`benchmarks/perf`](benchmarks/perf) contains optimized Flux/Rust/C comparison workloads; run `./tools/flux-bench` to execute them.

Flux is under active development and is not yet a stable 1.0 language.
