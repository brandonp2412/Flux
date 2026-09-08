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

A function whose entire body is one value-producing expression may use the concise brace form; the expression is an implicit return and uses the same static return checking and native ABI as an explicit `return`:

```flux
fn add(a: i64, b: i64) -> i64 { a + b }
fn positive(value: i64) -> bool { value > 0 }
```

Concise bodies require a non-`void` return type. Multi-value forwarding remains supported when the single expression is a call with the exact declared return shape.

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

Bindings are explicitly typed and immutable by default:

```flux
let count: i64 = 4
let enabled: bool = true
let label: str = "Flux"
```

Local mutation must be declared explicitly with `var`. Reassignment preserves the declared static type, and immutable `let` bindings, parameters, destructured bindings, and `for` loop variables cannot be assigned to:

```flux
fn count_to(limit: i64) -> i64 {
    var count: i64 = 0
    while count < limit:
        count = count + 1
    return count
}
```

`while` conditions must have type `bool`; `break` and `continue` use the same statically checked loop scope as `for`. Assignment is a statement, not a value-producing expression. The bootstrap compiler intentionally performs no implicit `bool`/integer/string conversions.

## Flat grid views

Flux UI syntax is deliberately flat rather than a nested widget tree. A `view` declares one grid and a set of sibling elements placed explicitly into that grid:

```flux
view Dashboard {
    grid columns: 240 1fr 320
    grid rows: 64 1fr
    grid gap: 16
    Text title at 1,2 span columns 2
        text: "Flux dashboard"
    Nav sidebar at 1,1 span rows 2
        label: "Navigation"
    Chart revenue at 2,2
        label: "Revenue"
}
```

Fixed grid tracks use target-independent logical units, `Nfr` tracks divide remaining space proportionally, and `auto` is reserved for intrinsic sizing. Coordinates are one-based `row,column` positions. `span rows N` and `span columns N` extend an element across tracks. Element names must be unique within a view. Placement is statically checked against the declared row/column counts, including the full extent of spans. Ordinary sibling grid regions must not overlap; intentional overlap will use a separate explicit overlay/absolute-positioning model rather than changing the meaning of ordinary grid placement.

The extra indentation under an element configures properties on that sibling element; it does not create child UI elements. UI elements themselves remain at the view's single element level, and deeper element nesting is rejected by the parser. This preserves a grid-oriented source structure that can later lower directly into target-native layout primitives rather than recreating Flutter-style widget construction.

Bootstrap built-in elements have concrete property contracts. `Text` supports `text: str` and `selectable: bool`; `Button` supports `text: str`, `enabled: bool`, and `on_press: fn() -> void`; `Nav`, `Chart`, and `Content` expose `label: str`; `Card` exposes `title: str`; and `Header` exposes `text: str`. Supplied property expressions are type-checked through the ordinary Flux expression/type system, so a named function can be passed directly as a callback without a controller object or widget subclass. Unknown built-in element/property names are static errors.

Declared Flux views are reusable typed element contracts rather than widget subclasses. A view may declare typed parameters and compile-time defaults, use those parameter values in its own flat element properties, and then appear as a sibling element type inside another view:

```flux
view Greeting(name: str, *, selectable: bool = false) {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: name
        selectable: selectable
}

view App {
    grid columns: 1fr
    grid rows: 1fr
    Greeting greeting at 1,1
        name: "Flux"
}
```

Composition arguments use the same indented property form and are checked against the target view's parameter types. Parameters without defaults are required; defaulted parameters may be omitted. Public/private module visibility applies to views, and recursive view composition cycles are rejected. Composition remains a flat layout relationship: the parent grid places one sibling region whose implementation is another declared view, without exposing a nested widget ownership tree or class model.

The bootstrap compiler currently parses, formats, merges, type-checks, and indexes this view metadata while the native rendering/runtime backend remains future work. View declarations therefore emit no runtime C yet.

## Modules and imports

Each `.flux` source file is a module in the bootstrap project model. A module imports another source file with an explicit relative path:

```flux
import "service.flux"
```

Imports resolve relative to the importing file, must remain relative, must end in `.flux`, and must already be normalized: `.` and `..` path segments are rejected rather than silently canonicalized. The project loader recursively parses each source exactly once, preserves a distinct stable source ID for diagnostics, detects import cycles, and then type-checks the merged program as one compilation unit. Imports are transitive, but imported declarations remain module-private unless explicitly exported. Package imports are additionally constrained to remain inside the package root.

