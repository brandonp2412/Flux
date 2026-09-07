# Flux language notes

This document records the current language contract and the direction that future compiler work should preserve.

## Core goals

Flux is an ahead-of-time compiled language for writing one source codebase that becomes target-native binaries. Portability is a source-language property; the runtime should not emulate another platform's widget hierarchy or ship a browser-like execution environment to make applications portable.

Performance policy: abstractions should compile away where practical, optimization should be target-aware, and platform-specific escape hatches may exist without forcing ordinary application code to fork.

Safety policy: safe Flux must prevent use-after-free, dangling references, data races caused by ordinary safe language constructs, and type confusion. Unsafe operations, if introduced, must be syntactically explicit and narrowly scoped.

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

The bootstrap representation stores only an error message. The language-level type is opaque so richer error metadata can be introduced later without turning errors into strings. A future propagation shorthand may reduce repetitive error forwarding, but it must still compile to explicit control flow rather than stack unwinding.

## Generics

Flux deliberately has no generics in the source language. This is a design constraint, not a deferred feature.

Reusable container and algorithm design will therefore rely on a combination of:

- built-in compiler-known collection types;
- interfaces/protocol-like dynamic or static dispatch where justified;
- generated/specialized standard-library implementations;
- concrete application types.

The compiler implementation may use generic implementation languages internally; that does not expose generics to Flux programs.

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

Currently implemented:

- `i64`
- `bool`
- `str`
- `error`
- `void`

`str` is currently an immutable string view/literal type in the bootstrap compiler. An owned string type will be introduced together with ownership semantics.

`error` is a compiler-known recoverable-error value. It is nullable only through the dedicated `nil` literal; `nil` does not type as an integer, boolean, string, or general null pointer.

## Control flow

Currently implemented:

- `if condition:` with an indented body;
- `for name in start..end:` with an exclusive integer range;
- `return`;
- expression statements.

Planned:

- `else` / `elif`;
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

LSP, debugger, and profiler support are mandatory product features.

The compiler should therefore maintain stable source spans and a reusable semantic query layer from early development. The debugger will need source-to-native debug metadata, expression evaluation rules, and predictable optimized-build behavior. The profiler should support low-overhead instrumentation and symbolization without requiring a separate language parser.
