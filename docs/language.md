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

Struct values lower to native value structs in the bootstrap C backend. Functional updates use `Type { ..base, field: value }`. The base may be any expression, is evaluated exactly once, and is copied by value before the listed fields are replaced. The bootstrap backend lowers update shapes through generated typed inline helpers rather than relying on non-standard C expression extensions.

Structs can also be destructured into inferred, statically typed field bindings:

```flux
let User { name, age: years } = load_user()
```

Shorthand fields bind to the same local name; `field: local` renames the binding and `field: _` ignores a field. The pattern may select only the fields it needs. The source expression is evaluated exactly once, aliases of the struct type are accepted, unknown fields and wrong source types are compile-time errors, and pattern bindings may not silently shadow an existing local.

## Function parameters

Flux keeps ordinary positional parameters simple while supporting explicit named-only APIs. A `*` in the parameter list marks every following parameter as named-only:

```flux
const DEFAULT_COUNT: i64 = 3

fn describe(prefix: str, suffix: str = "!", *, count: i64 = DEFAULT_COUNT, label: str) -> i64 {
    return count
}

fn main() -> i64 {
    return describe("hello", label: "world")
}
```

Parameters before `*` are positional-only. Trailing positional parameters and named-only parameters may provide defaults with `= expression`; a named-only parameter without a default is required. Required positional parameters may not follow a positional parameter with a default. Calls place positional arguments first and use `name: value` for named arguments. Positional arguments cannot follow named arguments, duplicate/unknown named arguments are rejected, and positional-only parameters cannot be supplied by name.

Current defaults are compile-time primitive expressions (`i64`, `bool`, or `str`) and may reference top-level compile-time constants. They cannot depend on another parameter, local state, or a runtime function call. The compiler expands omitted defaults and reorders supplied named arguments into the function's concrete declaration-order native ABI; Flux does not need runtime named-argument dictionaries or reflection.

## First-class functions

Named functions are ordinary typed values. Function types use `fn(parameter_types) -> return_type` and may be given transparent aliases:

```flux
type Mapper = fn(i64) -> i64

fn double(value: i64) -> i64 {
    return value * 2
}

fn apply(transform: Mapper, value: i64) -> i64 {
    return transform(value)
}
```

A named function can be assigned to a binding, passed as an argument, returned from another function, and invoked through that binding. Function-value calls are positional because a function type describes the callable ABI rather than declaration-only parameter names/defaults. The bootstrap backend lowers these values to typed native C function pointers with deterministic generated typedefs; there is no callable object, boxing, reflection, or dynamic-dispatch runtime.

First-class function types currently support zero or one return value. Ordinary Flux functions may still return multiple values; making multi-return shapes first-class requires a standardized function-value ABI and remains future work. Anonymous functions and closures are separate planned features because captured values must integrate with the ownership model rather than being hidden heap objects.

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

Struct values can also be destructured by field. Field types are inferred from the struct declaration, fields may be renamed, and `_` may ignore a field without creating a binding:

```flux
struct User {
    name: str
    age: i64
}

fn main() -> i64 {
    let user: User = User { name: "Ada", age: 42 }
    let User { name, age: years } = user
    print(name)
    print(years)
    return 0
}
```

Struct destructuring is statically checked against the named struct type, rejects unknown/duplicate fields and duplicate local bindings, and evaluates the source expression exactly once before projecting fields. Concrete type aliases may be used as the pattern type because aliases are transparent at runtime.

## Closed enums and tagged unions

Closed enums are named value types with a fixed set of variants. Variants may carry zero or more positional payload values:

```flux
enum Outcome {
    Ok(i64)
    Error(str)
    Pending
}

fn load() -> Outcome {
    return Outcome.Ok(42)
}
```

Construction is always namespace-qualified as `Enum.Variant(...)`; payloadless variants still use `()` so variant construction remains syntactically distinct from ordinary field access. Payload arity and types are checked statically. Enum and struct definitions may refer to each other forward by value when the resulting layout is acyclic. Recursive by-value cycles are rejected until Flux has explicit ownership/indirection types.

The bootstrap backend lowers each enum to a native tag plus a union containing only the payload storage required by payload-bearing variants. Typed inline constructors build the tagged value; there is no object hierarchy, reflection, heap allocation, or hidden dynamic dispatch.

Enum values are consumed with exhaustive `match` statements. Payloads are bound positionally and statically typed; `_` ignores an unused payload position:

```flux
fn score(outcome: Outcome) -> i64 {
    match outcome:
        Outcome.Ok(value):
            return value
        Outcome.Error(_):
            return -1
        Outcome.Pending():
            return 0
}
```

Every variant must appear exactly once, every arm must target the scrutinee's enum type, and payload binding arity must match the variant declaration. A `match` scrutinee is evaluated once, and an exhaustive match whose arms all return satisfies function return analysis. Match expressions that themselves yield a value are planned separately.

## Compile-time constants

Top-level constants use `const name: type = expression`. Constants are evaluated by the compiler and substituted directly into generated code rather than emitted as mutable/runtime globals:

```flux
const BASE: i64 = 40
const ANSWER: i64 = BASE + 2
const ENABLED: bool = ANSWER == 42
```

The current constant evaluator supports `i64`, `bool`, and `str`, including forward constant references, primitive unary/binary operators, comparisons, equality, and boolean short-circuiting. Integer arithmetic follows Flux runtime integer semantics: addition/subtraction/multiplication wrap as signed `i64`, while division by zero and `i64::MIN / -1` are compile-time errors. Constant cycles, unknown references, type mismatches, function calls, struct values, and other runtime-only expressions are rejected.

## Type aliases

Concrete type aliases use `type Name = Target`. They are transparent compile-time names, not new runtime wrapper types:

```flux
type UserId = i64
type Person = User
```

Aliases may refer forward to structs and may chain through other aliases. The compiler resolves them to a concrete primitive or struct type for static checking and native lowering while preserving the alias spelling in source formatting and semantic tooling. Recursive alias cycles and unknown final targets are compile-time errors. Flux aliases are deliberately non-generic.

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
- `break` and `continue` inside loops, including through nested `if` / `match` blocks;
- exhaustive enum `match` statements with typed positional payload bindings;
- `return`;
- expression statements.

Planned:

- `while` once explicit local mutation/state semantics make it useful;
- `match` expressions that produce values;
- broader struct/list/record patterns as those value types mature.

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