Top-level functions, constants, type aliases, structs, enums, and interfaces are private to their source module by default. Prefix a declaration with `pub` to make it usable from another module, for example `pub fn parse(...)`, `pub struct User`, or `pub interface Readable`. Private declarations remain freely usable inside their own source file. Public APIs cannot expose private named types through function signatures, public aliases, struct fields, enum payloads, or interface capabilities, and public interfaces cannot compose private interfaces. `pub` is not valid on imports or interface implementation blocks. Public imported declarations are still referenced by their declared names in source for now, so duplicate-declaration diagnostics apply across the full import graph. Internally, however, loaded sources have stable module identities: package modules are named from `[package].name` plus the root-relative source path with `.flux` removed (for example `example::src::service`), while direct source projects use paths relative to the entry file's directory. This gives tooling and future namespace/ABI work deterministic module identity without adding an object-style namespace model. `fluxc check`, `fluxc emit-c`, and `fluxc build` are project-aware and operate on the complete import graph; `fluxc format` formats the selected source file only and preserves `pub` visibility.

A package may define a `flux.toml` manifest. The bootstrap manifest is intentionally small and dependency-free:

```toml
[package]
name = "example"
version = "0.1.0"
entry = "src/main.flux"
```

`name` and `entry` are required quoted strings; `version` is optional. The entry path must be relative, end in `.flux`, exist, and remain inside the package root after canonical path resolution. Package fields outside this currently specified schema are rejected so unsupported metadata is not silently ignored. `fluxc check`, `fluxc emit-c`, and `fluxc build` accept a direct `.flux` entry as before, a package directory containing `flux.toml`, or the `flux.toml` path itself. The manifest now anchors stable package-qualified module identities, but source-level namespace qualification, dependency resolution, and package ABI rules remain later work.

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

## Interfaces and capabilities

Interfaces are compile-time capability contracts made only from function signatures. They do not define objects, constructors, fields, inheritance, mixins, or hidden instance state:

```flux
interface Storage {
    fn load(path: str) -> (str, error)
    fn save(path: str, data: str, *, durable: bool) -> error
}
```

An interface declaration requires at least one function signature. Member parameter and return types are checked statically using the same concrete type system as ordinary functions; named-only parameters are preserved as part of the contract. Interface members do not have bodies or parameter defaults. Duplicate members and collisions between interface names and other type namespaces are compile-time errors.

Declaring an interface is zero-runtime metadata: the bootstrap native backend emits no function, object, vtable, allocation, or reflection data merely because an interface exists.

Concrete data types implement capabilities by explicitly mapping each interface member to an ordinary free function:

```flux
struct FileStorage {
    root: str
}

fn file_load(storage: FileStorage, path: str) -> (str, error) {
    return path, nil
}

fn file_save(storage: FileStorage, path: str, data: str, *, durable: bool) -> error {
    return nil
}

impl Storage for FileStorage {
    load: file_load
    save: file_save
}
```

The mapped free function receives the concrete implementing value as its first positional parameter. Every remaining parameter must match the capability signature's types and named-only shape, named parameters keep their contract names, and the return shape must match exactly. Every interface capability must be mapped exactly once. This gives Flux explicit conformance without methods, hidden receivers, classes, inheritance, or vtables. Implementation declarations themselves add no runtime object.

When the receiver's concrete type is statically known, the interface namespace can dispatch a capability directly:

```flux
let data: str, err: error = Storage.load(storage, "settings.flux")
let save_err: error = Storage.save(
    storage,
    "settings.flux",
    data,
    durable: true,
)
```

`Interface.capability(receiver, ...)` is namespace-qualified function dispatch, not a method call on the receiver. The compiler checks that the receiver type has the declared implementation, validates the remaining capability arguments against the interface signature, resolves the mapping at compile time, and emits a direct call to the mapped free function. Multi-value returns retain their ordinary native return ABI. No vtable, reflection, runtime interface object, or dynamic lookup is introduced for static dispatch.

When runtime polymorphism is needed, a concrete value can be packed explicitly into an interface value:

```flux
let storage: Storage = Storage(file_storage)
let data: str, err: error = Storage.load(storage, "settings.flux")
```

