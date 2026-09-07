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
- A flat, HTML-like declarative UI surface with grid-first layout rather than deeply nested widget trees.
- Tooling is part of the language product: LSP, debugger, and profiler are first-class deliverables.

## Current bootstrap milestone

The repository currently contains a dependency-free Rust bootstrap compiler with:

- parsing for functions, structs, typed bindings, calls, field access, `if` / `elif` / `else`, and exclusive range `for` loops;
- explicit multi-value function returns and strictly typed destructuring bindings;
- static checking for `i64`, `bool`, `str`, `error`, `void`, and named struct value types;
- struct literals with exact field validation, nested value layouts, and native by-value lowering;
- function signature, return, and argument validation;
- structured parse/type/codegen diagnostics with stable source IDs, reusable source-span metadata, and safe multi-error parser/type-checker recovery;
- parser recovery that reports syntax errors from later malformed functions instead of stopping at the first one;
- a native bootstrap backend that emits C and invokes Clang with optimization enabled;
- checked integer division at runtime;
- CLI commands for checking, deterministic formatting, emitting C, and building a native executable;
- compiler tests and runnable native examples, including nested structs.

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

Check without producing a binary:

```sh
cargo run -- check examples/hello.flux
```

Format source deterministically, or verify canonical formatting in CI:

```sh
cargo run -- format examples/hello.flux
cargo run -- format examples/hello.flux --check
```

Tooling/CI can request structured diagnostics without parsing human text:

```sh
cargo run -- check examples/hello.flux --json
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
