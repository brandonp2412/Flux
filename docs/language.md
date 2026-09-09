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
    for i in 0..=10:
        if i < 5:
            print(i)
    return 0
}
```

Tabs are not valid indentation. Blocks must use consistent indentation at each nesting level. Flux has no comment syntax: `#` outside a string is a parse error. Flux also has no ternary/conditional expression form; value selection uses ordinary `if` control flow or exhaustive `match` expressions.

Flux has no lint-warning tier: an unused parameter or binding is a compile error unless its name begins with `_` to mark it intentionally ignored.

Flux source naming is lower camelCase for values, functions, constants, built-ins, properties, events, and environment bindings: `requestCount`, `onPress`, and `windowHeight`. Type-like names remain PascalCase. Tooling emits camelCase only; ordinary user-defined identifiers may still parse in legacy snake_case for compatibility, but language-owned names use the camelCase spelling.

Bindings are explicitly typed and immutable by default:

```flux
let count: i64 = 4
let enabled: bool = true
let label: str = "Flux"
let pattern: str = r"\d+\w+"
```

Raw strings use `r"..."`. Backslashes are literal rather than escape introducers, so paths and regular-expression-like text do not require doubled backslashes. The closing `"` still terminates the literal, so the bootstrap raw form does not embed a double quote. Raw and ordinary literals have the same `str` type and runtime representation; canonical formatting normalizes raw literals to the equivalent escaped ordinary string.

Local mutation must be declared explicitly with `var`. Reassignment preserves the declared static type, and immutable `let` bindings, parameters, destructured bindings, and `for` loop variables cannot be assigned to:

```flux
fn countTo(limit: i64) -> i64 {
    var count: i64 = 0
    while count < limit:
        count = count + 1
    return count
}
```

`while` conditions must have type `bool`; `break` and `continue` use the same statically checked loop scope as `for`. Assignment is a statement, not a value-producing expression. The bootstrap compiler intentionally performs no implicit `bool`/integer/string conversions. `i64` arithmetic is checked: `+`, `-`, `*`, unary `-`, and `/` fail explicitly on overflow or invalid division. Integer `/` truncates toward zero, so Flux does not need a second Dart-style `~/` operator with identical `i64` semantics.

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

Bootstrap built-in elements have concrete compiler-owned property contracts. Native `Text` includes typed text/selectability, size/weight/family/slant/decoration/spacing/color controls; `Button`, `TextInput`, `Image`, `Toggle`, and `Radio` expose their native interaction/content contracts; and common properties cover visibility, accessibility, layout, styling, transforms, and pointer/focus events. Supplied property expressions are checked through the ordinary Flux expression/type system, so state-derived values and named callbacks do not require controller objects or widget subclasses. Unknown built-in element/property names are static errors, and the same contracts drive LSP completion/hover.

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

Imports resolve relative to the importing file, must remain relative, must end in `.flux`, and must already be normalized: `.` and `..` path segments are rejected rather than silently canonicalized. The project loader recursively parses each source exactly once, preserves a distinct stable source ID for diagnostics, detects import cycles, and then type-checks the merged program as one compilation unit. Imports are transitive, but visibility still follows the importing module's forward import graph: loading two sibling modules into the same project does not make their public declarations visible to each other. A public declaration is reachable from another module only when the declaring module is directly or transitively imported from that usage module. Imported declarations remain module-private unless explicitly exported. Package imports are additionally constrained to remain inside the package root.

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
let User { name, age: years } = loadUser()
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

fn fileLoad(storage: FileStorage, path: str) -> (str, error) {
    return path, nil
}

fn fileSave(storage: FileStorage, path: str, data: str, *, durable: bool) -> error {
    return nil
}

impl Storage for FileStorage {
    load: fileLoad
    save: fileSave
}
```

The mapped free function receives the concrete implementing value as its first positional parameter. Every remaining parameter must match the capability signature's types and named-only shape, named parameters keep their contract names, and the return shape must match exactly. Every interface capability must be mapped exactly once. This gives Flux explicit conformance without methods, hidden receivers, classes, inheritance, or vtables. Implementation declarations themselves add no runtime object.

When the receiver's concrete type is statically known, the interface namespace can dispatch a capability directly:

```flux
let data: str, err: error = Storage.load(storage, "settings.flux")
let saveErr: error = Storage.save(
    storage,
    "settings.flux",
    data,
    durable: true,
)
```

`Interface.capability(receiver, ...)` is namespace-qualified function dispatch, not a method call on the receiver. The compiler checks that the receiver type has the declared implementation, validates the remaining capability arguments against the interface signature, resolves the mapping at compile time, and emits a direct call to the mapped free function. Multi-value returns retain their ordinary native return ABI. No vtable, reflection, runtime interface object, or dynamic lookup is introduced for static dispatch.

When runtime polymorphism is needed, a concrete value can be packed explicitly into an interface value:

```flux
let storage: Storage = Storage(fileStorage)
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
const defaultCount: i64 = 3

