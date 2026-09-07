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
- static checking for `i64`, `bool`, `str`, and `void`;
- function signature and argument validation;
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

## Near-term roadmap

1. Replace string diagnostics with source spans and structured diagnostics.
2. Implement explicit multi-value returns and the built-in `error` / nullable-error model.
3. Add structs, ownership moves, borrows, and the first borrow checker.
4. Introduce Flux typed IR and a direct optimizing native backend.
5. Add packages/modules and stable ABI rules.
6. Build the LSP on the same parser/type database as the compiler.
7. Define debug metadata and profiler hooks before optimizing them away.
8. Implement the flat UI grammar and grid layout engine after the core ownership/IR model is stable.

See `docs/language.md` for the evolving language specification.
