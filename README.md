# Flux

Flux is an experimental compiled language for building native applications and services from one codebase without a widget-emulation runtime. The long-term target is maximally optimized platform-native binaries for desktop, server, and other supported targets.

## Language direction

- Python-like readability and low ceremony.
- Function bodies are delimited by `{}`.
- Control-flow bodies use indentation (`if`, `for`, etc.).
- Strict static typing; implicit type coercions are deliberately minimized.
- Function-first architecture built from data, functions, and interfaces rather than classes, inheritance, mixins, or widget/controller object hierarchies.
- No generics in the Flux language.
- No exception / try-catch model. Recoverable failures are represented explicitly in return values.
- Rust-like memory-safety goals: ownership, borrowing, lifetime validation, and no unchecked dangling references in safe Flux.
- Native ahead-of-time compilation.
- A flat, HTML-like declarative UI surface with grid-first layout rather than deeply nested widget trees; the bootstrap parser now supports top-level `view` declarations with sibling grid placement syntax.
- Tooling is part of the language product: LSP, debugger, and profiler are first-class deliverables.

## Current bootstrap milestone

The repository currently contains a dependency-free Rust bootstrap compiler with:

- parsing for functions, structs, closed payload enums, typed bindings, calls, field access, `if` / `elif` / `else`, exclusive range `for` loops, indentation-based `while` loops, and statically scoped `break` / `continue`;
- immutable `let` bindings by default plus explicit typed local mutation through `var` and statically checked assignment;
- explicit multi-value function returns and strictly typed destructuring bindings;
- static checking for `i64`, `bool`, `str`, `error`, `void`, named struct value types, transparent concrete type aliases, and compile-time constants;
- struct literals, field access, `Type { ..base, field: value }` functional updates, and struct destructuring patterns with inferred field types and single-evaluation native lowering;
- namespace-qualified enum construction such as `Outcome.Ok(42)`, with typed payload validation and native tag/union representation;
- exhaustive enum `match` statements with typed payload bindings, guaranteed-return analysis, and single-evaluation native `switch` lowering;
- positional and named-only function parameters with required named arguments, compile-time defaults, and zero-runtime-overhead call reordering;
- first-class named function values and concrete `fn(...) -> ...` function types, lowered to typed native function pointers without callable objects;
- function signature, return, and argument validation;
- structured parse/type/codegen diagnostics with stable source IDs, reusable source-span metadata, and safe multi-error parser/type-checker recovery;
- terminal diagnostics that show the offending source, exact carets, related declaration labels and suggested fixes, automatically colorize interactive terminals, and wrap/crop to the current terminal width;
- parser recovery that reports syntax errors from later malformed functions instead of stopping at the first one;
- a native bootstrap backend that emits C and invokes Clang with optimization enabled;
- checked integer division at runtime;
- CLI commands for checking, deterministic formatting, emitting C, building native executables, automatically running/rebuilding development targets, and serving bootstrap LSP diagnostics over stdio, including package-root/`flux.toml` targets;
- compile-time constant folding for `i64`, `bool`, and `str`, including forward references and short-circuit boolean expressions with no runtime global storage;
- compiler tests and runnable native examples, including nested structs, struct destructuring, zero-cost type aliases, folded constants, payload enums, exhaustive matching, named/default parameters, higher-order functions, explicit mutation/`while`, flat-grid UI syntax, typed built-in UI properties, and parameterized view composition.

The C backend is a bootstrap implementation, not the final backend architecture. The intended next backend milestone is a direct typed IR suitable for LLVM-class optimization and target-specific lowering.

## Example

```flux
fn square(value: i64) -> i64 {
    return value * value
}

fn main() -> i64 {
    for i in 0..5:
        let squared: i64 = square(i)
        print(squared)
    return 0
}
```

Build it:

```sh
cargo run -- build examples/hello.flux -o hello
./hello
```

Run in development mode with automatic save detection:

```sh
cargo run -- run examples/hello.flux
```

The bootstrap runner watches imported Flux modules automatically, debounces rapid saves, recompiles on change, and restarts only after a successful replacement build. Compiler errors keep the last good process untouched and the watcher remains active until the next save. State-preserving hot apply is a later development-ABI milestone; the current runner is the controlled-restart foundation for it.

