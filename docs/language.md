# Flux language notes

This document records the current language contract and the direction that future compiler work should preserve.

## Core goals

Flux is an ahead-of-time compiled language for writing one source codebase that becomes target-native binaries. Portability is a source-language property; the runtime should not emulate another platform's widget hierarchy or ship a browser-like execution environment to make applications portable.

Performance policy: abstractions should compile away where practical, optimization should be target-aware, and platform-specific escape hatches may exist without forcing ordinary application code to fork.

Safety policy: safe Flux must prevent use-after-free, dangling references, data races caused by ordinary safe language constructs, and type confusion. Unsafe operations, if introduced, must be syntactically explicit and narrowly scoped.

Abstraction policy: Flux is function-first. Programs are composed from data, functions, and interfaces/capabilities. Flux does not use classes, inheritance, mixins, widget subclasses, controller hierarchies, or constructor-oriented architecture as its abstraction model.

## Syntax

Functions use braces:

```flux
fn add(a: i64, b: i64) -> i64 {
    return a + b
}
```

Control flow uses indentation:

```flux
fn main() -> i64 {
    for i in 0..10:
        if i < 5:
            print(i)
    return 0
}
```

Tabs are not valid indentation. Blocks must use consistent indentation at each nesting level.

Bindings are explicitly typed:

```flux
let count: i64 = 4
let enabled: bool = true
let label: str = "Flux"
```

The bootstrap compiler intentionally performs no implicit `bool`/integer/string conversions.

## Struct values

Flux structs are plain named value types. They contain data only: no constructors, methods, inheritance, object identity, or hidden heap allocation.

```flux
struct User {
    name: str
    age: i64
}

fn birthday(user: User) -> User {
    return User { name: user.name, age: user.age + 1 }
}
```

Struct literals must provide each declared field exactly once with the declared type. Unknown fields, missing fields, duplicate fields, and unknown named types are compile-time errors. Field access is statically resolved. Structs may contain other structs by value and are emitted in dependency order; recursive by-value layouts are rejected because they have infinite size. Indirect recursive structures will be introduced only together with explicit ownership/reference semantics.

Struct values lower to native value structs in the bootstrap C backend. Copy/update sugar is not implemented yet; when introduced it must evaluate its source value once and preserve the same explicit value semantics.

## Error handling

Flux does not use exceptions or `try` / `catch` for recoverable failures.

The model uses explicit multiple return values, similar in spirit to Go but strictly typed and integrated with Flux ownership. Multi-values are deliberately not general-purpose tuple values. A function returning multiple values must be consumed by a destructuring binding, and every binding type is checked positionally at compile time. This keeps the feature narrow, predictable, and easy to lower efficiently.

Recoverable failures use the compiler-known `error` type. `nil` is the only no-error value, and `error("message")` constructs an error. `error` is intentionally not a generic `Result<T, E>` or general optional type; Flux has no generics, and normal error handling stays explicit in function signatures and control flow:

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

The bootstrap representation stores only an error message. The language-level type is opaque so richer error metadata can be introduced later without turning errors into strings.

A function may directly forward another function's multi-value result with `return call(...)` when the complete return shape matches exactly. This is compile-time checked positionally and lowers to ordinary native return values; it does not introduce exceptions, stack unwinding, or general tuple values.

When successful values are needed before the enclosing function returns, a destructuring binding may use `else return`:

```flux
fn load_config(path: str) -> (str, error) {
    let data: str, err: error = load(path) else return
    print(data)
    return data, nil
}
```

This form is intentionally narrow. The final destructured value must have type `error`, and the call's full multi-value return shape must exactly match the enclosing function. The call is evaluated once. If the final error value is non-`nil`, the exact multi-value result is returned immediately; otherwise the values are destructured and execution continues. This is explicit control flow rather than exception propagation.

## Generics

Flux deliberately has no generics in the source language. This is a design constraint, not a deferred feature.

Reusable container and algorithm design will therefore rely on a combination of:

- built-in compiler-known collection types;
- interfaces/protocol-like dynamic or static dispatch where justified;
- generated/specialized standard-library implementations;
- concrete application types.

The compiler implementation may use generic implementation languages internally; that does not expose generics to Flux programs.

## Dart-inspired ergonomics direction

Flux should borrow Dart's strongest expression and collection ergonomics without importing Dart's class-oriented object model. Planned features include optional types, null/optional-aware access and indexing, coalescing, `first`/`last` and related sequence conveniences, cascades that evaluate their target once, collection spreads and collection-level `if`/`for`, records, patterns/destructuring, named/default parameters, concise functions, string interpolation, and async/generator ergonomics.