fn describe(prefix: str, suffix: str = "!", *, count: i64 = defaultCount, label: str) -> i64 {
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

A named function can be assigned to a binding, passed as an argument, returned from another function, and invoked through that binding. Capture-free anonymous functions use the same concrete function-value representation and can be stored or passed directly:

```flux
type Mapper = fn(i64) -> i64

fn apply(transform: Mapper, value: i64) -> i64 {
    return transform(value)
}

fn main() -> i64 {
    let double: Mapper = fn(value: i64) { value * 2 }
    return apply(fn(value: i64) -> i64 { value + 1 }, double(20))
}
```

Anonymous parameters are typed and positional. Their single-expression body may infer a scalar or `void` return, or the return type may be written explicitly with `->`. Anonymous parameters follow the normal source-cleanliness rule: an unused named parameter is a compile error unless it begins with `_`. Reading an outer local binding from an anonymous function is currently rejected; that operation is a closure capture and remains blocked on the ownership/lifetime model rather than being implemented as an implicit heap object.

Function-value calls are positional because a function type describes the callable ABI rather than declaration-only parameter names/defaults. The bootstrap backend lowers named and capture-free anonymous values to typed native C function pointers with deterministic generated typedefs/helpers; there is no callable object, boxing, reflection, or dynamic-dispatch runtime.

First-class function types currently support zero or one return value. Ordinary Flux functions may still return multiple values; making multi-return shapes first-class requires a standardized function-value ABI and remains future work. Safe captured closures remain future work under the ownership model.

## Error handling

Flux does not use exceptions or `try` / `catch` for recoverable failures.

The model uses explicit multiple return values, similar in spirit to Go but strictly typed and integrated with Flux ownership. Multi-values are deliberately not general-purpose tuple values. A function returning multiple values must be consumed by a destructuring binding. Bindings may spell out their types explicitly or use the narrow inferred pattern form `let (first, second) = call(...)`; inferred positions take their types directly from the function's return shape, and `_` ignores a position without creating a binding. Arity and positional types are checked at compile time. This keeps the feature narrow, predictable, and easy to lower efficiently.

Recoverable failures use the compiler-known `error` type. `nil` is the only no-error value, and `error("message")` constructs an error. `error` is intentionally not a generic `Result<T, E>` or general optional type; Flux has no generics, and normal error handling stays explicit in function signatures and control flow:

```flux
fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "configuration loaded", nil
}

fn main() -> i64 {
    let (data, err) = load("settings.flux")
    if err != nil:
        print(err)
    print(data)
    return 0
}
```

The explicit typed form remains available when the declaration should repeat the return contract, for example `let data: str, err: error = load(path)`. Both forms evaluate the source call exactly once. The inferred parenthesized form is a multi-value pattern, not a tuple value: it cannot be stored, indexed, or passed around as a general aggregate.

The bootstrap representation stores only an error message. The language-level type is opaque so richer error metadata can be introduced later without turning errors into strings.

A function may directly forward another function's multi-value result with `return call(...)` when the complete return shape matches exactly. This is compile-time checked positionally and lowers to ordinary native return values; it does not introduce exceptions, stack unwinding, or general tuple values.

When successful values are needed before the enclosing function returns, a destructuring binding may use `else return`:

```flux
fn loadConfig(path: str) -> (str, error) {
    let (data, _) = load(path) else return
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

Lists can be matched directly by length without wrapping them in an enum. Exact patterns such as `[]`, `[only]`, and `[first, last]` match one concrete length. A single rest binding such as `[first, ...middle, last]` matches any list with at least the fixed prefix/suffix arity; `middle` is inferred as `T[]` and is a zero-copy view preserving the source stride. `_` is an unconditional fallback. Flux checks the union of list-length patterns at compile time, rejects non-exhaustive matches and arms made unreachable by earlier patterns, and applies the same rules to statement and value-producing matches:

```flux
fn classify(values: i64[]) -> i64 {
    return match values:
        []: 0
        [_]: 1
        [_, ...middle, _]: middle.length + 2
}

fn show(values: i64[]) -> i64 {
    match values[::-1]:
        []:
            print 0
        [only]:
            print only
        [first, ...middle, last]:
            print first
            print middle.length
            print last
    return 0
}
```

The list scrutinee is evaluated once. Prefix and suffix element bindings use logical list order even for stepped/reversed views, and rest bindings remain zero-copy strided views. Named pattern bindings are ordinary Flux bindings: if they are not read, compilation fails rather than emitting a warning.

Enum and list match arms may add a boolean guard after the pattern. Pattern bindings are in scope inside the guard, and guarded arms may fall through to later arms of the same variant or list shape when the condition is false:

```flux
fn classify(outcome: Outcome) -> i64 {
    return match outcome:
        Outcome.Ok(value) if value > 100: 2
        Outcome.Ok(_): 1
        Outcome.Error(_): -1
        Outcome.Pending(): 0
}

fn classifyList(values: i64[]) -> i64 {
    return match values:
        [only] if only > 100: 2
        [only]: 1
        _: 0
}
```

Guards must have type `bool`. A guarded arm does not make its variant or list-length domain exhaustive because the guard may be false, so an unguarded covering arm is still required somewhere later. Once an unguarded arm covers a variant or list shape, a later arm for that already-covered domain is rejected as unreachable. These rules apply to both statement and value-producing `match` forms.

## String literals

Flux `str` values support ordinary escaped strings, raw strings, and multiline strings:

```flux
let escaped: str = "line one\nline two"
let path: str = r"C:\Flux\bin"
let message: str = """
    Flux multiline strings
    keep source indentation readable.
    # remains literal text here.
"""
```

Triple-quoted `"""..."""` strings use the same escapes as ordinary strings. When the opening delimiter is followed by a newline and the closing delimiter is on its own indented line, Flux removes the delimiter-only leading/trailing line and strips the shared indentation from non-empty content lines. This keeps source indentation out of the runtime value. A `#` inside any string remains ordinary text; Flux still has no comment syntax outside strings. The formatter preserves multiline literal blocks verbatim while formatting the surrounding Flux source, so readable indentation and line breaks are retained exactly as written.

## Compile-time constants

Top-level constants use `const name: type = expression`. Constants are evaluated by the compiler and substituted directly into generated code rather than emitted as mutable/runtime globals:

```flux
const BASE: i64 = 40
const ANSWER: i64 = BASE + 2
const ENABLED: bool = ANSWER == 42
```

The current constant evaluator supports `i64`, `bool`, and `str`, including forward constant references, primitive unary/binary operators, comparisons, equality, and boolean short-circuiting. Integer arithmetic follows Flux runtime integer semantics: addition, subtraction, multiplication, and negation are checked for `i64` overflow, while division rejects zero and `i64::MIN / -1`. The same failures are compile-time diagnostics when they occur in constant/default expressions. Constant cycles, unknown references, type mismatches, function calls, struct values, and other runtime-only expressions are rejected.

The native lowering pass also folds pure primitive expressions outside `const` declarations. Expressions such as `2 * 20 + 2`, `10 > 3 && !false`, and `"Flux" == "Flux"` become native literals before C emission. A local binding, view state, window environment value, function call, or other runtime dependency stops folding at that expression, so dynamic work remains dynamic. Boolean short-circuiting is preserved during folding, and fully static checked-arithmetic failures are reported before native execution. Compile-time UI and application properties use the same folding rules, so values such as `size: 7 * 4` and `height: 120 * 2` are accepted without introducing runtime calculation.

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

## Bootstrap lists, indexing, slicing, and comprehensions

Flux spells concrete list types as `T[]`, preserving the language rule that source generics do not exist. The current bootstrap supports immutable local lists with homogeneous element types:

```flux
let values: i64[] = [1, 2, 3, 4, 5]
let first: i64 = values[0]
let last: i64 = values[-1]
let middle: i64[] = values[1:4]
let tail: i64[] = values[-2:]
let evens: i64[] = values[::2]
let reversed: i64[] = values[::-1]
let [reverseFirst, _, _, _, reverseLast] = reversed
let [reverseHead, ...reverseBody, reverseTail] = reversed
let reverseMiddle: i64[] = values[3:0:-2]
let spreadValues: i64[] = [0, ...values[1:4], ...values[::-2], 9]
let includeHigh: bool = true
let conditionalValues: i64[] = [0, if includeHigh: 7, if false: 8 else: 9, ...values[1:4]]
let count: i64 = values.length
let empty: bool = values.isEmpty
let present: bool = values.isNotEmpty
let first: i64 = values.first
let last: i64 = values.last
let one: i64 = values[2:3].single
let window: i64[] = values | skip 1 | take 3
let checks: bool[] = [value > 2 for value in values]
let hasLarge: bool = checks | any
let allLarge: bool = checks | every
let total: i64 = values | reduce add
let positive: bool = values | fold true allPositive
let emptyTotal: i64 = values[:0] | fold 7 add
let joined: i64[] = values[::2] | concat values[::-1][:2]
let joinedDoubled: i64[] = values[::2] | concat values[::-1][:2] | map double
let unique: i64[] = values | concat values[::-1] | distinct
let nested: i64[][] = [values[::2], values[::-1][:2]]
let flat: i64[] = nested | flatten
let ordered: i64[] = [4, 1, 3, 2] | sorted
let chunks: i64[][] = values[::-1] | chunked 2
let rejoined: i64[] = chunks | flatten
let doubled: i64[] = [value * 2 for value in values]
let large: i64[] = [value * 2 for value in values if value > 2]
for value in values:
    print value
for index, value in values:
    print(index + value)
```

Index expressions must be `i64`. Negative indices count from the end and an index outside the list is a checked runtime error. Slices follow Python-style exclusive-end semantics with `[start:end]` and `[start:end:step]`: bounds may be omitted or negative, bounds clip to the list extent, positive steps move forward, negative steps move backward, and `[::-1]` reverses a list view. A literal zero step is rejected at compile time and a dynamically computed zero step raises an explicit Flux runtime error. Slices remain zero-copy even when stepped or reversed: the native list descriptor carries a byte stride, and slicing an already-strided view composes the strides instead of copying elements. List destructuring uses `let [first, _, last] = source` for exact patterns and `let [first, ...middle, last] = source` for rest patterns. The source must be a concrete `T[]`; ordinary non-`_` bindings infer `T`, while the single optional `...rest` binding infers `T[]`. All named bindings remain immutable and participate in the ordinary unused-binding compile checks. The source is evaluated once and strided/reversed views are read in logical order. Exact patterns require the runtime list length to equal the number of ordinary positions and fail explicitly with `Flux runtime error: list pattern requires exactly N elements` on mismatch. Rest patterns require at least the ordinary-position count, bind the unmatched middle as a zero-copy view preserving the source stride, and fail with `Flux runtime error: list pattern requires at least N elements` when too short. `_` ignores one position and `..._` ignores the unmatched remainder. The same exact/rest shapes are available in direct exhaustive list `match` arms, where a non-matching length selects the next arm instead of raising a runtime error. List literals may splice another list with `...source`, for example `[0, ...middle, ...values[::-2], 9]`. Every spread source must have exactly the same concrete element type as the surrounding literal. List construction also has dedicated control items: `if condition: value` inserts the value only when the condition is true, while `if condition: first else: second` inserts exactly one selected value. This syntax is valid only as a list item; it does not introduce a general ternary/conditional expression. Conditions are `bool`, both branches must match the surrounding element type, and only the selected branch is evaluated. Spread sources, conditions, and selected values are evaluated once in source order; native lowering computes the total length with overflow checks and fills one stack-backed result buffer, including from strided views. During the bootstrap, literals using spread or list-control items must be bound directly to an immutable local `let`, matching the ownership restriction used by other collection-producing operations. Compiler-known list values also expose `length: i64`, `isEmpty: bool`, and `isNotEmpty: bool` as property-like syntax that lowers directly to the native list descriptor; these are not methods or object members. `first` and `last` return the element type and reuse checked list indexing, so an empty list fails with an explicit bounds error. `single` returns the element only when the list length is exactly one and otherwise raises an explicit Flux runtime error.

Comprehension sources must be lists, the optional filter must be `bool`, and the produced element type is inferred from the value expression. The bootstrap native lowering evaluates the source once and uses stack-backed result storage sized to the source list, so filtered comprehensions do not require hidden heap allocation or intermediate collections. List iteration likewise evaluates its source exactly once. `for value in values:` infers `value` from the element type; `for index, value in values:` additionally binds an `i64` index without manual counter state. Both forms support the ordinary loop-scoped `break` and `continue` rules.

`take(list, count)` and `skip(list, count)` are compiler-known typed sequence functions. They preserve the concrete list element type, require an `i64` count, clamp oversized counts to the available length, and reject negative counts with an explicit Flux runtime error. Both lower to zero-copy list views, so pipelines such as `values | skip 1 | take 3` do not allocate or copy list elements.


`any(list)` and `every(list)` currently accept `bool[]`. `any` returns true when at least one element is true and returns false for an empty list. `every` returns true only when every element is true and uses the standard vacuous-truth identity of true for an empty list. Predicate-style queries remain explicit and allocation-free in source by composing a boolean comprehension with the pipeline, for example `[value > 2 for value in values] | any`.

`map(list, callback)`, `filter(list, predicate)`, and its readable alias `where(list, predicate)` are compiler-known collection transforms. `map` requires a concrete `fn(T) -> U` callback and produces `U[]`; `filter`/`where` require `fn(T) -> bool` and preserve `T[]`. The source is evaluated once and strided views are consumed directly. Chained transform stages fuse into one source loop and write only the final collection into one stack-backed buffer sized to the original source, avoiding intermediate list materialization. During the bootstrap collection-producing transform pipelines must be bound directly to an immutable local value, matching list comprehensions.

`fold(list, initial, reducer)` and `reduce(list, reducer)` are compiler-known scalar reductions. The reducer must be a concrete function value with an exact type, so either a named/function binding or a capture-free anonymous function is valid: `fold` requires `fn(A, T) -> A`, while `reduce` requires `fn(T, T) -> T`. `fold` returns the initial value unchanged for an empty list. `reduce` requires at least one produced element and otherwise raises `Flux runtime error: reduce requires a non-empty list`. Both evaluate their source once, iterate strided list views directly, and compose through pipelines such as `values[::-1] | reduce add`. When a terminal reduction follows `map`/`filter`/`where`, those transforms are executed lazily inside the reduction loop, so no intermediate transformed list or buffer is created. During the bootstrap reductions lower when bound directly to a local value, matching the direct-binding restriction used by list comprehensions.

`concat(left, right)` joins two lists with exactly the same concrete element type. It accepts strided inputs, preserves logical element order, checks length overflow explicitly, and produces one contiguous stack-backed local list. It composes through pipelines, for example `left | concat right | map double`. Like other collection-producing bootstrap operations, the concatenated result must remain within the supported local lifetime.

`distinct(list)` removes duplicate scalar values while preserving the first occurrence of each value. It currently supports `i64[]`, `bool[]`, `str[]`, and `error[]`, whose equality rules are already defined by Flux; aggregate, nested-list, and function elements are rejected rather than receiving implicit deep equality. Strided inputs are consumed in logical order and the result is a stack-backed local list that can feed later pipeline stages.

`flatten(nested)` removes exactly one list layer, converting `T[][]` to `T[]`. Both the outer nested list and each inner list may be strided views; flattening walks their logical order, checks total output length overflow before allocating the result buffer, and then writes one contiguous stack-backed local list. It does not recursively flatten arbitrary depth.

`sorted(list)` returns a new ascending list without mutating its source. The bootstrap supports `i64[]`, `bool[]`, and `str[]`; booleans order `false` before `true`, strings use lexical ordering, and other element types are rejected until Flux has an explicit ordering contract. Strided inputs are read in logical order, the stable result is stack-backed, and pipelines such as `values[::-1] | sorted | map double` compose normally.

`chunked(list, size)` splits a list into consecutive `T[]` views and returns them as `T[][]`. The size is an `i64` and must be greater than zero; a literal zero is rejected statically and dynamic non-positive sizes fail with an explicit Flux runtime error. Chunk descriptors are stack-backed but their elements remain zero-copy views into the original list, preserving positive or negative source stride. The final chunk may be shorter, an empty source produces zero chunks, and pipelines such as `values[::-1] | chunked 2 | flatten` preserve logical order.

This is intentionally a local-lifetime slice while Flux's ownership model is unfinished. List values currently cannot be returned from functions, stored in structs/enums, or declared as mutable `var` bindings. Those forms are compile errors rather than unsafe implicit lifetime escapes. List parameters are permitted as non-consuming immutable borrowed views of caller-local storage; owned argument/return transfer, owned storage, mutation, and aggregate storage remain part of the ownership/container roadmap.

## Dart-inspired ergonomics direction

Flux should borrow Dart's strongest expression and collection ergonomics without importing Dart's class-oriented object model. Current list construction already includes spreads, dedicated `if`/`else` items, and comprehensions. Remaining planned ergonomics include optional types, null/optional-aware access and indexing, coalescing, cascades that evaluate their target once, records, broader patterns/destructuring, string interpolation, and async/generator ergonomics.

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

The compiler now has an explicit alias-aware `Copy` classification as the first ownership primitive. In the current bootstrap, `i64`, `bool`, immutable `str`/`error` handles, function values, closed interface values, and structs/enums whose complete payload graph is copyable are `Copy`. `void` is not a value, and `T[]` list/view values are non-copy until owned collection storage and move/borrow checking land. Mutable-binding and aggregate-storage validation use this shared classification instead of assuming that lists are the only future non-copy type.

Direct local transfer now exercises that distinction: `let destination: T = source` moves `source` when `T` is non-copy, and a later read of `source` is a compile error. `Copy` values remain reusable. Moves performed on `if` or exhaustive `match` paths that can rejoin later code conservatively invalidate the outer binding; paths that definitely `return`, `break`, or `continue` are kept separate from sibling fallthrough state. Inside loops, a direct non-copy move is accepted only when every remaining path is guaranteed to `break` or `return` before another iteration; fallthrough and `continue` remain compile errors because they could consume the same value again. This iteration-exit proof propagates through nested `if`/`match` blocks, and moves that escape an inner loop through `break` are revalidated against every enclosing loop before they can reach another iteration. Break-path move state is still propagated to post-loop code, so a value consumed before `break` cannot be reused after the loop. Loop bodies whose moved path necessarily returns preserve zero-iteration fallthrough ownership. Calls, partial aggregate moves, and owned-resource transfer remain deferred to normalized ownership/IR analysis.

Existing list/view operations model the bootstrap's first immutable-borrow behavior: reading, indexing, slicing, iteration, matching, sequence transforms, and passing `T[]` to a function do not consume the local list owner. These are compiler-enforced local views, not yet first-class source-level borrow/reference types, and they cannot escape through list returns or aggregate storage.

The first full borrow checker should operate over normalized typed semantics/IR rather than parser syntax. General moves across calls/returns/fields, first-class immutable and exclusive mutable borrows, lifetime inference, partial-move analysis, and deterministic destruction remain subsequent ownership milestones.

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
- `for name in start..end:` with an exclusive integer range and `for name in start..=end:` with an inclusive integer range; both evaluate their bounds once, and inclusive lowering avoids signed overflow at `i64::MAX`;
- `break` and `continue` inside loops, including through nested `if` / `match` blocks;
- exhaustive enum `match` statements with typed positional payload bindings;
- value-producing enum `match` expressions in bindings and returns;
- `return`;
- expression statements.

Flux intentionally has no ternary expression. Branching that returns a value remains explicit:

```flux
fn label(enabled: bool) -> str {
    if enabled:
        return "enabled"
    return "disabled"
}
```

`while` is implemented for explicitly mutable local state. Broader list/record patterns remain planned as those value types mature.

## Shell-inspired typed call flow

Function calls may omit parentheses when positional arguments are sufficient:

```flux
print "hello"
scale value 2
```

A single `|` forms a typed value pipeline. The left-hand result becomes the first positional argument to the next function; it does not capture or parse process stdout:

```flux
let result: i64 = increment 2 | scale 5
print result
```

Shell-style redirection serializes a scalar function result to a typed `str` path. `>` truncates and `>>` appends. Supported bootstrap result types are `i64`, `bool`, `str`, and `error`:

```flux
message > "result.txt"
message >> "result.txt"
```

A trailing `&` detaches a call or pipeline statement. The current native bootstrap uses POSIX process primitives only in programs that actually contain background execution:

```flux
work 42 | report &
```

Parenthesized calls remain valid and are required for named arguments. These shell-inspired forms are typed function syntax, not a subprocess command language.

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

The semantic database now also builds a deterministic structural control-flow graph for every checked function as the first IR foundation. CFG nodes retain Flux source spans and typed binding declarations, while edges make sequential flow, `if` branches, loop back-edges, `break`, `continue`, `return`, match-arm dispatch, and explicit `else return` success/error exits visible to later analyses. Each statement node also carries a stable set of binding reads plus explicit source/destination/span metadata for direct non-copy local moves, mirroring the ownership behavior already enforced by the bootstrap checker. This graph is deliberately not yet the full typed value IR: expressions still live in the checked AST, borrow/lifetime/path-state facts are not normalized yet, and bootstrap C code generation still lowers from the AST. Ownership and optimization passes should migrate onto normalized CFG/IR facts rather than adding more syntax-shaped special cases to the bootstrap checker.

## Application entry and bootstrap Linux GUI

A GUI executable may select one zero-parameter root view with an application declaration instead of defining `fn main() -> i64`:

```flux
view HelloApp {
    grid columns: 1fr
    grid rows: auto auto auto
    grid gap: 12
    state clicked: bool = false
    derived showGreeting: bool = !clicked

    Text greeting at 1,1
        text: "Hello, Flux!"
        visible: showGreeting

    Text status at 2,1
        text: "Clicked!"
        visible: clicked

    Button action at 3,1
        text: "Toggle"
        onPress: clicked => !clicked
}

app HelloApp
```

The bootstrap Linux backend lowers this root view to a GTK4 application/window and `GtkGrid`; `Text`, `Button`, `TextInput`, `Image`, `Toggle`, and `Radio` become native GTK controls. View-local state is explicit `bool` or `i64` data initialized from compile-time values, and events may apply typed functional transitions such as `onPress: state => nextExpression`. Views may also declare read-only computed bindings with `derived name: type = expression`. Bootstrap derived values support `i64`, `bool`, and `str`; they may read view parameters, mutable state, read-only window environment bindings, compile-time constants, and earlier derived bindings. Declaration order is significant, which rejects forward/cyclic derived dependencies without a runtime dependency graph. Derived values cannot be transition targets and are recomputed in order at the start of each native refresh before dependent properties update. State- and derived-dependent properties use ordinary Flux expressions and update existing controls rather than reconstructing the window/grid. Root views also receive compiler-owned read-only environment bindings `windowWidth`, `windowHeight`, `windowIsLandscape`, `windowIsPortrait`, and `displayScale`; ordinary native window resizes/scale changes update those values and reuse the same refresh path, so responsive property expressions need no window/controller object. Common transforms participate in the same model: translation, rotation, uniform/per-axis scale, skew, and percentage transform origins are typed flat-element properties. Static values fold into the element stylesheet, while runtime primitive expressions refresh a persistent native CSS provider. Named callbacks remain supported as a separate event form. Fixed grid tracks feed native size requests, `fr` tracks expand, and row/column spans remain the explicit Flux placement model. GTK types are not exposed in Flux source and do not establish a widget-oriented source architecture. Exact maximized/fullscreen content geometry, responsive grid definitions, owned/string/aggregate mutable state, animation timelines, and a lower-level long-term Wayland renderer remain later work.

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

The UI subsystem should compile to platform-native rendering and input backends rather than define the compiler architecture around one UI toolkit. The GTK4 Linux bootstrap is therefore an early end-to-end dogfood lowering for the existing flat UI semantics, not a commitment to GTK as Flux's permanent renderer or an object model visible to application code.

## Tooling contract

The bootstrap LSP now provides push diagnostics, canonical document formatting, compiler-backed quick fixes, safe hover information, full-document semantic tokens, completion for Flux keywords/builtins/current-module and public forward-import declarations plus lexically visible parameters/local bindings, typed signature help for every current parenthesized call form, focused inferred-type inlay hints, and conservative definition/reference/rename support. Signature help covers ordinary/imported functions, first-class concrete function values, namespace-qualified interface capabilities, enum payload constructors, interface packing, and the compiler built-ins `print`/`error`; interface capability help reflects Flux's namespace-qualified free-function model by showing an explicit receiver parameter rather than presenting a method. Open files with imports are analyzed through the compiler's real project loader with in-memory overlays for every open unsaved Flux buffer, so module visibility, imported type changes, completion detail, hover signatures, and ordinary imported-call signature help are reflected before files are saved. Local completion respects active indentation scope, so a binding from a completed branch/loop is not suggested afterward. Qualified enum/interface namespace completion remains available immediately after incomplete edits such as `Outcome.` or `Storage.` by probing the declarations/imports before the active broken top-level declaration, including reachable imported contracts. Flat UI tooling reuses compiler-owned view contracts: at view layout indentation completion suggests grid declarations and built-in element kinds; element property blocks receive typed built-in properties or composed-view parameters and suppress properties already declared on that element; same-file custom-view contracts declared before the currently malformed view remain available for property completion during incomplete edits; hover on built-in element kinds/properties remains available even while the surrounding view is syntactically incomplete, while valid composed views show their typed parameter contract. Inlay hints are restricted to types the language intentionally infers—currently struct/enum pattern bindings and range-loop variables—so explicitly annotated bindings are not restated. From an importing document, go-to-definition, references, and rename operate across the typed loaded dependency graph; reverse dependents outside that graph are not guessed. Highlighting uses standard LSP token categories for Flux keywords, literals, comments, operators, builtin types/functions, and semantically resolved symbols. Navigation/refactoring only acts when a declaration target is unambiguous, excludes strings/comments from identifier occurrences, and rejects invalid, keyword, or colliding rename targets. Cached workspace analysis, reverse-dependent discovery, fully resolved shadow-aware usage navigation, value-member completion, and richer UI navigation remain later work.

LSP, debugger, and profiler support are mandatory product features. Development hot reload is also a first-class requirement: `flux run` should watch source files automatically and apply compatible changes on file save, preserving compatible state and falling back to a controlled restart only when necessary. The normal workflow must not require a manual hot-reload key.

The bootstrap CLI now provides `fluxc run <target>` as the first development-runner slice. It builds and launches the native program, watches the compiler's actual import graph (and `flux.toml` for packages), debounces rapid saves, and recompiles automatically. Native compilation has explicit `debug`, `profile`, and `release` modes. `fluxc build` defaults to `release`; `fluxc run` defaults to `debug`. Debug builds use no optimization with full Clang debug information and frame pointers, profile builds retain debug information/frame pointers under optimization, and release builds use aggressive optimization plus LTO. These modes establish predictable optimization/symbol behavior, but source-level Flux debug locations still require dedicated compiler debug metadata. Until the stable development ABI and compatible state model exist, a successful change is applied by building a replacement binary first and then performing a controlled restart. A failed edit leaves the last good child untouched, prints the ordinary rich compiler diagnostics, and keeps watching so saving a repair retries automatically. This is deliberately not described as state-preserving hot apply yet: compatible function/view replacement, Fast Refresh-style boundaries, and incremental compiler caching remain later milestones.

Compiler failures are represented as structured diagnostics rather than raw strings. A diagnostic records its compilation stage (`parse`, `type`, or `codegen`), message, and an optional source span. Every span also carries a stable `SourceId`, allowing editor/workspace tooling to distinguish identical line/column locations in different files; callers may provide their own IDs, while deterministic name-derived IDs are available for CLI/file workflows. Diagnostics can carry secondary labels, notes, and machine-applicable replacement fixes. Function and statement AST nodes retain source spans, while expression nodes carry token-level line/column/length spans assembled through unary, binary, call, and parenthesized expressions. Parser recovery synchronizes across malformed functions, and type checking safely continues across independent statements/functions while preserving declared binding types after bad initializers to avoid misleading cascades. `check_source_all` exposes the complete recovered diagnostic batch. Human CLI rendering shows the relevant source line with exact carets, related declaration labels and fixes, wraps messages to the active terminal width, and crops long paths/source lines around the relevant location. ANSI color is enabled for appropriate interactive terminals, respects `NO_COLOR`, and can be forced for tooling/tests. Missing-syntax diagnostics place their primary caret at the insertion point when the parser can identify it. `fluxc check <file> --json` remains a separate clean structured JSON channel for LSP/CI consumers, with no ANSI or human stderr mixed into the payload. This same diagnostic model is intended to be reused directly by editor tooling instead of reparsing CLI text.

The compiler therefore maintains stable source spans and a reusable semantic query layer from early development. Editor parse snapshots reuse unchanged source by fingerprint, and the semantic database indexes analyzed function, parameter, binding, loop-variable, and signature information for compiler/LSP consumers. `fluxc lsp` communicates over stdio JSON-RPC, negotiates UTF-8/UTF-16 positions, and reuses compiler diagnostics directly. Open files are analyzed against the real import graph with every open unsaved Flux file substituted through the project loader's in-memory overlay map; changing or closing one buffer republishes diagnostics for the other open buffers that may depend on it. Imported modules are analyzed in their containing open project graph rather than incorrectly treated as executable entry points. `textDocument/formatting` delegates to the same deterministic, comment-preserving canonical formatter and returns one whole-document edit when changes are needed. `textDocument/codeAction` exposes machine-applicable diagnostic fixes as preferred quick-fix workspace edits, preserving the compiler's exact replacement span/text instead of reparsing error messages. `fluxc format --check` remains the non-mutating CI formatting contract. Cached/incremental workspace analysis and cross-module navigation/completion/refactoring remain later work. The debugger will need source-to-native debug metadata, expression evaluation rules, and predictable optimized-build behavior. The profiler should support low-overhead instrumentation and symbolization without requiring a separate language parser.