Check without producing a binary:

```sh
cargo run -- check examples/hello.flux
```

Human diagnostics use the terminal width (`COLUMNS` when supplied, otherwise the interactive terminal width) and enable ANSI color only for an appropriate terminal. `NO_COLOR` disables color; `FORCE_COLOR=1` can force it. Long source lines and paths are cropped around the relevant span instead of overflowing, while diagnostic messages, labels, notes, and fixes wrap to fit.

Format source deterministically, or verify canonical formatting in CI:

```sh
cargo run -- format examples/hello.flux
cargo run -- format examples/hello.flux --check
```

Editors can launch the bootstrap language server over stdio:

```sh
cargo run -- lsp
```

The current LSP slice publishes parse/type diagnostics using the same source spans, labels, and fix metadata as the compiler, including real import-graph type checking with every open unsaved Flux buffer substituted as an in-memory project overlay. It also serves canonical whole-document formatting, machine-applicable quick fixes, safe declaration/unambiguous-symbol hover and go-to-definition, dependency-graph references/rename, ordinary-function signature help, full-document semantic highlighting, and completion for keywords/builtins/current-module plus public forward-import declarations and lexically visible local bindings. Imported ordinary-function completion, hover, and signature help use unsaved overlays as well. Ambiguous/shadowed navigation and rename are deliberately refused rather than guessed. Shared cached workspace analysis, reverse-dependent discovery, richer member/property completion, fully resolved shadow-aware usage navigation, and interface/view signature help remain later LSP milestones.

Tooling/CI can request structured diagnostics without parsing human text:

```sh
cargo run -- check examples/hello.flux --json
```

Packages can select their entry source with `flux.toml`:

```toml
[package]
name = "package-example"
version = "0.1.0"
entry = "src/main.flux"
```

The package directory or manifest can then be passed directly to project-aware commands:

```sh
cargo run -- check examples/package
cargo run -- build examples/package -o package-example
```

## Multi-value returns

Flux keeps multi-values explicit and does not make general tuple values part of ordinary expressions. Functions may declare multiple return values and callers destructure them into explicitly typed bindings:

```flux
fn divide(value: i64, by: i64) -> (i64, bool) {
    if by == 0:
        return 0, false
    return value / by, true
}

fn main() -> i64 {
    let result: i64, ok: bool = divide(84, 2)
    return 0
}
```

This is the foundation for Flux's recoverable-error model. Flux now has a built-in `error` type: `nil` means no error, while `error("message")` constructs a recoverable failure. `error` is compiler-known rather than a generic result container, so the model stays explicit and compatible with Flux's no-generics constraint. Exceptions are not part of the design. Multi-value results can also be forwarded directly when the caller returns the exact same shape, so wrappers stay concise without hidden control flow:

```flux
fn load_config(path: str) -> (str, error) {
    return load(path)
}
```

Forwarding is positional and strictly checked: both arity and every return type must match the enclosing function signature.

When a function needs the successful values before returning, Flux provides a narrow explicit propagation form:

```flux
fn load_config(path: str) -> (str, error) {
    let data: str, err: error = load(path) else return
    print(data)
    return data, nil
}
```

`else return` is only valid on a multi-value destructuring binding when the final value is `error` and the called function's complete return shape exactly matches the enclosing function. It evaluates the call once, returns that exact result when the error is non-`nil`, and otherwise continues with the destructured bindings. It is ordinary control flow, not exception handling or stack unwinding.

```flux
fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "configuration loaded", nil
}

fn main() -> i64 {
    let data: str, err: error = load("settings.flux")
    if err != nil:
        print(err)
    print(data)
    return 0
}
```

## Roadmap

`ROADMAP.MD` is the source of truth for Flux development and is updated at the start of every development session. It covers the complete language/compiler plan, Dart-inspired non-OOP ergonomics, ownership, native UI, Android/iOS/desktop/web/server targets, automatic save-triggered hot reload, testing, LSP, debugger, profiler, packaging, and Flux 1.0 criteria.

See `ROADMAP.MD` for planned work and `docs/language.md` for the evolving language specification.
