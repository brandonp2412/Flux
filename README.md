# Flux

Flux is an experimental compiled language for building native applications and services from one codebase without a widget-emulation runtime. The long-term target is maximally optimized platform-native binaries for desktop, server, and other supported targets.

## Language direction

- Python-like readability and low ceremony.
- Function bodies are delimited by `{}`.
- Control-flow bodies use indentation (`if`, `for`, etc.).
- Strict static typing; implicit type coercions are deliberately minimized.
- No generics in the Flux language.
- No exception / try-catch model. Recoverable failures are represented explicitly in return values.
- Rust-like memory-safety goals: ownership, borrowing, lifetime validation, and no unchecked dangling references in safe Flux.
- Native ahead-of-time compilation.
- A flat, HTML-like declarative UI surface with grid-first layout rather than deeply nested widget trees.
- Tooling is part of the language product: LSP, debugger, and profiler are first-class deliverables.

## Current bootstrap milestone

The repository currently contains a dependency-free Rust bootstrap compiler with:

- parsing for functions, typed bindings, calls, `if`, and exclusive range `for` loops;
- explicit multi-value function returns and strictly typed destructuring bindings;
- static checking for `i64`, `bool`, `str`, `error`, and `void`;
- function signature, return, and argument validation;
- structured parse/type/codegen diagnostics with reusable source-span metadata;
- a native bootstrap backend that emits C and invokes Clang with optimization enabled;
- checked integer division at runtime;
- CLI commands for checking, emitting C, and building a native executable;
- compiler tests and a runnable example.

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

## Near-term roadmap

1. Refine diagnostics to token-level spans and add multi-diagnostic recovery for editor tooling.
2. Add `else` / `elif` and a small explicit error-propagation shorthand that lowers to ordinary control flow.
3. Add structs, ownership moves, borrows, and the first borrow checker.
4. Introduce Flux typed IR and a direct optimizing native backend.
5. Add packages/modules and stable ABI rules.
6. Build the LSP on the same parser/type database as the compiler.
7. Define debug metadata and profiler hooks before optimizing them away.
8. Implement the flat UI grammar and grid layout engine after the core ownership/IR model is stable.

See `docs/language.md` for the evolving language specification.