Bootstrap interface values are closed-world tagged values containing the concrete implementing data by value. Dynamic capability dispatch lowers to a native tag switch that calls the corresponding mapped free function. This adds no heap allocation, vtable, reflection, hidden object identity, or source-language object model. Interface values may be passed to and returned from functions, selected by conditional expressions, and used with named capability arguments and multi-value returns. Embedding interface values inside structs/enums is intentionally deferred until ownership and stable layout/ABI rules are defined.

Interfaces compose contracts explicitly without class inheritance:

```flux
interface Readable {
    fn load(path: str) -> (str, error)
}

interface Writable {
    fn save(path: str, data: str, *, durable: bool) -> error
}

interface Storage: Readable, Writable {
    fn label() -> str
}
```

Composition flattens compatible parent capabilities into the composed contract. An `impl Storage for Concrete` therefore maps `load`, `save`, and `label` explicitly to free functions. Composition cycles, unknown parents, and inherited capabilities with incompatible signatures are compile-time errors. Identical capabilities contributed by multiple parents are merged rather than duplicated. This is contract aggregation only; it introduces no subclassing, inherited state, method lookup hierarchy, or object identity.

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

Enum values are consumed with exhaustive `match` statements. Payloads are matched positionally and statically typed; `_` ignores an unused payload position. A payload that is a struct may be destructured directly in the arm pattern, including nested struct patterns:

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

```flux
struct User {
    name: str
    age: i64
}

enum Event {
    Loaded(User)
    Empty
}

fn describe(event: Event) -> i64 {
    match event:
        Event.Loaded(User { name, age: years }):
            print(name)
            return years
        Event.Empty():
            return 0
}
```

Every variant must appear exactly once, every arm must target the scrutinee's enum type, and payload pattern arity must match the variant declaration. Struct payload patterns are checked against the concrete payload type, bind projected fields with their declared static types, may nest recursively, and lower directly to native field access without constructing intermediary values. A `match` scrutinee is evaluated once, and an exhaustive match whose arms all return satisfies function return analysis.

`match` can also produce a value while keeping Flux's indentation-based control-flow style. The current multiline expression form is supported directly in typed bindings and returns:

```flux
fn score(outcome: Outcome) -> i64 {
    return match outcome:
        Outcome.Ok(value): value
        Outcome.Error(_): -1
        Outcome.Pending(): 0
}

fn main() -> i64 {
    let outcome: Outcome = Outcome.Ok(42)
    let score: i64 = match outcome:
        Outcome.Ok(value): value + 1
        Outcome.Error(_): -1
        Outcome.Pending(): 0
    print(score)
    return 0
}
```

Every expression arm must produce the same non-`void` type and the match must remain exhaustive. Pattern bindings and nested struct payload patterns work exactly as in statement matches. Native lowering evaluates the scrutinee once, switches on the enum tag, projects payload values only in the selected arm, and assigns or returns the selected result without introducing a boxed runtime value.

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
- value-producing enum `match` expressions in bindings and returns;
- Python-style conditional expressions, `a if condition else b`;
- `return`;
- expression statements.

Conditional expressions require a `bool` condition and both branches must produce the same non-`void` type. They are right-associative, so `a if first else b if second else c` works naturally. The native backend lowers them to target-native conditional control flow, preserving lazy evaluation of the unselected branch.

```flux
fn label(enabled: bool) -> str {
    return "enabled" if enabled else "disabled"
}
```

Planned:

- `while` once explicit local mutation/state semantics make it useful;
- broader list/record patterns as those value types mature.

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
    grid columns: 240 1fr
    grid rows: auto 1fr

    Nav sidebar at 1,1 span rows 2
    Header header at 1,2
    Content content at 2,2
}
```

The layout engine should resolve a flat element set against explicit grid coordinates/areas. Components may still compose reusable content, but composition should not force layout to become a deeply nested ownership tree.

The UI subsystem is intentionally downstream of the core type/ownership/IR work: it should compile to platform-native rendering and input backends rather than define the compiler architecture around one UI toolkit.

## Tooling contract

The bootstrap LSP now provides push diagnostics, canonical document formatting, compiler-backed quick fixes, safe hover information, full-document semantic tokens, completion for Flux keywords/builtins/current-module and public forward-import declarations plus lexically visible parameters/local bindings, ordinary-function signature help, and conservative definition/reference/rename support. Open files with imports are analyzed through the compiler's real project loader with in-memory overlays for every open unsaved Flux buffer, so module visibility, imported type changes, completion detail, hover signatures, and ordinary imported-call signature help are reflected before files are saved. Local completion respects active indentation scope, so a binding from a completed branch/loop is not suggested afterward. From an importing document, go-to-definition, references, and rename operate across the typed loaded dependency graph; reverse dependents outside that graph are not guessed. Highlighting uses standard LSP token categories for Flux keywords, literals, comments, operators, builtin types/functions, and semantically resolved symbols. Navigation/refactoring only acts when a declaration target is unambiguous, excludes strings/comments from identifier occurrences, and rejects invalid, keyword, or colliding rename targets. Cached workspace analysis, reverse-dependent discovery, richer member/property completion, fully resolved shadow-aware usage navigation, and interface/view signature help remain later work.

LSP, debugger, and profiler support are mandatory product features. Development hot reload is also a first-class requirement: `flux run` should watch source files automatically and apply compatible changes on file save, preserving compatible state and falling back to a controlled restart only when necessary. The normal workflow must not require a manual hot-reload key.

The bootstrap CLI now provides `fluxc run <target>` as the first development-runner slice. It builds and launches the native program, watches the compiler's actual import graph (and `flux.toml` for packages), debounces rapid saves, and recompiles automatically. Until the stable development ABI and compatible state model exist, a successful change is applied by building a replacement binary first and then performing a controlled restart. A failed edit leaves the last good child untouched, prints the ordinary rich compiler diagnostics, and keeps watching so saving a repair retries automatically. This is deliberately not described as state-preserving hot apply yet: compatible function/view replacement, Fast Refresh-style boundaries, and incremental compiler caching remain later milestones.

Compiler failures are represented as structured diagnostics rather than raw strings. A diagnostic records its compilation stage (`parse`, `type`, or `codegen`), message, and an optional source span. Every span also carries a stable `SourceId`, allowing editor/workspace tooling to distinguish identical line/column locations in different files; callers may provide their own IDs, while deterministic name-derived IDs are available for CLI/file workflows. Diagnostics can carry secondary labels, notes, and machine-applicable replacement fixes. Function and statement AST nodes retain source spans, while expression nodes carry token-level line/column/length spans assembled through unary, binary, call, and parenthesized expressions. Parser recovery synchronizes across malformed functions, and type checking safely continues across independent statements/functions while preserving declared binding types after bad initializers to avoid misleading cascades. `check_source_all` exposes the complete recovered diagnostic batch. Human CLI rendering shows the relevant source line with exact carets, related declaration labels and fixes, wraps messages to the active terminal width, and crops long paths/source lines around the relevant location. ANSI color is enabled for appropriate interactive terminals, respects `NO_COLOR`, and can be forced for tooling/tests. Missing-syntax diagnostics place their primary caret at the insertion point when the parser can identify it. `fluxc check <file> --json` remains a separate clean structured JSON channel for LSP/CI consumers, with no ANSI or human stderr mixed into the payload. This same diagnostic model is intended to be reused directly by editor tooling instead of reparsing CLI text.

The compiler therefore maintains stable source spans and a reusable semantic query layer from early development. Editor parse snapshots reuse unchanged source by fingerprint, and the semantic database indexes analyzed function, parameter, binding, loop-variable, and signature information for compiler/LSP consumers. `fluxc lsp` communicates over stdio JSON-RPC, negotiates UTF-8/UTF-16 positions, and reuses compiler diagnostics directly. Open files are analyzed against the real import graph with every open unsaved Flux file substituted through the project loader's in-memory overlay map; changing or closing one buffer republishes diagnostics for the other open buffers that may depend on it. Imported modules are analyzed in their containing open project graph rather than incorrectly treated as executable entry points. `textDocument/formatting` delegates to the same deterministic, comment-preserving canonical formatter and returns one whole-document edit when changes are needed. `textDocument/codeAction` exposes machine-applicable diagnostic fixes as preferred quick-fix workspace edits, preserving the compiler's exact replacement span/text instead of reparsing error messages. `fluxc format --check` remains the non-mutating CI formatting contract. Cached/incremental workspace analysis and cross-module navigation/completion/refactoring remain later work. The debugger will need source-to-native debug metadata, expression evaluation rules, and predictable optimized-build behavior. The profiler should support low-overhead instrumentation and symbolization without requiring a separate language parser.