These features must remain compatible with Flux's no-generics rule, explicit recoverable-error returns, ownership model, and function/interface architecture. Property-like conveniences on compiler-known collection types are syntax sugar and do not imply an object hierarchy.

The detailed adoption/rejection checklist lives in `ROADMAP.MD`.

## Memory model direction

The planned safe memory model borrows the useful properties of Rust while aiming for less surface syntax in application code:

- values have one owning location unless the type is explicitly copyable;
- assignment/pass-by-value moves non-copy values by default;
- immutable borrows may coexist;
- mutable borrows are exclusive;
- borrows may not outlive their owner;
- compiler lifetime reasoning should be mostly inferred;
- reference-counting is not the default ownership model;
- tracing garbage collection is not required for ordinary safe code.

The first borrow checker should be implemented after structs and a typed intermediate representation exist, so ownership rules are checked over normalized semantics rather than parser syntax.

## Primitive bootstrap types

Currently implemented primitives:

- `i64`
- `bool`
- `str`
- `error`
- `void`

Programs may additionally declare named `struct` value types.

`str` is currently an immutable string view/literal type in the bootstrap compiler. An owned string type will be introduced together with ownership semantics.

`error` is a compiler-known recoverable-error value. It is nullable only through the dedicated `nil` literal; `nil` does not type as an integer, boolean, string, or general null pointer.

## Control flow

Currently implemented:

- `if condition:` with an indented body;
- `elif condition:` and `else:` attached to the preceding conditional chain;
- exhaustive `if` / `elif` / `else` return analysis, so a fully returning chain satisfies a function's return requirement;
- `for name in start..end:` with an exclusive integer range;
- `return`;
- expression statements.

Planned:

- `while`;
- `break` / `continue`;
- exhaustive `match` for closed enum-like types.

## Native compilation architecture

Bootstrap pipeline:

```text
Flux source
  -> parser / AST
  -> static type checker
  -> bootstrap C lowering
  -> Clang optimizer + native linker
  -> native binary
```

Target architecture:

```text
Flux source
  -> parser
  -> semantic/type/ownership database
  -> typed Flux IR
  -> optimization passes
  -> target lowering
  -> native object/link
```

The typed IR must retain source spans, ownership facts, and debug locations so the same semantic model can support the compiler, LSP, debugger, and profiler.

## UI direction

Flux UI syntax should be declarative and HTML-like, but layout should be flat and grid-first. Deep widget nesting should not be the normal way to express placement.

Conceptual direction:

```flux
view Dashboard {
    grid columns: 240px 1fr, rows: auto 1fr

    Sidebar at 1 / 1
    Header  at 2 / 1
    Content at 2 / 2
}
```

The layout engine should resolve a flat element set against explicit grid coordinates/areas. Components may still compose reusable content, but composition should not force layout to become a deeply nested ownership tree.

The UI subsystem is intentionally downstream of the core type/ownership/IR work: it should compile to platform-native rendering and input backends rather than define the compiler architecture around one UI toolkit.

## Tooling contract

LSP, debugger, and profiler support are mandatory product features. Development hot reload is also a first-class requirement: `flux run` should watch source files automatically and apply compatible changes on file save, preserving compatible state and falling back to a controlled restart only when necessary. The normal workflow must not require a manual hot-reload key.

Compiler failures are represented as structured diagnostics rather than raw strings. A diagnostic records its compilation stage (`parse`, `type`, or `codegen`), message, and an optional source span. Every span also carries a stable `SourceId`, allowing editor/workspace tooling to distinguish identical line/column locations in different files; callers may provide their own IDs, while deterministic name-derived IDs are available for CLI/file workflows. Diagnostics can carry secondary labels, notes, and machine-applicable replacement fixes. Function and statement AST nodes retain source spans, while expression nodes carry token-level line/column/length spans assembled through unary, binary, call, and parenthesized expressions. Parser recovery synchronizes across malformed functions, and type checking safely continues across independent statements/functions while preserving declared binding types after bad initializers to avoid misleading cascades. `check_source_all` exposes the complete recovered diagnostic batch, `fluxc check` renders it for humans, and `fluxc check <file> --json` emits clean structured JSON for LSP/CI consumers. This diagnostic model is intended to be reused directly by editor tooling instead of reparsing CLI text.

The compiler therefore maintains stable source spans and a reusable semantic query layer from early development. Editor parse snapshots reuse unchanged source by fingerprint, and the semantic database indexes analyzed function, parameter, binding, loop-variable, and signature information for compiler/LSP consumers. Flux formatting is deterministic and comment-preserving; `fluxc format --check` provides a non-mutating CI contract for canonical source. The debugger will need source-to-native debug metadata, expression evaluation rules, and predictable optimized-build behavior. The profiler should support low-overhead instrumentation and symbolization without requiring a separate language parser.
