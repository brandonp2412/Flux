# Flux Roadmap

Flux aims to provide the practical application-development reach of Flutter while remaining a native, ahead-of-time compiled, function-first language. The language is built around functions, immutable data, explicit state, interfaces/capabilities, strict static typing, and composition. It does not use classes, inheritance, widget subclasses, mixins, controller hierarchies, or exception-based recoverable errors.

## Roadmap maintenance and progress

This file is the single source of truth for Flux development and overall progress. Do not invent a separate percentage in individual sessions.

Stable overall-progress baseline:

- Baseline date: 2026-09-10
- Baseline commit: `7d219b1`
- Baseline roadmap units: 739 checkbox items
- Completed baseline units: 462
- Reported overall progress: **63%**
- Current net scope growth after the baseline: 53 checkbox items
- New roadmap items after the baseline are scope growth and do not increase the 739-unit denominator.

At the **start of every Flux development session**:

1. Read `ROADMAP.MD` before choosing work or reporting progress.
2. Inspect the repository state and run the existing test suite.
3. Update this roadmap to reflect the actual repository state, newly discovered work, changed sequencing, or completed prerequisites before or alongside implementation.
4. Choose one coherent highest-value unchecked subsystem that advances the current milestone.
5. Validate the change with compiler tests and native end-to-end examples where applicable.
6. Check off completed items and record meaningful follow-up work before committing.
7. Increase `Completed baseline units` only for verified work corresponding to an item that existed in the 739-unit baseline. Record newly discovered items as scope growth instead.
8. The reported overall percentage is rounded `Completed baseline units / 739 * 100` and must never decrease; reopening old work becomes follow-up/scope growth rather than rewriting completed history.
9. Commit roadmap/progress updates together with the implementation they describe when practical.

Do not let the roadmap become an aspirational document that diverges from the compiler.

## Priority 1 — Minimal end-to-end manual dogfooding

**This is the immediate project priority.** Until this milestone is complete, prefer work that directly enables a developer to write a small Flux GUI program in a real editor, receive first-class Flux language tooling, compile it, launch it as a real native application, interact with it, and visually verify the result. Other roadmap work should be sequenced behind this unless it is a prerequisite for this path or fixes a correctness regression in an already-supported feature.

The minimum acceptance path is deliberately small: one Linux desktop target first, one native window, the existing flat grid layout, `Text` and `Button`, one click-driven state update/repaint, and the existing save-triggered development loop. This milestone does **not** require completing the full UI toolkit, ownership model, Android target, animation, navigation, package ecosystem, debugger, or profiler before manual dogfooding starts.

- [x] **P0 manual E2E:** the first dogfood path is now assembled and individually validated on a Linux Wayland development host: open `examples/hello_app.flux` through `tools/flux-nvim`, receive Flux syntax plus attached semantic LSP services, run it via `:FluxRun`/`flux run`, get the native GTK4/Wayland window, click the typed state-transition button, and rely on the existing save-triggered controlled-restart loop. The canonical manual sequence and recovery checks live in `docs/manual-e2e.md`; state-preserving in-process Fast Refresh remains explicitly beyond this minimal P0 gate.
  - [x] Compiler-backed flat `view`/grid syntax with typed built-in UI properties and composed views.
  - [x] `flux lsp` foundation (plus `fluxc` compatibility) with diagnostics, semantic highlighting, completion, hover, signature help, navigation/refactoring, formatting, fixes, inlay hints, and UI syntax awareness.
  - [x] `flux run` automatic save detection with rebuild and safe controlled restart on successful recompilation.
  - [x] Minimal application entry/runtime that opens a real native Linux window from a Flux program: bootstrap `app ViewName` selects a zero-parameter root view and replaces `fn main`; the Linux bootstrap lowers it to a GTK4 application/window while keeping GTK objects out of Flux source.
  - [x] Bootstrap native lowering/render path for the existing flat grid: explicit rows/columns/spans/gap lower to `GtkGrid`, fixed tracks feed native size requests, and `fr` tracks expand in the corresponding axis. A future lower-level renderer may replace GTK without changing Flux grid source.
  - [x] Render usable `Text` and `Button` primitives with native GTK4 controls, typed literal/constant/state-derived properties, spacing/margins, and a checked-in `examples/hello_app.flux` dogfood app.
  - [x] Pointer/click dispatch from the native Wayland window to a typed Flux `Button.onPress` callback: the development-host dogfood launch produced the Flux callback output from real button activations.
  - [x] Minimal functional local UI state and invalidation/repaint: explicit `state clicked: bool = false` belongs to the view instance, `onPress: clicked => !clicked` is a typed state transformation, state-dependent Text/Button properties lower to a native refresh function, and the Wayland dogfood test toggled the app render and then restored it pixel-identically on the second activation (apart from unrelated desktop-panel changes).
  - [x] Editor integration/setup for the first dogfood editor: checked-in dependency-free Neovim runtime detects `.flux`, supplies immediate syntax colouring, launches `flux lsp` for semantic highlighting/full editor services, and exposes `:FluxRun`; `tools/flux-nvim` starts this experience without modifying global editor configuration.
  - [x] Checked-in `examples/hello_app.flux` manual acceptance app plus `docs/manual-e2e.md` documents the editor → compile/run → native window → click/state update → save-triggered rebuild/restart path, including failure/recovery expectations and non-interactive preflight checks.

When choosing work, pull the smallest prerequisites from later milestones forward as needed to finish this P0 path. Do not expand those prerequisites into their full eventual subsystem before the manual E2E is working.

## Non-negotiable language/product principles

- [x] Python-like readability with low ceremony.
- [x] Function bodies use `{}`.
- [x] Control-flow blocks use indentation.
- [x] Strict static typing.
- [x] No comments in Flux source; `#...` outside strings is a syntax error rather than a comment form.
- [x] No lint/warning tier for source cleanliness: violations such as unused named bindings/parameters/loop or pattern variables are compile errors. A leading `_` explicitly marks an intentionally ignored binding.
- [x] No ternary/conditional-expression syntax; value selection uses control flow or value-producing `match` rather than `a if condition else b`/`?:` forms.
- [x] Namespace, enum, interface, and platform qualification uses dot syntax (`Outcome.Ok(...)`, `Storage.load(...)`, `android.vibrate(...)`).
- [x] No source-language generics.
- [x] No `try` / `catch` / exceptions for recoverable failures.
- [x] Explicit multi-value returns and `error` values form the recoverable-error model.
- [x] Functions, data, and interfaces are the primary abstraction model.
- [x] No classes or inheritance in Flux.
- [x] No mixins or class-oriented extension hierarchy.
- [x] No widget/controller/view-model object hierarchies in UI code.
- [x] Immutable values by default; local mutation is explicit and locally visible through typed `var` bindings and assignment; ordinary `let` bindings, parameters, destructured bindings, and loop variables remain immutable.
- [x] **Beautiful by default is a non-negotiable product goal:** ordinary Flux UI written by a developer with little or no design expertise renders through compiler-owned semantic typography, spacing, sizing, color, interaction, accessibility, and presentation-state defaults without requiring a hand-built design system; explicit application styling remains the straightforward higher-priority escape hatch.
- [ ] Rust-class memory-safety goals without requiring Rust-level annotation density in normal application code.
- [ ] Rust-class native performance is a non-negotiable release-build target: generated programs should remain in the C/C++/Rust performance class without a mandatory VM, garbage collector, interpreter, or framework runtime tax.
- [ ] Zero-cost abstractions by default: high-level language/library/UI constructs must compile away when their semantics require no runtime work, and redundant bounds/safety/dynamic-dispatch checks should be eliminated when statically proven unnecessary.
- [x] Native AOT output for application and server/headless targets: current Linux application and headless `fn main` builds compile through optimized native Clang output with no VM/interpreter, while Android applications produce ABI-specific native shared libraries packaged into APK/AAB artifacts. Future backend replacement remains tracked under the typed-IR/native-backend milestone rather than reopening the AOT product contract.
- [x] Portable source lowers to real platform implementations rather than a browser/VM/widget-emulation wrapper on the current application targets: the same flat Flux UI lowers to native GTK4/Wayland controls on Linux and native Android Views, while target-specific APIs remain explicit and unreachable target-only code tree-shakes without poisoning portable builds. Future targets must preserve this native-platform contract rather than reopening the product principle.
- [x] **No user-written platform bridges:** Flux source calls current Android platform capabilities and native UI/lifecycle callbacks directly; the compiler/toolchain owns the required JNI/JVM/DEX/native entry glue, with regression coverage proving direct callbacks and no method-channel, serialization-bus, plugin-registry, or application-authored Java/Kotlin/JNI requirement. Future platform bindings must preserve this source-level contract.
- [x] **Native-first platform lowering:** current Linux UI lowers directly to GTK4/native Wayland integration, Android uses direct NDK APIs where available (including API-level/lifecycle entry points) and compiler-generated direct JNI/framework calls only where Android requires JVM APIs, and unreachable helpers are tree-shaken. Future backends remain required to use the thinnest optimizable native boundary rather than a mandatory framework runtime.
- [x] The LSP, debugger, profiler, formatter, test runner, package tooling, and hot reload are first-class parts of the language product: all are exposed through the user-facing `flux` workflow, reuse compiler-native source/diagnostic/build semantics rather than separate framework layers, and have checked-in regression coverage. Production-depth follow-ups remain tracked in their dedicated milestones without reopening this product-integration principle.

## Current compiler baseline

Implemented on `main`:

- [x] Rust bootstrap compiler.
- [x] Functions and explicit return types.
- [x] Primitive `i64`, `bool`, `str`, `error`, and `void` types.
- [x] Checked `i64` arithmetic semantics: addition, subtraction, multiplication, unary negation, and division lower through explicit overflow/invalid-operation guards; constant/default evaluation diagnoses the same failures at compile time.
- [x] Strict typed bindings.
- [x] Expressions and function calls.
- [x] Indentation-based `if` / `elif` / `else`.
- [x] Integer range `for` loops with exclusive `start..end` and inclusive `start..=end` forms; inclusive lowering preserves `continue` semantics and avoids signed overflow at `i64::MAX`.
- [x] Multi-value function returns.
- [x] Strict typed destructuring of multi-value calls.
- [x] Exact multi-value return forwarding.
- [x] Built-in `error`, `nil`, and `error("message")`.
- [x] Explicit `else return` recoverable-error propagation.
- [x] Structured parse/type/codegen diagnostics.
- [x] Token-level expression source spans.
- [x] Multi-diagnostic parser recovery across malformed functions with batch check diagnostics.
- [x] Multi-diagnostic type checking with safe statement/function continuation and cascade suppression.
- [x] Stable source IDs carried through AST spans and diagnostics, with caller-supplied or deterministic IDs.
- [x] Structured diagnostic labels, notes, and machine-applicable fixes.
- [x] Machine-readable JSON diagnostic output from the CLI/library.
- [x] Human terminal diagnostics with source excerpts, exact carets, secondary labels/fixes, automatic ANSI color, `NO_COLOR`/forced-color support, and terminal-width-aware wrapping/cropping.
- [x] Bootstrap C lowering through optimized Clang native compilation.
- [x] Native end-to-end examples and compiler tests.

---

# Milestone 1 — Complete the compiler/editor foundation

The parser and semantic layer must become robust enough that every later subsystem can share the same source model.

Milestone 1 is complete. Parser recovery synchronizes at function boundaries, type checking batches independent errors while suppressing misleading cascades, diagnostics carry stable source identity plus rich JSON metadata, and all declaration/control-flow/expression syntax retains precise spans. Editor consumers can reuse parse snapshots and query one analyzed semantic database for symbols/signatures. Flux deliberately has no comment syntax; the canonical formatter therefore formats code only, normalizes indentation/expression spacing, is idempotent, and supports CI check mode. Development can now move into the functional language core.

- [x] Multi-diagnostic parser recovery instead of stopping at the first syntax error.
- [x] Multi-diagnostic type checking where safe to continue.
- [x] Stable file/source identifiers in spans, not only line/column.
- [x] Full token ranges for declarations, bindings, types, and control-flow constructs.
- [x] Diagnostic labels/notes/fix suggestions.
- [x] Machine-readable diagnostic output.
- [x] Human terminal diagnostics render source excerpts, precise labels/fixes, ANSI color when appropriate, and width-aware wrapping/cropping without changing JSON diagnostics.
- [x] Incremental parse snapshot API suitable for editor use, with stable source identity, source fingerprints, unchanged-source reuse, and reparsing on edits.
- [x] Semantic query database shared by compiler and LSP, indexing function/parameter/binding/loop symbols and analyzed signatures from one typed program.
- [x] Deterministic canonical Flux formatter, exposed through `fluxc format` and `fluxc format --check`; comments are rejected by the language rather than preserved by formatting.

# Milestone 2 — Functional language core

Flux should be capable of modelling substantial programs without object-oriented machinery.

## Data and algebraic types

- [x] `struct` value types with named fields, nested acyclic value layout, native C lowering, formatter support, and semantic indexing.
- [x] Struct literals, field access, and functional update/copy syntax with single evaluation of the base value.
- [x] Closed enums / tagged unions with typed positional payloads, namespace-qualified constructors, semantic indexing, deterministic native tagged-union lowering, and cross-value-type cycle diagnostics.
- [x] Exhaustive `match` expressions and statements.
  - [x] Exhaustive enum `match` statements with typed payload bindings, duplicate/missing-arm diagnostics, guaranteed-return analysis, formatter/semantic support, single scrutinee evaluation, and native `switch` lowering.
  - [x] Value-producing enum `match` expressions for typed bindings and returns, using indentation-based arms, exhaustive/type-consistent results, pattern bindings/struct patterns, formatter/semantic support, and native `switch` lowering.
- [x] Pattern matching over structs, enums, lists, and narrow function multi-values.
  - [x] Enum variant payload patterns in `match` statements, including `_` ignored payload positions.
  - [x] Flat struct destructuring patterns with shorthand fields, renaming, ignored bindings, inferred field types, aliases, and single source-expression evaluation.
  - [x] Nested struct destructuring patterns with recursive type validation, formatter/semantic support, and single root-expression evaluation.
  - [x] Struct patterns inside enum `match` arms, including nested struct patterns, static payload-type validation, formatter/semantic support, and direct native field projection.
  - [x] Exact local list destructuring bindings with `let [first, _, last] = source`: bindings infer the concrete `T[]` element type, the source is evaluated once, strided views preserve logical order, `_` ignores positions, exact runtime length is checked explicitly, and formatter/semantic tooling preserve the pattern.
  - [x] Local prefix/rest list patterns with `let [first, ...middle, last] = source`: one `...rest` binding infers `T[]`, captures the unmatched middle as a zero-copy strided view, supports `..._`, and requires only the fixed prefix/suffix arity at runtime.
  - [x] Direct list patterns in statement and value-producing `match`: exact `[]`/`[a, b]`, prefix/rest/suffix `[first, ...middle, last]`, and `_` fallback arms are statically exhaustive over list lengths, reject unreachable arms, infer `T`/`T[]` bindings, preserve strided-view order, evaluate the scrutinee once, and lower rest bindings as zero-copy views.
  - [ ] Map/record patterns after those value types exist.
  - [x] Inferred multi-value patterns outside typed call destructuring with `let (first, _, last) = expression`: arity and positional types come from the function return shape, `_` ignores positions, the source is evaluated once, shell-style calls/pipelines are supported, and `else return` retains exact error-shape forwarding.
- [x] Destructuring bindings beyond typed multi-return calls: struct, exact/rest list, and inferred narrow multi-value `let` patterns are supported, including direct exhaustive list `match`; broader map/record patterns remain tracked above.
- [x] Transparent concrete type aliases with forward/chained resolution, cycle diagnostics, formatter/tooling support, and zero runtime representation.
- [x] Compile-time constants with forward references, constant folding, cycle diagnostics, alias-aware typing, and zero-runtime-storage lowering for `i64`, `bool`, and `str`.

## Control flow

- [x] `break` / `continue` in `for` loops with static scope validation through nested `if` / `match` blocks and native lowering.
- [x] Indentation-based `while` loops with statically boolean conditions, loop-scoped `break` / `continue`, and native lowering; useful mutation is explicit through typed `var` bindings rather than making ordinary bindings mutable.
- [x] Value-producing `match` expressions for functional code.
- [x] Ternary/conditional-expression syntax intentionally removed; ordinary `if`/`elif`/`else` controls statement flow and exhaustive `match` is the value-producing selection form.

## Interfaces and capabilities

The interface/capability foundation is complete for the bootstrap compiler: declarations, implementations, static dispatch, dynamic interface values, and composition all share one non-OOP contract model. The remaining platform-capability item is intentionally sequenced behind the first real non-Linux platform backend rather than blocking the current function-language work.

- [x] `interface` declarations consisting of statically validated function signatures/capabilities, with named-only parameter metadata, formatter/semantic support, namespace diagnostics, and zero-runtime lowering until implementation/dispatch semantics are introduced.
- [x] Explicit implementation of interfaces by concrete data types without inheritance, using exhaustive capability-to-free-function mappings with concrete first-parameter receiver validation, exact named-parameter/return contracts, formatter/semantic support, and zero-runtime conformance metadata. Module implementations remain naturally downstream of modules/packages.
- [x] Static interface dispatch where the concrete receiver type is known, using `Interface.capability(receiver, ...)` syntax and compile-time lowering directly to the mapped free function, including named arguments and multi-value returns with no vtable/dynamic lookup.
- [x] Efficient bootstrap interface/dynamic dispatch for runtime polymorphism: interface values are closed-world tagged by-value unions and `Interface.capability(interfaceValue, ...)` lowers to a tag switch calling the mapped free function, with no heap allocation, vtable, reflection, or object identity. Cross-module/stable ABI refinement remains part of later IR/ABI work.
- [x] Interface values usable as function arguments and return values via explicit `Interface(concrete)` packing; bindings, statement-level conditional selection, scalar returns, named capability arguments, and multi-value dynamic dispatch are supported. Struct/enum embedding remains intentionally blocked until ownership/layout ABI rules mature.
- [x] Interface composition without class inheritance using `interface Child: ParentA, ParentB { ... }`; compatible capabilities flatten into one contract for implementation/static/dynamic dispatch, while unknown parents, cycles, duplicate parents, and conflicting inherited signatures are rejected.
- [ ] Platform capabilities expressed through interfaces so portable code does not import Android/iOS/etc APIs directly. Target-specific Flux code may also import first-class platform modules such as `android.*` directly when it intentionally needs Android-only functionality. In both cases the compiler owns native interop; user-written bridge layers are not part of the programming model. Modules/imports now exist; this is blocked on the first real platform backend. Do not invent placeholder platform APIs before one exists.

## Functions

- [x] Named functions as first-class values: typed function references can be stored, passed, returned, and invoked through native function-pointer lowering.
- [ ] Closures with safe capture semantics. Outer-local capture from anonymous functions is deliberately rejected until ownership/lifetime rules can make the environment representation safe rather than silently heap-boxing it.
- [x] Capture-free anonymous functions using `fn(value: i64) { value * 2 }` with typed positional parameters, optional explicit scalar/`void` return annotations, inferred scalar/`void` returns, formatter/semantic support, direct higher-order-call use, and deterministic native helper/function-pointer lowering. Anonymous parameters obey the same unused-binding compile-error rule as named functions.
- [x] Concise single-expression function syntax using `fn name(args) -> type { expression }`, preserving brace-delimited bodies while lowering through the ordinary implicit return/type-check/native ABI path, including exact multi-return call forwarding.
- [x] Named-only parameters using a `*` delimiter, with statically validated named call arguments and native ABI reordering.
- [x] Optional positional/named parameters with explicit compile-time primitive defaults, including top-level constant references and call-site default expansion.
- [x] Required named parameters: named-only parameters without defaults must be supplied explicitly.
- [x] Function type aliases through concrete `type Mapper = fn(...) -> ...` aliases; first-class function values currently support zero or one return value.
- [ ] Partial application / ergonomic function binding if it remains predictable and allocation-free where possible.
- [x] Typed value pipeline/composition syntax with `|`, passing the previous result as the next call's first argument; compiler-known list transforms additionally fuse where profitable without changing source semantics.

## Modules/packages

- [x] Modules and imports using explicit relative `.flux` paths, deterministic recursive loading, duplicate suppression, preserved per-file source IDs, transitive merged semantic/type checking, formatter support, and project-aware CLI check/build/emit-C. Public cross-module declarations are reachable only through the importing module's explicit transitive forward import graph; loading two sibling modules into one project no longer leaks their public symbols to each other.
- [x] Explicit public/private visibility: top-level functions, constants, aliases, structs, enums, and interfaces are module-private by default; `pub` exports cross-file API, public signatures cannot leak private named types, formatter/source spans preserve visibility, and imports/implementation blocks cannot be `pub`.
- [x] Package manifest (`flux.toml`) with dependency-free `[package]` parsing, required `name`/`entry`, optional `version`, strict schema diagnostics, validated package-root containment, checked-in native example coverage, and project-aware CLI targeting by source file, package directory, or manifest path.
- [x] Stable module naming rules: imports use normalized relative `.flux` paths without `.`/`..` segments, package imports cannot escape the manifest root, and every loaded source receives a deterministic package-qualified module name derived from `[package].name` plus its root-relative source path (direct source projects use the entry-directory-relative path).
- [x] Qualified names consistently use dot syntax; `.` distinguishes namespace/type qualification by static context while remaining ordinary value field access for values.
- [x] Cyclic dependency diagnostics with the cycle reported at the importing path span and the resolved cycle chain attached as a note.
- [ ] Package-level constants/resources.
- [ ] Stable ABI rules for native/library boundaries.

---

# Milestone 3 — Dart-inspired ergonomics without Dart's OOP model

Dart has excellent expression and collection ergonomics. Flux should adopt or adapt the useful parts while rejecting class-centric semantics. Exact syntax is subject to Flux consistency and static-safety review.

Reference set (current Dart language documentation):

- https://dart.dev/language/operators
- https://dart.dev/language/collections
- https://dart.dev/language/patterns
- https://dart.dev/language/records
- https://dart.dev/null-safety/understanding-null-safety

## Null/optional-value ergonomics

Flux currently reserves `nil` specifically for `error`; a general optional-value design must therefore be explicit and must not weaken type safety.

- [ ] Optional value types, likely `T?`, with non-optional types remaining the default.
- [ ] Null/optional-aware access analogous to Dart `?.`.
- [ ] Optional-aware indexing analogous to Dart `?[]`.
- [ ] Coalescing analogous to Dart `??`.
- [ ] Coalescing assignment analogous to Dart `??=` where mutation is valid.
- [ ] Safe optional unwrapping via patterns/flow analysis.
- [ ] Avoid routine runtime null assertions; any force-unwrap equivalent must be explicit and discouraged.
- [ ] Flow-sensitive promotion after optional/type checks.

## Indexing and collection conveniences

- [x] `value[index]` indexing with strict index typing for bootstrap lists, including Python-style negative indices with checked bounds.
- [ ] Optional indexing (`value?[index]` or Flux-equivalent) for optional receivers.
- [x] Checked `first` and `last` conveniences for compiler-known lists; empty access fails with an explicit Flux bounds diagnostic.
- [x] Checked `single` / exactly-one validation for compiler-known lists; any length other than one fails explicitly at runtime.
- [x] `length` and `isEmpty` / `isNotEmpty` conveniences for compiler-known lists, exposed as zero-cost properties rather than methods.
- [x] Full Python-style list slices/ranges: `[start:end]` and `[start:end:step]`, omitted/negative bounds, positive or negative steps, `[::-1]` reversal, clipping, and explicit zero-step rejection. Native slices remain zero-copy through strided list views.
- [x] Typed list iteration with `for value in list:` and indexed `for index, value in list:` forms; the source is evaluated once and the index is `i64`.
- [x] Typed `map(list, callback)`, `filter(list, predicate)`, and `where` alias with exact concrete callback signatures, strided-source support, pipeline composition, and stack-backed bootstrap results; chained transforms fuse into one source loop and one final result buffer.
- [x] Related collection-producing functional sequence operations beyond map/filter/where.
  - [x] Typed `concat(left, right)` with exact element-type matching, strided-input support, pipeline composition, overflow checking, and a stack-backed local result.
  - [x] Stable `distinct(list)` for scalar-equality lists (`i64`, `bool`, `str`, `error`), preserving first-occurrence order across strided inputs and composing through typed pipelines.
  - [x] One-level `flatten(nested)` for concrete nested lists, preserving logical order across strided outer/inner lists, checking total length overflow, and producing a stack-backed local result.
  - [x] Immutable stable `sorted(list)` for ordered scalar lists (`i64`, `bool`, `str`), preserving the source, consuming strided inputs in logical order, and producing a stack-backed ascending result that composes through pipelines.
  - [x] Zero-copy `chunked(list, size)` producing `T[][]` from strided inputs, with a positive `i64` size contract, shorter final chunk, empty-input identity, and stack-backed chunk descriptors that reference the original list storage.
- [x] Typed `fold(list, initial, reducer)` and `reduce(list, reducer)` scalar reductions with concrete callback signatures, strided-view support, pipeline composition, and explicit empty-list rejection for `reduce`.
- [x] `any(bool[])` / `every(bool[])` boolean aggregation with Python/Dart-style empty-sequence identities (`false` / `true`), designed to combine with boolean comprehensions and typed pipelines.
- [x] Zero-copy `take(list, count)` / `skip(list, count)` sequence views, designed to compose naturally through typed pipelines such as `values | skip 1 | take 3`; counts are `i64`, oversized counts clamp, and negative counts fail explicitly at runtime.
- [x] Lazy pipeline fusion where it avoids needless allocations: chained `map`/`filter`/`where` stages share one loop and terminal `fold`/`reduce` consumes the transformed stream directly without an intermediate list.

`first`, `last`, etc. may use property-like syntax on compiler-known collection types while lowering to ordinary functions. This is syntax ergonomics, not an object model.

## Cascades / fluent mutation without builder objects

Dart's `..` cascade is useful because it avoids repeating a target. Flux should keep the ergonomic idea but apply it to values/functions rather than encouraging mutable object builders.

- [x] Functional cascade syntax analogous to `..` for repeated transformations on one target: `value .. transform args .. finish` is typed as the equivalent value pipeline, with parenthesized or shell-style function stages supported and canonical formatting normalized to `|`.
- [ ] Optional-aware cascade analogous to `?..`.
- [x] Cascades produce successive transformed values rather than introducing hidden mutation: each stage receives the previous stage result as its first argument and ordinary Flux call typing applies.
- [x] Cascades use the existing functional pipeline lowering, adding no builder object, allocation, dynamic dispatch, or distinct runtime abstraction.
- [x] Cascade expressions evaluate the original target exactly once; lowering nests the same single-evaluation typed call flow used by pipelines.

## Collection literals and control flow

- [x] List literals with homogeneous static element typing and concrete `T[]` source spelling (no source generics).
- [ ] Set literals.
- [ ] Map literals.
- [x] Spread elements in list literals with `...source`; spread sources must have the exact surrounding `T[]` type, are evaluated once in source order, preserve strided-view semantics, and lower into one overflow-checked stack-backed result buffer.
- [ ] Optional-aware spread analogous to Dart `...?`.
- [ ] Null/optional-aware collection elements analogous to modern Dart `?element`.
- [x] List-only `if condition: value` / `if condition: first else: second` construction items with strict bool conditions, exact element typing, selected-branch evaluation, and the same overflow-checked stack-backed builder used by spreads.
- [x] Python-style list comprehensions with optional boolean filters: `[expr for item in source if condition]`.
- [ ] Pattern guards inside collection construction where useful.
- [x] Bootstrap list literals/slices/comprehensions lower directly to native list descriptors; slices are zero-copy and comprehensions use stack-backed output sized from the source list without intermediate collections.

## Records, patterns, and destructuring

Flux intentionally keeps function multi-return values narrow today. General records should be added only when they do not blur explicit multi-return/error semantics.

- [ ] Anonymous immutable records for lightweight aggregate data.
- [ ] Named record fields.
- [ ] Positional record fields.
- [ ] Record destructuring patterns.
- [x] List patterns with exact/rest/wildcard shapes in local bindings plus exhaustive statement/value `match`, including zero-copy rest views over strided lists.
- [ ] Map patterns.
- [x] Struct patterns for bindings and enum match payloads; map/record pattern forms remain pending with their value types.
- [x] Enum/tagged-union variant patterns with typed positional payload bindings, ignored positions, nested struct payload patterns, and exhaustive statement/value matching.
- [x] Relational/logical scalar enum-payload patterns where readable: `==`, `!=`, `<`, `<=`, `>`, and `>=` compare against compile-time primitive constants, `&&`/`||` compose relational tests, statement/value matches share native lowering and formatter support, and refutable pattern arms remain conservatively non-exhaustive until a later irrefutable arm covers the variant.
- [x] Boolean pattern guards on enum/list statement and value-producing `match` arms, with pattern bindings in scope, formatter/semantic support, native lowering, guarded-arm fallthrough, conservative exhaustiveness, and unreachable-arm diagnostics after an unguarded covering arm.
- [x] Exhaustiveness checking for the implemented closed enum and list-length pattern domains, including unreachable/duplicate-arm rejection.
- [x] Pattern-based mutable declarations and assignments beyond `let` destructuring: typed multi-value, inferred multi-value, exact/rest list, and nested struct patterns may declare copyable targets with `var`, while corresponding multi-value/list/struct assignment patterns update existing mutable bindings after exact static type checks. Pattern RHS values are evaluated once, list assignments preserve exact/rest cardinality checks and strided order, struct assignments project nested fields directly, `_` remains discard-only, non-copy mutable captures remain rejected until full move/borrow semantics land, and formatter/semantic/CFG/native lowering share the same forms.

## Expression ergonomics

- [x] Shell-inspired typed call flow without subprocess semantics:
  - [x] Parentheses-free function calls such as `print "hello"` and `scale value 2`.
  - [x] Typed value pipelines such as `source | transform | consume`, passing each return value as the next call's first argument.
  - [x] `>` / `>>` scalar-result redirection to a typed `str` path, truncating or appending respectively.
  - [x] Trailing `&` detached background execution for call/pipeline statements; the current native bootstrap lowers this through POSIX process primitives only when a program actually uses background execution.
- [ ] String interpolation. Runtime interpolation is sequenced after owned-string/buffer semantics: the bootstrap `str` currently lowers to borrowed native string pointers, so Flux must not introduce hidden scratch-buffer allocation/lifetime behavior merely for interpolation syntax. Compile-time interpolation may arrive earlier only if it shares the eventual syntax and semantics cleanly.
- [x] Multiline strings with triple-quoted `"""..."""` syntax, shared-indent stripping for normal indented source, ordinary escape semantics, formatter canonicalization, and LSP string-token/signature scanning support.
- [x] Raw strings with Python-like `r"..."` syntax: backslashes are literal, the value remains ordinary `str`, semantic highlighting treats the whole literal as a string, and canonical formatting normalizes it to equivalent escaped ordinary-string syntax.
- [x] No ternary expression form: Python-style `a if condition else b` and C-style `?:` are intentionally absent; use statement control flow or `match` values.
- [x] No separate Dart-style `~/`: Flux `i64` `/` already truncates toward zero and is checked for division by zero and `i64::MIN / -1`, so a second operator would duplicate semantics.
- [x] Concise inclusive integer ranges with `start..=end` alongside exclusive `start..end`, including single-evaluation bounds and overflow-safe native iteration.
- [x] Match expressions returning values; broader switch-like pattern expression ergonomics can build on the same lowering.
- [x] Assignment expressions are intentionally not part of Flux: assignment remains a statement restricted to explicitly mutable `var` bindings, keeping mutation locally visible and preventing hidden side effects inside conditions/calls/larger expressions. A standalone `=` inside an expression now produces a targeted parse diagnostic while `==` remains ordinary equality.
- [x] Strong primitive constant folding for literal/constant expression trees in ordinary code and compile-time UI/application metadata, with checked integer semantics, comparisons/equality, and boolean short-circuiting; runtime-dependent expressions remain dynamic.

## Async/generator ergonomics

Adapt the convenience of Dart's async language features while preserving Flux's explicit error-return model.

- [ ] Async functions.
- [ ] `await` or Flux-equivalent suspension syntax.
- [ ] Async failures returned explicitly, not thrown as exceptions.
- [ ] Streams/sequences as explicit types/capabilities.
- [ ] Generator functions.
- [ ] `yield` / `yield from` equivalent if it fits ownership rules.
- [ ] Cancellation as an explicit capability/value.

## Dart features intentionally not copied

- [x] No classes.
- [x] No inheritance.
- [x] No mixins.
- [x] No abstract base classes as the polymorphism foundation; use interfaces.
- [x] No factory constructors or constructor hierarchy as an architectural pattern.
- [x] No class extension methods as an object-system feature; prefer free functions, interfaces, and compiler-known ergonomic sugar.
- [x] No exception-based async errors.
- [x] No source-language generics.

---

# Milestone 4 — Ownership and memory safety

The ownership track now has an explicit compiler-level copyability classification rather than relying on ad-hoc list checks. Bootstrap scalar handles (`i64`, `bool`, `str`, `error`), function values, closed interface values, and recursively copyable structs/enums are `Copy`; `void` is not a value and local list/view types are deliberately non-copy until owned collection storage and move/borrow rules exist. Existing mutable-binding and aggregate-storage safety gates query that classification, so future non-copy value types inherit the conservative restrictions automatically. Direct non-copy moves are represented on the semantic CFG and a deterministic forward may-analysis computes moved-value state at every reachable CFG node; use-after-move diagnostics consume those normalized path facts rather than maintaining a second syntax-shaped branch/loop ownership state machine. Borrowed list-parameter aliases are likewise reclassified from normalized reaching-definition/value provenance before move-state propagation, including transitive aliases, so the bootstrap borrow checker no longer carries a parallel syntax-tree reborrow analysis. Zero-copy local list views now also participate in CFG-liveness ownership validation: a list owner cannot move while a derived slice/indexed-list/`first`/`last`/`single`/`take`/`skip`/`chunked` view or a list-typed binding projected by list destructuring/matching remains live, while a move after the view's final use stays valid.

- [x] Define copyable primitive/value types with alias-aware structural classification shared by semantic validation.
- [ ] Move semantics for non-copy values.
  - [x] Bootstrap direct local transfers (`let destination: T = source`) move non-copy values, preserve ordinary reuse for `Copy` types, and conservatively merge moves across `if`/exhaustive `match` branches.
  - [x] Bootstrap loop control-flow recognizes iteration-safe direct moves through CFG back-edges: fallthrough and `continue` paths expose the moved source on a later iteration and fail, while paths that leave the loop through `break`/`return` do not create a repeated read.
  - [x] Return-terminated ownership paths do not poison reachable sibling/post-loop state because move facts propagate only over actual CFG successors; zero-iteration loop exits likewise preserve the pre-loop state where appropriate.
  - [x] Nested direct-move loop analysis follows composed CFG edges across `if`/`match`, nested loops, branch-specific `break`, and `continue`, removing the former syntax-shaped loop-exit ownership bookkeeping.
  - [x] Bootstrap direct-local move path-state is computed to a fixed point on the semantic CFG with deterministic move origins and reachability facts; compile-time primitive conditions and statically empty integer ranges remove impossible CFG edges before ownership propagation so dead moves do not poison live owners.
  - [ ] General move analysis across calls, aggregate fields, partial moves, and owned resource types after normalized control-flow ownership facts exist.
- [ ] Immutable borrows.
  - [x] Bootstrap local list/view reads, indexing/slicing, transforms, iteration, matching, and list function parameters are non-consuming immutable borrows of their local owner; borrowed list parameters can be rebound through direct local alias chains without incorrectly moving either the parameter or an earlier borrowed alias.
  - [x] Reachable non-copy name reads are represented explicitly as immutable-borrow facts on normalized CFG ownership nodes, derived from typed IR value reachability rather than syntax-wide read collection. Use-after-move validation consumes those borrow facts, so compile-time-dead nested regions such as a short-circuited boolean RHS do not spuriously borrow a moved owner while runtime-reachable borrows still fail deterministically.
  - [ ] First-class borrow/reference types and inferred borrow lifetimes for owned values.
- [ ] Exclusive mutable borrows.
- [ ] Lifetime inference.
- [ ] Prevent use-after-move.
  - [x] Reject later reads of a non-copy local after a direct bootstrap move, including conservative post-branch invalidation for paths that can rejoin later code.
  - [x] Direct-local use-after-move diagnostics are driven by CFG/IR move-state before each normalized evaluation node, including loops, branch joins, `break`, `continue`, and returns.
  - [ ] Extend CFG/IR-wide move-state analysis to consuming calls, partial moves, future owned aggregates, and owned resource types.
- [ ] Prevent dangling borrows.
  - [x] Bootstrap live-view move exclusion for zero-copy list borrows: CFG liveness rejects moving a local list owner while a derived slice, nested-list index/property (`first`/`last`/`single`), `take`, `skip`, `chunked`, list-rest pattern, list-match rest, destructured nested-list element, or transitive view binding remains live, while permitting the move immediately after the borrow's last reachable use. Full first-class lifetime inference and escaping-borrow validation remain pending.
  - [x] Follow-up: value-producing enum/list `match` results preserve zero-copy list-view provenance through the selected arm, so a live match-selected view prevents moving its owner while a move after the view's last use remains valid.
- [ ] Prevent aliasing violations.
- [ ] Deterministic destruction/resource release.
- [ ] Safe closures/captures under the ownership model.
- [ ] Thread-safety/sendability rules.
- [ ] Narrow, explicit `unsafe` escape hatch only where necessary.
- [x] First bootstrap borrow checker over normalized semantics/IR rather than parser syntax: direct non-copy moves, immutable non-copy reads, borrowed list-parameter reborrows, path-sensitive move state, and use-after-move diagnostics are derived from CFG/typed-value/reaching-definition facts. Full borrow/reference types, inferred lifetimes, mutable aliasing, dangling-borrow prevention, partial moves, and drop semantics remain tracked explicitly above rather than being implied complete.

# Milestone 5 — Standard library and built-in concrete containers

Flux has no generics, so common containers need compiler-known/specialized implementations and ergonomic concrete typing.

- [ ] Owned string type plus immutable string views/slices.
- [ ] Arrays/lists.
  - [x] Bootstrap immutable local `T[]` lists with literals, checked indexing, zero-copy slicing, and filtered comprehensions.
  - [ ] Owned/escaping list storage, owned list argument/return transfer, aggregate fields, mutation, and lifetime/ownership integration; current list parameters are immutable borrowed views.
- [ ] Maps.
- [ ] Sets.
- [ ] Byte buffers.
- [ ] Paths and filesystem APIs.
  - [x] Bootstrap compiler-owned filesystem capabilities: `fs.exists`, `fs.isFile`, `fs.isDirectory`, `fs.createDirectory`, recursive `fs.createDirectories`, `fs.removeFile`, `fs.removeDirectory`, recursive `fs.removeDirectories`, `fs.writeText`, `fs.appendText`, `fs.rename`, and buffered `fs.copyFile` are strictly typed, lower directly to native/POSIX filesystem primitives, return explicit Flux `error` values for mutations, participate in LSP qualified completion/signature help, and tree-shake completely when unreachable. `fs.createDirectories` uses mkdir-p semantics, validates existing path components are directories, and releases its temporary native path buffer on every exit path; `fs.removeDirectories` recursively removes nested directory contents without following directory symlinks and releases per-entry traversal buffers on every path.
  - [ ] Owned path/string results, file reads, directory enumeration, richer filesystem metadata/errors, and ownership-safe byte I/O after owned strings/collections exist.
- [ ] Date/time/duration.
  - [x] Bootstrap native scalar time primitives: `time.unixMillis()` exposes wall-clock Unix milliseconds, `time.monotonicMillis()` provides an elapsed-time clock, and `time.sleepMillis(durationMs)` performs an EINTR-safe native sleep with compile-time rejection of constant negative durations plus a runtime guard for dynamic negatives. The helpers are tree-shaken when unreachable and participate in qualified LSP completion/signature help; calendar values, time zones, formatting, and richer duration types remain pending.
  - [x] Allocation-free UTC calendar decomposition from Unix milliseconds: `time.utcYear`, `utcMonth`, `utcDay`, `utcHour`, `utcMinute`, `utcSecond`, `utcMillisecond`, `utcWeekday`, and `utcDayOfYear` are strictly typed scalar accessors, normalize negative pre-epoch timestamps correctly, lower through the native UTC calendar conversion on current targets, participate in LSP completion/signature help, and tree-shake when unreachable. First-class calendar values, formatting, named time zones/local-time conversion, and richer duration types remain pending.
  - [x] Allocation-free UTC calendar construction with `time.utcUnixMillis(year, month, day, hour, minute, second, millisecond)`: component arguments are strictly typed, statically known invalid ranges/impossible month-day combinations are rejected during checking, dynamic invalid inputs trap instead of silently normalizing, native lowering uses checked integer calendar arithmetic including pre-epoch dates, LSP completion/signature help expose the full contract, and unreachable use tree-shakes completely.
- [ ] JSON parsing/encoding.
- [ ] URI/URL support.
- [ ] HTTP primitives.
- [ ] TLS integration.
- [ ] WebSocket support.
- [ ] Cryptographic primitives through audited platform/native libraries.
- [x] Bootstrap process/environment APIs for desktop/server targets: typed compiler-owned `process.pid()`, `process.parentPid()`, `process.hasEnv(name)`, and `process.env(name, fallback)` lower directly to host process/environment primitives, tree-shake when unreachable, reject reachable Android use until mobile semantics are defined, and participate in LSP qualified completion/signature help. Environment strings remain borrowed views under the current bootstrap `str` model; owned mutation/snapshots can build on the owned-string/collection track.
- [ ] Iteration protocol/interface.
- [ ] Efficient specialized collection implementations generated by compiler/stdlib tooling rather than source generics.

# Milestone 6 — Concurrency

- [ ] Task model.
- [ ] Structured concurrency.
- [ ] Cancellation propagation.
- [ ] Timers.
  - [x] Bootstrap blocking monotonic deadline timer with `time.sleepUntilMonotonic(deadlineMillis)`: the absolute `i64` deadline composes directly with `time.monotonicMillis()`, already-expired deadlines return immediately, native lowering rechecks the monotonic clock around EINTR-safe sleeps to avoid cumulative relative-sleep drift, LSP completion/signature help expose the contract, and unreachable timer support tree-shakes completely. Asynchronous task-integrated timers remain pending with the task/structured-concurrency model.
- [ ] Channels/message passing.
- [ ] Worker/thread execution.
- [ ] Data-race prevention integrated with ownership.
- [ ] Event-loop integration for UI targets.
- [ ] Server-oriented high-throughput I/O runtime.

# Milestone 7 — Typed Flux IR and optimizing native backend

Performance acceptance for this milestone is semantic as well as benchmarked: abstractions that can disappear must disappear, required safety checks must remain correct, and representative optimized Flux programs must be continuously compared against equivalent optimized Rust/C baselines so regressions are visible before release.

- [ ] Typed Flux IR with source spans.
  - [x] Bootstrap per-function semantic CFG nodes retain original source spans, function parameter/return types, and explicit typed local-binding metadata while expressions remain in the checked AST.
  - [x] Every normalized statement-level expression evaluation now carries its exact canonical static value shape in CFG/IR, including multi-value calls, loop/match guards, pattern-scoped expressions, redirects, and destructuring sources.
  - [x] All currently supported nested expression shapes lower into deterministic typed IR values with dependency IDs, including literals/names, unary/binary operations, calls/pipelines, lists/spreads/conditional items, indexing/slicing, struct construction/fields, conditionals, value-producing match/list-match guards and arms, and comprehension iterable/filter/body expressions. Explicit IR control regions for those conditional subexpressions and backend lowering directly from typed IR remain pending.
  - [x] Typed value dependencies now classify eager, short-circuit, branch, match, loop, and deferred-body uses and derive per-value reachability from reachable CFG evaluation roots. Source-foldable short-circuit and conditional-list branches prune unselected nested values immediately, while cross-statement propagated constants remain distinct so symbol elimination cannot outrun the bootstrap AST backend before direct typed-IR lowering lands.
- [ ] Ownership facts represented in IR.
  - [x] Bootstrap CFG action/evaluation nodes carry deterministic binding-read sets plus explicit source/destination/span facts for direct non-copy local moves.
  - [x] Direct-local move path-state and reachability are computed to a fixed point on CFG edges and exposed per node; bootstrap use-after-move checking consumes these IR facts.
  - [ ] Add borrow regions, partial moves, drops, lifetimes, and consuming-call ownership facts to normalized CFG/IR analysis.
- [ ] Explicit control-flow graph.
  - [x] The semantic database exposes deterministic per-function structural CFGs for sequential statements, `if`, loops/back-edges, `break`, `continue`, `return`, match-arm dispatch, and `else return` success/error exits.
  - [x] Bootstrap statement-level expression evaluation is explicit in CFG nodes for initializers, assignments, destructuring, return values, shell/redirection values, conditions, range bounds, iterables, match scrutinees, and match guards; `while`/`continue` edges re-enter condition evaluation correctly, while compile-time primitive conditions and statically empty integer ranges omit impossible branch/body edges.
  - [ ] Move expression evaluation/value production fully onto typed IR so later optimization no longer depends on checked-AST expression trees; all current expression shapes now have typed value/dependency nodes, while explicit control-flow regions for conditional expression subgraphs and backend consumption of those IR values remain pending.
- [ ] SSA or equivalent optimization-friendly value representation.
  - [x] Normalized evaluation sites produce deterministic typed value IDs for every single- or multi-value result, with producer node, result index, source span, canonical type, and common nested operand dependencies.
  - [x] CFG nodes now carry explicit typed local-definition facts for ordinary bindings/assignments, multi-value/list/struct destructuring, range/collection loop variables, and statement-match pattern bindings. Backward liveness consumes those normalized definitions rather than special-casing only mutable locals.
  - [x] Function parameters and normalized local definitions have deterministic definition identities, and every ordinary typed name-read records the exact may-reaching definition set computed over CFG edges. Branch and loop joins therefore expose explicit phi-equivalent input definitions without inventing runtime phi objects.
  - [x] Expression-local bindings also have typed scoped definition identities: value-producing enum/list-match payload patterns, comprehension variables, and anonymous-function parameters resolve their nested name reads to the exact scoped definition while preserving outer reaching-definition facts. Explicit conditional-expression control regions remain pending.
  - [x] Ordinary binding/assignment definition IDs link back to their producing typed IR values, so reaching-definition sets are usable as value-flow inputs rather than name-only facts.
- [x] Preserve debug locations through lowering: generated C emits source-aware `#line` directives for named/anonymous functions and every Flux statement, project builds retain canonical module paths, and Clang debug/profile DWARF line tables resolve native instructions back to the original `.flux` files and lines.
- [x] Constant folding for the current typed primitive/value-flow domain: compile-time expressions fold with checked Flux semantics and typed IR propagates primitive constants through local definition-use chains to prune impossible control flow before native lowering; aggregate/owned-value folding can extend the same mechanism as those value models land.
  - [x] Bootstrap primitive constant folding handles literals/constants, checked i64 arithmetic/comparisons, bool logic/short-circuiting, string equality, unary operations, and value-producing primitive conditionals before C lowering.
  - [x] Typed IR now propagates primitive constants to a fixed point through local definition-use chains, unary/binary operations, short-circuit boolean results, and conditional values. Constant local conditions prune impossible CFG edges and then recompute reaching definitions/liveness, enabling cross-statement dead-branch elimination without relying on C/LLVM to rediscover the fact.
- [ ] Dead-code elimination.
  - [x] Bootstrap native lowering removes semantic-CFG-unreachable statements after terminating control flow (`return`, `break`, `continue`), literal or locally propagated compile-time-selected `if`/`while` branches, and statically empty integer ranges before emitting C; general SSA value-liveness and broader cross-block optimization remain pending.
  - [x] The CFG now computes deterministic backward live-before/live-after facts for parameters and every normalized local definition. Native lowering removes dead mutable-local assignments, omits dead mutable initializers when safe, prunes dead multi-value/list/struct destructuring extractions and statement-match pattern materialization, substitutes internal loop counters for dead source-level loop indexes, and avoids dead foreach element loads while retaining required RHS effects and list-pattern cardinality checks. Full SSA-value DCE remains pending.
- [ ] Inlining.
  - [x] Bootstrap private named functions and capture-free anonymous functions lower with internal native linkage while `main` and explicit `pub` exports retain external linkage. This removes accidental interposition/ABI surface for compiler-owned internals and gives the current Clang/LTO backend full freedom to inline or internalize reachable private helpers without changing Flux source semantics; generated-C regression coverage pins the linkage boundary, and a real release-build regression executes a private leaf helper then verifies it disappears from the optimized native symbol table.
- [ ] Escape analysis.
- [x] Bounds-check elimination where proven safe for current list operations: compiler-generated iteration/transform/match/destructure accesses lower unchecked only behind compiler-established cardinality/range facts, and provably in-range literal `first`/`last`/`single` accesses skip redundant checks while dynamic/unproven indexes keep explicit checked semantics.
  - [x] Bootstrap compiler-generated list iteration, transform, reduction, sort/distinct/flatten/concat, spread-copy, comprehension, boolean aggregation, destructuring, and list-match extraction use unchecked strided element access only after compiler-emitted length guards establish the index range. Direct list-literal indexing plus `first`/`last`/`single` also use unchecked access when the compiler can prove the selected position exists; dynamic or otherwise unproven indexing and cardinality failures retain explicit checked semantics.
- [x] Devirtualization/interface specialization for the current closed-world interface model: concrete receivers dispatch directly, typed interface-pack facts shrink dynamic target sets, single-target interface values erase tags/switches entirely, and exported interface-valued boundaries stay conservatively open.
  - [x] Concrete-receiver interface calls lower directly to the mapped free function, and whole-program reachability follows that exact mapping rather than retaining every implementation. When no interface value is materialized, private interface runtime layout/dispatch metadata disappears entirely.
  - [x] Concrete interface packing is represented explicitly in typed IR with interface/target/value identities. Closed-world dynamic dispatch, interface layouts, tags, pack helpers, and mapped implementation reachability specialize to only the concrete targets reachable through those IR pack facts; public interface-valued function boundaries remain conservatively open.
  - [x] Single-target interface value sets devirtualize further: their native representation carries the concrete value directly with no tag/union, and dynamic capability helpers lower to a direct mapped call with no runtime switch. Multi-target sets retain checked tagged dispatch.
- [ ] Target-aware optimization pipeline benchmarked against equivalent optimized Rust/C implementations for representative compute, collection, dispatch, and native-UI workloads; performance regressions become release blockers rather than accepted framework overhead.
- [ ] Native object generation through an LLVM-class backend initially.
- [ ] Prove and eliminate redundant safety checks (including bounds checks) and abstraction overhead through CFG/SSA analysis while preserving checks whose safety preconditions cannot be established statically.
- [x] LTO in native release builds through Clang `-flto`, covered by build-mode tests.
- [x] Continuous bootstrap performance harness with optimized overflow-safe Flux-vs-Rust/C compute, collection-pipeline, interface-dispatch, and dynamic function-call/control-flow baselines, output-equivalence checks, median runtime/code-size reporting, a configurable checked-Rust release threshold, C reference tracking, and a push/PR CI gate. Native-UI and broader workload coverage remains part of the target-aware optimization item above.
- [x] Tree shaking/dead symbol elimination for all current bootstrap backend concepts: whole-program reachability removes unreachable functions, anonymous functions, value types/helpers, interface metadata/dispatch targets, enum/struct helpers, function-pointer typedefs, platform/runtime helpers, and dead view metadata while preserving public/dynamic roots; future backend resource kinds must join the same reachability graph when introduced.
  - [x] Bootstrap whole-program function reachability keeps `main`, public exports, interface implementation mappings, application/view callback references, and their transitive private callees while omitting provably unreachable private function prototypes/bodies and anonymous functions from generated C.
  - [x] Reachability now prunes unused private struct/enum definitions, individual unused enum-constructor and struct-update helpers, function-pointer typedefs, unused interface pack helpers and per-capability dynamic-dispatch/multi-return helpers, implementation mappings for interface capabilities that are never referenced, unreachable interface value-layout targets, metadata/callback/type roots from views the current native backend does not emit, background-process headers, and unused bootstrap runtime helper bodies while retaining public value types/contracts, every implementation needed by a genuinely dynamic capability, reachable function-local aggregate uses, recursive by-value dependencies, and all enum tags needed by reachable matching. Primitive `const` declarations are compile-time-only and therefore create no runtime storage/symbol roots.
  - [x] Closed-world dynamic interface target sets now come from typed IR interface-pack facts, so an implementation can be removed from dynamic dispatch even when its concrete type remains reachable for unrelated code; exported interface-valued entry points deliberately keep the target set open.
  - [x] Whole-program reachability consumes only CFG- and value-reachable typed IR: compile-time-dead statements and source-foldable unselected nested values no longer retain private callees, first-class named/anonymous functions, function-pointer typedefs, value types, interface packs, interface implementations, dispatch targets, enum-constructor helpers, or background-process runtime support merely because their checked AST/IR was constructed before reachability pruning.
  - [ ] Extend reachability pruning/specialization to future backend symbols/resources as those backend concepts are introduced.
- [ ] Remove bootstrap C as the semantic backend after parity is proven.

# Milestone 8 — Application model

- [x] `app` declaration / application entry model.
  - [x] Bootstrap `app ViewName` process entry selects one visible zero-parameter root view, is formatter/LSP-keyword aware, rejects competing `fn main`, and lowers through the current native Linux/Android application backends. Rich process arguments and multiple-window entry remain pending in their dedicated follow-up tracks rather than blocking the core application entry model.
- [x] Bootstrap application metadata: `app ViewName(id: ..., title: ..., width: ..., height: ..., resizable: ...)` accepts compile-time typed values/constants, validates positive dimensions and GLib-compatible reverse-DNS-style application IDs, formats canonically, and lowers identity/title/default size/resizability into the native Linux window. Icons/permissions remain in their dedicated later tracks rather than being hidden in this bootstrap surface.
- [ ] Assets/resources.
- [ ] Environment/configuration.
- [x] Platform permissions for the current native application targets: Android packages accept validated declarative `[android].permissions`, generate deterministic manifest entries, expose typed `android.permissionGranted` / `android.requestPermission` runtime capabilities, and infer compiler-owned vibration/notification permissions only when reachable code needs them; Linux has no equivalent application-manifest permission surface to configure. Permission JNI/runtime support tree-shakes when unused and remains hidden behind Flux APIs rather than application bridge code.
- [ ] Lifecycle events as functions/capabilities, not class callbacks.
  - [x] Bootstrap `app` metadata accepts named typed `onStart`, `onResume`, `onPause`, `onStop`, and `onExit` `fn() -> void` callbacks. Linux maps start/stop/exit to native application activation/shutdown and resume/pause to native window active-state changes; Android maps the same callbacks directly onto `ANativeActivityCallbacks`, with no application/controller object exposed to Flux source.
  - [ ] Suspension/restoration, saved-state/configuration lifecycle, multiple-window lifecycle, and capability-rich lifecycle context.
- [x] Background/foreground transitions for current application targets: typed `onResume` / `onPause` lifecycle callbacks map to GTK window active-state changes on Linux and native Activity resume/pause callbacks on Android, preserving the function/capability model without controller objects.
- [ ] Multiple windows where the target supports them.
- [ ] Platform capability discovery.
  - [x] Android bootstrap exposes `android.sdkInt() -> i64` as a direct NDK-backed runtime capability query, allowing ordinary typed Flux control flow to gate target-specific behavior by the device API level without JNI/reflection or a platform object. Broader feature/capability discovery remains pending.

# Milestone 9 — UI language and flat layout

The UI model must avoid Flutter-style deeply nested widget construction and object hierarchies. Sequencing note: the compiler-backed surface syntax is being prototyped early, before rendering backends, because the flat layout grammar is a core language constraint and should not be retrofitted after a widget-tree API takes shape.

- [x] `view` declarations/functions for the flat declarative UI model.
  - [x] Bootstrap top-level `view Name { ... }` declarations with visibility, source spans, formatter support, project merging, and semantic symbols.
  - [x] Parameterized `view Name(data: Type, optional: Type = default) { ... }` declarations: parameters are statically typed, defaults use the ordinary constant-default rules, view bodies can reference parameters, and parameter symbols retain editor spans/types.
  - [x] Additional view-producing function composition is intentionally omitted from the current language: parameterized views already compose as typed flat elements, while arbitrary view-returning functions would add builder-style construction without a demonstrated capability gap. Reopen only if a concrete use case cannot be expressed through typed data/view composition.
- [x] Views compose through typed data rather than subclassing: a declared Flux view can be used as another view's flat sibling element type, arguments are named properties checked against the target view's parameters/defaults, module visibility is enforced, and recursive composition cycles are rejected.
- [x] Flat element declarations.
  - [x] Bootstrap sibling element syntax (`Type name at row,column`) with unique local names and optional property expressions; nested child elements are syntactically rejected.
- [x] Grid-first layout.
  - [x] Bootstrap grid grammar with explicit columns/rows, logical fixed units, `fr` tracks, `auto`, gap, explicit padding, explicit row/column placement, and row/column spans lowers to the current native Linux/Android layouts. Common minimum sizing, margins/alignment, stable placement checks, and responsive property expressions are implemented; responsive track definitions, max/intrinsic sizing, and optional named areas remain explicit follow-up work.
  - [ ] Statically type/check element properties against built-in or interface-defined element contracts instead of treating property expressions as untyped view metadata.
    - [x] Bootstrap built-in element contracts validate known property names and property expression types, including typed function callbacks, without introducing widget classes; semantic symbols expose the property type for editor tooling.
    - [x] Declared Flux views act as custom element contracts derived directly from their typed parameters/defaults; custom-view property types flow into semantic/editor symbols.
    - [ ] Interface-defined platform/native element contracts after platform capability rules are mature.
  - [x] Validate placement against declared track counts and define overlap policy: row/column spans that escape the declared grid are static errors, and ordinary sibling grid regions may not overlap unless the grid explicitly opts into overlay composition.
  - [x] Named grid areas are intentionally omitted from the current grid language: stable element names plus explicit row/column/span placement keep the layout locally readable without introducing a second naming/indirection layer. Revisit only if real application layouts demonstrate a material readability win.
- [ ] Flex/flow layout where grid is not appropriate.
- [x] Explicit overlay positioning when requested: flat grids accept `grid overlay: true`, allowing sibling row/column regions to overlap deliberately while preserving ordinary bounds validation, source-order native composition, and the no-wrapper flat layout model on Linux and Android. Omitted/false overlay mode continues to reject accidental overlap statically; absolute coordinate positioning remains intentionally unnecessary for this contract rather than being smuggled into ordinary layout.
- [ ] Constraints, min/max sizing, intrinsic sizing.
  - [x] Bootstrap common `minWidth: i64` / `minHeight: i64` element constraints lower to native size requests (combined with fixed grid tracks) and reject non-positive/non-compile-time values. Max/intrinsic/content measurement semantics remain pending.
  - [x] Common `visible: bool` can derive from view state on native Text/Button/Toggle controls and refreshes the existing control without adding wrapper elements to the flat layout.
- [ ] Responsive breakpoints/container queries.
  - [x] Compiler-owned portable window breakpoints are exposed as read-only `windowIsCompact`, `windowIsMedium`, and `windowIsExpanded` boolean view-environment bindings derived from logical width (`<600`, `600..<840`, `>=840`). They type-check like the existing window/orientation environment, participate in LSP completion, update through the existing native environment-refresh path on Linux and Android, and add no runtime layout object or framework dependency. Per-container queries remain pending.
- [x] Text.
  - [x] Bootstrap Linux GTK4 `Text` lowering for literal/constant/state-derived `text: str`, `selectable: bool`, compile-time positive `size: i64`, `bold: bool`, font family/slant/decoration, letter spacing, line height, and hexadecimal foreground color, including grid placement and Pango-backed typography without exposing GTK/Pango objects to Flux source.
- [ ] Rich text.
- [ ] Images/vector images.
  - [x] Bootstrap native `Image` control uses a typed compile-time/state-derived `source: str`, `alt: str`, `canShrink: bool`, and compile-time `fit: str` (`fill`/`contain`/`cover`/`scaleDown`); Linux lowers to `GtkPicture`, refreshes state-derived source/alt values, and reuses common visibility/accessibility/layout/event properties. The current source is a runtime file path; package asset embedding/resource identity and vector-specific behavior remain pending rather than being hidden behind cwd assumptions.
- [x] Buttons.
  - [x] Bootstrap Linux GTK4 `Button` lowering for compile-time `text: str` / `enabled: bool` plus typed named `fn() -> void` `onPress` dispatch.
- [ ] Text inputs.
  - [x] Bootstrap native `TextInput` control with typed `text: str`, `placeholder: str`, `enabled: bool`, `autofocus: bool`, `password: bool`, `maxLength: i64`, compile-time `keyboardType: str`, `onChange: fn(str) -> void`, and `onSubmit: fn(str) -> void`; Linux lowers single-line input to `GtkEntry`, passes the current borrowed entry text synchronously to named Flux callbacks, supports native password masking/validated maximum length plus text/email/number/decimal/phone/URL input purposes, and treats `text` as an initial native value so refresh does not clobber user edits while Flux-owned editable string state remains sequenced behind the ownership/string model.
  - [x] Linux GTK multiline bootstrap parity: `multiline: true` lowers to native `GtkTextView`/`GtkTextBuffer`, preserves word/character wrapping, delivers full-buffer `onChange`, defaults Enter to text insertion, and supports explicit `submitOnEnter: true` through a native key controller without application bridge code. Password masking, placeholder text, and maximum-length enforcement remain explicit unsupported combinations rather than silently degrading semantics.
  - [ ] Editable Flux-owned text state, richer data/message validation, portable selection/caret APIs, and fully portable IME semantics. Semantic validation presentation is now first-class through `validationState`; Android already has native multiline submit policy plus direct focused selection/caret APIs.
- [x] Toggles/check/radio controls.
  - [x] Bootstrap native `Toggle` control with typed `label: str`, `checked: bool`, `enabled: bool`, and `onChange` event; Linux lowers to `GtkCheckButton`, while `onChange: state => nextExpression` reuses the same functional transition/state refresh model as buttons.
  - [x] Bootstrap native `Radio` controls use typed `label: str`, `selected: bool`, `enabled: bool`, and `onSelect`; sibling radios are grouped natively and `onSelect` fires only for the newly active control, while shared Flux state remains the source of truth for selection.
- [ ] Lists/grids with virtualization.
- [x] Scrolling.
  - [x] Flat root grids may declare `grid scroll: true`; Linux lowers the existing grid into a native `GtkScrolledWindow` with automatic scrollbars while Flux source remains a flat sibling element set with no scroll-wrapper widget hierarchy.
- [ ] Overlays/popovers/tooltips.
  - [x] Bootstrap common `tooltip: str` property is statically typed/LSP-visible on built-in elements and lowers directly to native GTK tooltip text on the existing control, without adding a source-level wrapper/overlay object. Rich/custom tooltip content, popovers, and explicit overlay positioning remain pending.
- [ ] Dialogs/sheets.
- [ ] Menus/context menus.
- [x] Native platform control mapping where beneficial without exposing platform classes to Flux code.
- [x] **Beautiful-by-default visual system** that makes minimally styled Flux apps look production-quality out of the box rather than exposing raw platform-widget defaults.
  - [x] Platform-aware semantic design tokens for typography, spacing, color, elevation, radii, motion, and control sizing, with coherent defaults rather than requiring users to choose pixel values everywhere. Color tokens (`surface`, `surfaceRaised`, `text`, `textMuted`, `accent`, `onAccent`, `outline`, `danger`, `success`, `warning`, `shadow`) resolve through Android light/dark palettes or GTK theme colors; semantic typography roles supply native-scaled type defaults; read-only numeric tokens provide a shared 4/8/12/16/24/32 spacing rhythm, 6/10/16/pill radii, low/medium/high elevation values, and 120/200/320ms motion durations. Numeric tokens participate in compile-time Flux expressions, lower to constants with no theme-object runtime, are offered through LSP completion, and use the existing native logical-unit/density conversion. Platform-appropriate default control sizing remains compiler-owned (48dp Android / 40px Linux), while explicit values and hexadecimal colors stay available as escape hatches.
    - [x] Bootstrap semantic color authoring support: the shared compiler token table drives validation/native lowering and contextual LSP value completion for every common UI color property, so typing `color:`/`backgroundColor:`/border/shadow color fields suggests the valid semantic token literals instead of requiring memorization.
    - [x] Bootstrap semantic typography roles: `Text.variant` accepts `body`, `caption`, `heading`, `title`, and `display`; omitted variants use a polished body role, both native backends apply scalable role size/weight/line-height defaults, and explicit `size`, `bold`, or `lineHeightPercent` values override the role without requiring a theme framework/runtime.
  - [x] Native controls share one intentional visual language across a screen while preserving platform behavior, accessibility, focus, text scaling, and input conventions: compiler-owned roots, text, buttons, text inputs, toggles, and radios receive coherent semantic surface/text/accent defaults on Android and lightweight lower-priority GTK classes on Linux; native interaction/input semantics remain intact and application-priority element styling stays authoritative.
    - [x] Explicit Flux styling wins over compiler-owned/native role styling: Android clears any platform Button tint before installing an explicit Flux background/border/radius/shadow drawable, preventing `Button.primary` emphasis from recoloring an application-specified surface; shape-only Button styling now carries the semantic primary/ordinary surface into that drawable instead of accidentally making the control transparent. Linux application-priority element CSS already preserves the same explicit-style precedence.
  - [x] Layout defaults provide readable line lengths, comfortable touch targets, consistent rhythm, and sensible density on phone/tablet/desktop sizes without requiring design expertise.
    - [x] Coherent default layout now includes 20-logical-unit root padding and a 12-unit grid rhythm on both native backends, Android auto-sized Button/TextInput/Toggle/Radio controls receive a 48dp minimum hit height, Linux equivalents receive a desktop-appropriate 40px minimum, and semantic Text roles cap natural line length by default (72 characters for body/caption, progressively tighter for heading/title/display). `Text.maxWidthChars` accepts a compile-time override and `0` disables the cap; explicit `grid gap`, `grid padding`, fixed row sizing, and `minHeight` remain authoritative escape hatches.
  - [x] Automatic high-quality light/dark themes and accessible contrast, with platform/system preferences respected by default: Android semantic surface/text/accent palettes switch from the active `uiMode` (or explicit app theme) and cover root/control pressed/disabled/focused/checked states, while Linux semantic tokens resolve through active GTK theme colors; verified key Android text/background pairs meet at least 4.5:1 contrast without application-authored theme code.
  - [x] First-class polished loading, empty, error, disabled, focused, pressed, selected, and validation states so unfinished states do not look unfinished: every built-in element accepts a compile-time semantic `status` of `normal`, `loading`, `empty`, or `error` with contextual LSP completion; Linux lowers presentation plus hover/pressed/focus/disabled/checked states through low-priority GTK theme CSS, while Android applies semantic presentation styling after its existing native pressed/disabled/focused/checked state lists. Explicit element styling remains authoritative, and validation keeps its dedicated input semantics below.
    - [x] Bootstrap `TextInput.validationState` provides compile-time `normal` / `error` / `success` / `warning` semantics with contextual LSP completion. Linux lowers non-normal states to low-priority GTK theme classes backed by semantic danger/success/warning colors; Android feeds the same semantic palette into native `EditText` state tints while retaining disabled/focused behavior. Omission remains polished `normal`, and explicit application element styling stays authoritative.
  - [x] Custom themes and explicit styling remain straightforward escape hatches without adding a theme-object framework: application metadata can override any semantic palette entry with concrete `#RRGGBB` / `#RRGGBBAA` values (`surfaceColor`, `surfaceRaisedColor`, `textColor`, `textMutedColor`, `accentColor`, `onAccentColor`, `outlineColor`, `dangerColor`, `successColor`, `warningColor`, `shadowColor`). Existing element-level color/border/radius/padding/etc. styling remains higher priority, while Linux emits the custom palette directly into GTK CSS definitions and Android resolves the same generated palette through its compiler-owned native bridge.
  - [x] README/gallery screenshots are reproducible real application captures rather than synthetic mockups: `tools/capture-readme-showcase` builds the checked-in `examples/mobile_showcase` as a release x86_64 APK, installs/launches it in ADB-connected Waydroid, forces a known 1080x1920 portrait viewport at 360 dpi for the capture, validates the resulting PNG, and restores the emulator display settings afterward. The checked-in README image was regenerated through that exact workflow after live visual verification.

# Milestone 10 — Functional UI state and rendering

- [ ] Local view state without `StatefulWidget`-style objects.
  - [x] Bootstrap explicit primitive view state (`state name: type = compileTimeValue`) is compiler/formatter/semantic metadata and the Linux root-view runtime stores `bool` and `i64` state per app instance without source-level state objects/controllers; owned/string/aggregate state remains sequenced behind the ownership model.
- [x] Immutable state updates by default for the current functional UI-state model.
  - [x] Bootstrap UI events use a functional transition form (`onPress: state => nextExpression`): the next state expression is strictly type-checked against the target state and evaluated before native property refresh; bool/i64 transitions support primitive unary/binary arithmetic, comparisons, logic, checked integer arithmetic/division, and conditionals.
- [x] Derived/computed state for the current primitive UI-state model.
  - [x] Views may declare typed read-only `derived name: type = expression` bindings. Bootstrap `i64`/`bool`/`str` derived values may depend on parameters, mutable view state, read-only window environment, compile-time constants, and earlier derived bindings; declaration order rejects forward/cyclic dependencies, derived values cannot be transition targets, semantic/LSP tooling indexes them, and the Linux backend recomputes them before dependent native properties refresh.
- [ ] Explicit effects.
- [ ] Shared state/store primitives only where necessary.
- [ ] State ownership/lifetime rules integrated with the compiler.
- [x] Fine-grained dependency tracking for the current primitive UI model: codegen derives transitive state → derived-value → refreshable-property dependencies, state-transition callbacks carry the changed state identity, Linux and Android skip unrelated native element refresh work, and window-environment changes invalidate only environment-dependent properties while full refresh remains available for initialization and callback/lifecycle synchronization.
- [x] Minimal re-render/re-layout work after state changes on the current native application backends.
  - [x] Bootstrap GTK4 root-view refresh updates only state-dependent native Text/Button properties and does not reconstruct the window/grid; the dogfood toggle produced a localized render delta and the second toggle restored the app region to its original pixels.
  - [x] Bootstrap Android ordinary state transitions now preserve the existing native view hierarchy: compiler-owned stable element IDs let a generated refresh function recompute derived values and update existing text/labels, visibility/enabled state, toggle/radio selection, input hints, image content, accessibility text, tooltips, and transforms in place. Toggle/radio refresh suppresses listener re-entry; configuration/layout changes may still rebuild the hierarchy.
- [x] Stable element identity rules: flat element identity is derived deterministically from the declaring view plus the source element name rather than sibling position. Android lowers that identity to a non-zero 24-bit native View ID, rejects the vanishingly rare compile-time hash collision with a rename diagnostic, and preserves callback/refresh/focus identity across sibling insertion or reordering; Linux already addresses generated controls by their stable source element names.
- [x] No mandatory controller classes.
- [x] No build-context object threaded through user code.

# Milestone 11 — Automatic save-triggered hot reload

Flux hot reload should feel closer to React development than Flutter's manual reload workflow.

- [x] `flux run` watches the project automatically in development mode.
  - [x] Bootstrap `flux run`/`fluxc run` builds and launches a native development binary, then continuously watches the real project/import graph plus `flux.toml` where applicable; dependency-save rebuild/restart and broken-edit recovery are covered by an end-to-end CLI test.
- [ ] Saving a source file automatically triggers incremental recompilation.
  - [x] Bootstrap runner automatically recompiles on save through the reusable project-analysis cache: unchanged whole graphs are reused, and changed graphs reuse parsed ASTs for unaffected modules. Whole-program type checking/codegen still rerun after a semantic change, so true incremental recompilation remains pending.
- [x] No hot-reload hotkey is required for the normal workflow: the bootstrap runner reacts to saves automatically.
- [ ] Apply compatible function/view changes to the running process automatically.
- [ ] Preserve compatible application/UI state across reloads.
- [ ] Fast Refresh-style boundary detection: reset only state that is no longer structurally compatible.
- [ ] Automatically fall back to a controlled restart when a change cannot be safely hot-applied.
  - [x] Bootstrap controlled-restart path builds a replacement binary before stopping the current child; failed recompiles keep the last good process untouched and continue watching for repairs.
- [x] Surface reload/restart reason in tooling without interrupting normal development: source detection, compile failure, child exit, and successful rebuild/restart are reported concisely, and `fluxc run` publishes a stable last-known JSON status keyed by the resolved entry for editor/tooling consumers.
- [x] Debounce/coalesce rapid editor saves before recompiling.
- [x] Watch dependency/module changes using the compiler's loaded import graph, refreshing the watch set after successful recompilation and retaining known paths across failed edits.
- [ ] Stable development ABI/function indirection strategy.
- [ ] Incremental compiler cache.
  - [x] `fluxc run` owns a persistent `ProjectAnalysisCache`, avoiding the former separate check + compile analysis on successful rebuilds and reusing unchanged module ASTs across changed graphs.
  - [ ] Incremental semantic/type/codegen reuse for changed graphs, plus durable on-disk cache artifacts where they materially improve cold-start development builds.
- [ ] Target: save-to-visible-update latency competitive with modern React Fast Refresh for ordinary UI edits.

# Milestone 12 — Input and interaction

- [x] Pointer/mouse input for the bootstrap native UI: common tap plus hover/leave events cover ordinary pointer activation and presence without exposing event/controller objects; richer coordinates/buttons/drag semantics remain in the dedicated gesture/drag follow-up work.
  - [x] Common typed `onTap: fn() -> void` works on every built-in native element without exposing pointer/controller objects; Linux lowers it to a native `GtkGestureClick` release callback and supports the ordinary functional state-transition form.
- [x] Touch input for the bootstrap native UI: portable tap and native long-press recognition are wired on current application backends; multi-touch/drag/scale/velocity semantics remain in the dedicated gesture work.
  - [x] The same `onTap` contract lowers on Android to a compiler-wired native `View.OnTouchListener` and dispatches `ACTION_UP` directly back into compiled Flux callbacks/state transitions, with no application-authored Java/Kotlin/JNI bridge.
  - [x] Common `onLongPress: fn() -> void` lowers to Android's native `View.OnLongClickListener`, consumes the recognized long press after direct Flux dispatch, and supports typed functional state transitions without exposing `MotionEvent` or listener objects.
- [x] Keyboard input.
  - [x] Bootstrap native text typing reaches `TextInput` through GTK's keyboard handling; `onChange` receives the current borrowed text and Enter dispatches `onSubmit`. Common `onKey: fn(str) -> void` now provides portable explicit key-down handling on Linux and Android without exposing toolkit event objects: focused elements receive normalized names such as `Enter`, `Escape`, `Backspace`, `Delete`, `ArrowLeft`/`ArrowRight`/`ArrowUp`/`ArrowDown`, `Home`, `End`, and `PageUp`/`PageDown`, while printable Unicode keys are passed as their text value. Linux uses a capture-phase GTK key controller and Android uses the native `View.OnKeyListener`; declaring `onKey` also makes otherwise non-focusable native elements keyboard-focusable.
- [ ] Focus model.
  - [x] Bootstrap initial focus and focus events remain native and object-free: common compile-time `autofocus: bool` works on every built-in element on Linux and Android, automatically makes an otherwise passive target focusable, and rejects multiple enabled autofocus targets or an explicit `focusable: false` contradiction within one view. `TextInput` retains the same source spelling, common `onFocus` / `onBlur` events accept named `fn() -> void` callbacks or typed functional state transitions, and native traversal remains available. Focus scopes and restoration remain pending.
  - [x] Portable programmatic focus navigation remains compiler-owned and object-free: `focus.next(wrap: bool = false)`, `focus.previous(wrap: bool = false)`, `focus.first()`, `focus.last()`, and `focus.clear()` lower to native GTK focus traversal on Linux and reuse direct native View focus traversal on Android, with LSP completion/signature help and no focus/controller object exposed to Flux source.
- [x] Hover.
  - [x] Common `onHover` / `onLeave` events on native Text/Button/TextInput/Toggle/Radio controls accept named `fn() -> void` callbacks or typed functional state transitions; Linux lowers them through native motion controllers while keeping event/controller objects out of Flux source.
- [ ] Gestures.
  - [x] Portable single-tap recognition is available through `onTap`.
  - [x] Portable double-tap recognition through typed `onDoubleTap: fn() -> void` callbacks or functional state transitions: Linux uses native multi-press `GtkGestureClick` recognition, while Android recognizes two native click activations within the platform double-tap timeout and dispatches directly into compiled Flux without exposing event objects or application bridge code. Single-tap/click behavior remains intact, including keyboard/accessibility activation through the native click path.
  - [x] Portable long-press recognition is available through `onLongPress`; Linux uses native `GtkGestureLongPress` and Android uses the platform long-click recognizer. Drag, scale, and velocity semantics remain pending.
- [ ] Drag and drop.
- [ ] Keyboard shortcuts.
  - [x] Bootstrap `Button.shortcut: str` accepts validated compile-time forms such as `Ctrl+K`, `Ctrl+Shift+K`, `Alt+Enter`, and `Shift+Space`; Linux lowers them to native GTK shortcut triggers/actions and dispatches through the button's existing typed `onPress` logic.
  - [ ] Broader shortcut scopes, non-button actions, conflict diagnostics across composed views/windows, and platform-specific shortcut presentation.
- [ ] Context menus.
- [ ] Clipboard.
- [ ] IME/text-composition support.

# Milestone 13 — Navigation

- [ ] Typed routes.
- [ ] Route parameters.
- [ ] Back stack.
- [ ] Tabs.
- [ ] Nested navigation where genuinely needed.
- [ ] Deep links.
- [ ] Dialog/sheet navigation semantics.
- [ ] Browser history integration.
- [ ] Android predictive back.
- [ ] State restoration.

# Milestone 14 — Styling, themes, responsive UI, and animation

- [ ] Colors.
  - [x] Bootstrap `Text.color: str` accepts compile-time `#RRGGBB` / `#RRGGBBAA` values, validates hexadecimal syntax, and lowers directly to native Pango foreground/alpha attributes.
  - [x] Every built-in native element accepts compile-time `backgroundColor` / `borderColor` hexadecimal values, lowered through an element-specific native GTK style provider without wrapper controls. A first-class `Color` value type plus themes/tokens remain pending.
- [x] Typography.
  - [x] Bootstrap native `Text.size: i64` and `Text.bold: bool` contracts lower to Pango attributes on Linux; invalid/non-positive sizes fail at compile/codegen time instead of silently rendering.
  - [x] Native text family/slant/decoration/spacing controls: compile-time `fontFamily`, `italic`, `underline`, `strikethrough`, `letterSpacing`, and `lineHeightPercent` are compiler-owned/LSP-visible `Text` properties and lower directly to Pango attributes, including range/empty-family diagnostics.
  - [x] Native text layout/overflow controls: `textAlign` (`left`/`center`/`right`/`fill`), state-aware `wrap`, compile-time `wrapMode` (`word`/`char`/`wordChar`), `ellipsize` (`none`/`start`/`middle`/`end`), and positive `maxLines` lower directly to `GtkLabel`/Pango layout controls. Variable font axes, richer weight/style enums, and responsive typography remain future typography work.
- [x] Spacing.
  - [x] Flat-grid `grid padding: N` is source-level layout data and lowers to native root-grid margins; `grid gap` already lowers to native row/column spacing.
  - [x] Every built-in element has compile-time non-negative `margin` plus per-edge `marginTop`/`marginBottom`/`marginStart`/`marginEnd`, with per-edge values overriding the common margin and lowering directly onto the existing native control. Common `alignX` / `alignY` (`start`/`center`/`end`/`fill`) provide explicit cell alignment without layout-wrapper elements.
  - [x] Every built-in element also has compile-time non-negative inner `padding` with `paddingTop`/`paddingBottom`/`paddingStart`/`paddingEnd` overrides, lowered into the existing native element stylesheet without introducing wrapper controls.
- [x] Borders/radii.
  - [x] Bootstrap common `borderWidth: i64` and `radius: i64` are non-negative compile-time dimensions and lower to native border width/style and border radius on the existing element; `borderColor` shares the validated hexadecimal color contract.
  - [x] Per-edge border widths (`borderTopWidth` / `borderBottomWidth` / `borderStartWidth` / `borderEndWidth`), per-corner radii, and explicit `borderStyle` (`none`/`solid`/`dashed`/`dotted`/`double`) lower through GTK's standard CSS border surface. A positive width still defaults to `solid` when no style is supplied.
- [x] Shadows.
  - [x] Bootstrap common native shadows use compile-time `shadowColor`, `shadowBlur`, `shadowOffsetX`, and `shadowOffsetY` properties on existing flat elements; Linux lowers them directly to GTK CSS `box-shadow` without wrapper controls, with validated hexadecimal or semantic theme color and non-negative blur. The implicit shadow fallback now uses the theme-aware `shadow` token rather than a hard-coded color; spread/inset/elevation semantics remain future styling work.
- [x] Clipping.
  - [x] Bootstrap common `clip: bool` is state-aware and lowers to the native GTK overflow mode on the existing element; changing the driving Flux state refreshes clipping in place without wrapper widgets. Shape/path clipping remains future rendering work.
- [x] Transforms.
  - [x] Common native transforms on every built-in element: typed `translateX`, `translateY`, `rotateDegrees`, uniform `scalePercent`, axis overrides (`scaleXPercent` / `scaleYPercent`), `skewXDegrees` / `skewYDegrees`, and percentage transform origins lower directly to GTK4's supported CSS transform surface on the existing control. Compile-time values fold into the static element stylesheet; state-derived primitive expressions reuse a persistent per-element CSS provider refreshed by the existing functional UI invalidation path, so button/hover/focus transitions can move/rotate/scale/skew controls without wrapper widgets. Animated transitions remain tracked separately under the animation/property-transition roadmap rather than making base transform semantics depend on an animation system.
- [x] Light/dark themes.
  - [x] Bootstrap application-level `theme: "system" | "light" | "dark"` metadata is compile-time validated and lowers to the native GTK theme preference when explicitly light/dark; `system` leaves the platform preference untouched. Semantic UI color tokens now follow that active theme; runtime theme switching and per-view overrides remain future work.
- [ ] Design tokens/variables. Semantic color tokens and typography roles now provide the first compiler-owned token surfaces; general user-defined variables/tokens plus spacing/radius/elevation/motion token APIs remain pending.
- [x] System theme integration for the current native backends: Linux `theme: "system"` preserves the GTK/system preference, while Android resolves the current `Configuration.UI_MODE_NIGHT_*` mode before Activity creation and reapplies it on `uiMode` configuration changes; explicit `light`/`dark` select the matching native `DeviceDefault` theme without bundling a framework theme runtime.
- [x] Device/window/orientation queries for the current native application backends; richer monitor/work-area/device-class queries remain follow-up extensions rather than blocking the core environment surface.
  - [x] Bootstrap root-view environment exposes read-only typed `windowWidth: i64`, `windowHeight: i64`, `windowIsLandscape: bool`, `windowIsPortrait: bool`, and `displayScale: i64` bindings directly inside ordinary view property expressions. Linux seeds them from the application/default window geometry and native widget scale, watches `GtkWindow` default-width/default-height plus widget scale-factor changes, and reuses the existing fine-grained refresh path on ordinary resizes without exposing a window object to Flux source. Android converts `DisplayMetrics` pixels through the exact floating-point density before exposing logical width/height, keeps the integer `displayScale` compatibility binding separate, and uses the exact density for native dp-style layout/styling so non-integer-density devices do not inflate spacing, radii, fixed tracks, or translations. Exact maximized/fullscreen content geometry, monitor/work-area/device-class queries, and richer device-class semantics remain pending.
- [ ] Adaptive components where platform differences are valuable.
  - [x] Bootstrap responsive property adaptation can use the read-only window/orientation/scale environment in ordinary conditional expressions, so native element text/visibility/enabled/accessibility and other already state-aware properties can react to window changes without a controller or widget rebuild. Responsive grid-track definitions, component substitution, and compile-time-only style properties remain pending.
  - [x] Bootstrap `Button.primary: bool` maps an explicit compile-time primary action to native platform emphasis without exposing platform widget APIs in Flux source: Linux uses GTK's `suggested-action` style class, while Android resolves the active native theme accent, applies it through the platform button tint surface, derives contrasting label text, keeps an explicit lower-emphasis disabled color state, and preserves ordinary Android pressed/focus/accessibility behavior.
- [ ] Animation timelines.
- [ ] Easing/springs.
  - [x] Bootstrap property transitions accept compile-time `transitionEasing` values `linear`, `ease`, `easeIn`, `easeOut`, and `easeInOut`, mapped to native GTK CSS timing functions. Spring curves remain pending.
- [x] Property transitions.
  - [x] Every built-in element accepts non-negative compile-time `transitionMs` plus optional `transitionDelayMs` / `transitionEasing`. Linux lowers these to standard GTK CSS transitions on `all` animatable properties; this already animates state-driven transform changes without an animation object model. Reduced-motion policy and selective transition-property syntax remain separate follow-up work.
- [ ] Layout transitions.
- [ ] Gesture-driven animation.
- [ ] Frame synchronization.
- [x] Reduced-motion accessibility handling.
  - [x] Native property-transition styles include a `prefers-reduced-motion: reduce` override that removes transition duration/delay, and each transition CSS provider live-binds GTK's system `gtk-interface-reduced-motion` setting into its `prefers-reduced-motion` property. The application therefore follows the platform accessibility preference without a Flux-specific toggle.

# Milestone 15 — Android target

Android is the first application-platform target. Android integration must feel like programming Android in Flux, not embedding a second Kotlin/Java application beside it: Flux code can directly use Android-specific modules for capabilities such as notifications, WorkManager/background work, vibration, intents, services, media, and sensors. The backend should prefer direct NDK/native APIs when Android provides them; for JVM/AndroidX-only framework APIs it generates the necessary Kotlin/Java bytecode/classes and JNI/native entry thunks automatically. No method-channel equivalent, hand-written JNI shim, serialized plugin call protocol, or user-maintained bridge package is required in ordinary Flux application code.

- [x] Android target triple/backend.
  - [x] Bootstrap target-aware codegen lowers a Flux `app` to an NDK `NativeActivity` shared library for `arm64-v8a`, `x86_64`, or `armeabi-v7a`; ABI + minSdk select the matching NDK Clang target and Android builds do not pull in the Linux/GTK backend. Application source stays Java/Kotlin/JNI-free; when Android framework APIs require JVM-side types, Flux may generate compiler-owned runtime classes and direct JNI thunks. Real arm64 debug APK, x86_64 release APK, and armeabi-v7a profile APK artifacts have been produced on Nox, and the native library exports `ANativeActivity_onCreate` as the framework lifecycle entry point.
- [x] `flux build android`.
  - [x] Manifest-backed packages build directly through the installed SDK/NDK with `--abi`, `--mode`, and optional `-o`; no Gradle project, Kotlin application, method channel, or user-authored JNI shim is generated.
  - [x] `flux run android`.
  - [x] The command builds the selected ABI, installs the resulting APK, and launches its concrete exported activity. Runtime selection is ABI-aware: matching ADB devices are preferred, but a running matching Waydroid container is selected instead of attempting an incompatible ADB install; known mismatches fail before installation. `--device <adb-serial>|waydroid` explicitly disambiguates hosts with multiple runtimes, and omitting `--abi` probes that selected runtime before compilation. Device discovery/ABI probes are time-bounded, explicit-device runs skip unrelated transports, and install/launch operations now fail with actionable timeouts instead of hanging indefinitely on a stale ADB/Waydroid connection. ADB launches use the exact compiler-selected `FluxActivity` or framework `NativeActivity` component with `am start -W` instead of a package-only implicit intent, so launcher resolution failures cannot be reported as successful runs. The command has been exercised end to end on Nox through build → install → explicit launch → live/resumed-process verification on x86_64 Waydroid while arm64 ADB devices were also connected.
- [x] APK output.
  - [x] Bootstrap packaging generates the binary manifest, bundles `lib/<abi>/libflux.so`, zip-aligns the artifact, signs it, and verifies the final APK. Nox validation confirmed v1/v2/v3 signatures, minSdk/targetSdk metadata, the NativeActivity launcher, native ABI declaration, and the packaged Flux shared library.
- [x] AAB output.
  - [x] `flux build android ... --format aab` compiles all supported ABIs, converts the generated manifest/resource table to protobuf form with `aapt2`, assembles a valid base module with bundletool, signs the resulting App Bundle with the compiler-managed development key, and verifies the bundle. Nox validation confirmed bundletool accepts the release AAB and sees arm64-v8a, x86_64, and armeabi-v7a native libraries.
- [x] Debug/profile/release variants.
  - [x] Android uses the same explicit optimization contracts as native builds: debug (`-O0`, full debug), profile (`-O2`, debug/frame pointers), and release (`-O3`, LTO). All three modes have produced Android artifacts on Nox.
- [x] Android manifest generation.
  - [x] Flux generates package/version/SDK/application/launcher metadata itself rather than requiring an application-authored Android manifest; packages with native View rendering point at the compiler-generated `FluxActivity` subclass while headless/native-only bootstrap shapes can retain the framework `NativeActivity` directly. The generated Java-to-DEX step includes every compiler-emitted class (including nested/helper classes) rather than assuming the activity is a single `.class`, preventing runtime class omissions as compiler-owned Android glue grows.
- [x] Package/application ID configuration.
  - [x] Optional `[android].application_id` accepts a validated lowercase reverse-DNS identifier; otherwise Flux derives a deterministic `app.flux.<package>` ID from the package name.
- [x] minSdk/targetSdk configuration.
  - [x] Optional integer `[android].min_sdk` / `target_sdk` fields are validated (`minSdk >= 21`, target >= min) and drive both NDK compiler selection and generated APK metadata; bootstrap defaults are minSdk 23 / targetSdk 36, matching the Google Play phone/tablet target-API requirement effective August 31, 2026 while older targets remain explicitly selectable where needed.
- [x] Signing configuration.
  - [x] Development signing is compiler-owned: Flux creates/reuses its own debug keystore, signs aligned APKs with `apksigner`, signs AABs with `jarsigner`, and verifies the results.
  - [x] Release builds may configure `[android].keystore` plus `[android].key_alias`; relative keystore paths resolve from the package root, while passwords stay out of source and are supplied through `FLUX_ANDROID_KEYSTORE_PASSWORD` with optional `FLUX_ANDROID_KEY_PASSWORD` override. APK signing uses apksigner environment-password sources and AAB signing uses jarsigner environment-password sources so credentials do not appear in command arguments. Fresh-key Nox E2E builds verified both release formats against the configured signing certificate; unsigned/missing-key and missing-credential paths fail explicitly. Play App Signing can consume the resulting signed upload AAB without requiring a Flux-specific credential store.
- [x] Android lifecycle integration.
  - [x] Headless/native-only bootstrap apps wire typed Flux `app` `onStart`, `onResume`, `onPause`, `onStop`, `onExit`, `onConfigurationChanged`, and `onLowMemory` callbacks directly through `ANativeActivityCallbacks`; View-backed apps route the same lifecycle through the compiler-generated `FluxActivity` and direct JNI thunks. Both paths clear compiler-owned activity handles on destruction.
  - [x] Process-recreation state is compiler-owned and typed on both Android entry paths: `onSaveState: fn() -> str` returns an app-defined payload and `onRestoreState: fn(str) -> void` receives it after recreation. NativeActivity uses its saved-state buffer directly; generated View-backed activities persist the same payload through `Bundle` while application code contains no JNI/JVM serialization shim.
- [x] Activity/window integration hidden behind Flux application APIs.
  - [x] Flux source continues to declare only `app ViewName(...)`, with no Activity subclass or JNI bootstrap in application code. The compiler uses framework `NativeActivity` for native-only shapes and generates the minimal ordinary `FluxActivity`/Dex/JNI boundary when Android Views are required, including configuration-change handling and native lifecycle routing.
- [x] Compiler-owned Android interop lowering: current Android platform calls use direct NDK/C ABI lowering where available (`android.sdkInt()`), framework-only APIs share one compiler-emitted JNI attach/release boundary, generated `FluxActivity`/DEX native declarations route required JVM-side integration without application bridge code, and framework callbacks (including lifecycle/UI/deep-link callbacks) return directly into compiled Flux functions. Broader binding/API coverage remains tracked by the platform-module/FFI items below rather than reopening the interop mechanism.
  - [x] Framework-bound intrinsics share one compiler-emitted JNI attachment boundary: `android.vibrate(durationMs)` calls the vibrator service, `android.openUrl(url)` constructs/launches an Android `ACTION_VIEW` intent, `android.share(text)` constructs a native `ACTION_SEND` chooser, `android.setClipboardText(text)` writes native plain-text clipboard data through `ClipboardManager`/`ClipData`, `android.showKeyboard()` / `android.hideKeyboard()` control the focused native view through `InputMethodManager`, generic runtime permissions call `checkSelfPermission`/`requestPermissions`, and notification APIs directly use `NotificationManager`, `NotificationChannel`, `Notification.Builder`, runtime permission methods, and `PendingIntent`, including compiler-owned UTF-8→Java string conversion. No application JNI/Kotlin/Java bridge source or serialized method-channel protocol is generated. Non-Android native builds reject reachable `android.*` calls during code generation while unreachable platform-only functions can be tree-shaken safely.
- [ ] First-class target-specific `android.*` Flux modules generated/maintained from stable platform bindings where practical, with compile-time API-level/target checks and no reflection-based runtime dispatch requirement.
  - [x] Bootstrap `android` platform namespace participates in ordinary static checking plus LSP qualified completion/signature help; current members include `android.sdkInt()`, vibration, URL/share intents, native clipboard writes, keyboard show/hide control, programmatic focus next/previous/first/last/clear, focused-text selection/caret query/update, generic permission query/request, notification-channel creation, notification-permission query/request, plain/action notifications, and notification cancellation. `sdkInt` lowers directly through the NDK rather than JNI, giving target-specific code a typed runtime API-level gate. Broader generated bindings, compile-time availability metadata, and nested platform namespaces remain pending.
- [ ] Native rendering/input backend.
  - [x] Bootstrap flat-grid Android rendering/input slice: the compiler emits a native `GridLayout` with native `TextView`/`Button` children, row/column spans, compile-time text, and direct named `Button.onPress` callback dispatch into compiled Flux functions. Packaging automatically generates/compiles a tiny `FluxActivity`/Dex implementing the Android listener interface, with no application-authored Java/Kotlin/JNI, Gradle project, method channel, serialization bus, or plugin registry. Nox validation produced a signed x86_64 APK containing `classes.dex` + `libflux.so`, installed/launched it in running Waydroid, and produced a signed all-ABI AAB containing `base/dex/classes.dex` plus arm64-v8a/x86_64/armeabi-v7a native libraries.
  - [ ] Extend the Android renderer to functional view-state transitions/refresh, dynamic properties, remaining built-in controls, richer grid sizing/gaps, styling, keyboard/focus, and production input semantics.
    - [x] Android views now own `bool`/`i64` state plus `i64`/`bool`/`str` derived values; ordinary state-transition events recompute derived values and refresh existing native controls by compiler-owned stable element ID instead of rebuilding the hierarchy, and `text`, `visible`, and `enabled` expressions can read state, derived values, and live window/display environment bindings. Configuration changes refresh display metrics and may rebuild the view when layout/native configuration must be recreated.
    - [x] Native `TextInput`, `Toggle`, and `Radio` controls lower to `EditText`, `CheckBox`, and `RadioButton`; callbacks return directly into compiled Flux, typed text-change/IME-submit handlers receive `str`, `TextInput.multiline` enables native multiline editing, and compile-time `submitOnEnter` explicitly chooses whether Enter/IME Done submits (defaulting off for multiline and on for single-line). Toggle/radio state transitions refresh existing controls in place, and compiler-managed text/selection/focus caching preserves user input, caret/selection/composition, and focused element identity across the remaining full rebuild paths.
    - [x] Android grid lowering now honors fractional track weights, fixed logical-unit tracks, row/column spans, scaled grid padding, inter-cell gaps, per-element margins/minimum sizes, and start/center/end/fill alignment. Logical dimensions use Android's exact floating-point display density rather than a rounded scale factor, preventing systematic over/undersizing on common non-integer-density devices.
    - [x] First Android style slice: native controls support logical-unit padding, background color, solid border color/width, shared corner radius, and `Text` color/size/bold/italic/underline/strikethrough through compiler-generated framework code. `#RRGGBBAA` colors are translated correctly to Android ARGB at the framework boundary.
    - [x] Android common border parity now includes independent top/end/bottom/start widths and colors, common-color/width fallback, per-corner radii, and `none`/`solid`/`dashed`/`dotted`/`double` styles through one compiler-owned native drawable. The same per-edge color properties also lower to native GTK CSS on Linux.
    - [x] Android native shadows now honor compile-time `shadowColor`, non-negative `shadowBlur`, and signed X/Y offsets in the compiler-owned native drawable without changing Flux layout sizing; root native grids disable child/padding clipping so visual effects are not intentionally cropped at the container boundary.
    - [x] Native `Image` lowers to `ImageView` with typed `source`, `fit`, `alt`, and `canShrink` behavior; state/derived image sources refresh through the compiler-owned in-place refresh path. Nox E2E validation built and launched the resulting x86_64 APK in Waydroid.
    - [x] Android parity now covers native translate/rotate/scale transforms with configurable transform origin, per-corner radii, richer Text layout (`fontFamily`, letter spacing, line-height percentage, alignment, wrapping strategy, ellipsize, max lines), and direct tap/focus/blur/hover/leave callbacks through generated listener glue.
    - [x] Ordinary Android functional transitions use generated in-place native refresh rather than full `setContentView` reconstruction: every flat element receives a stable compiler-owned ID, refresh looks up the existing native control and reapplies refreshable dynamic properties, and programmatic Toggle/Radio selection is listener-suppressed so refresh cannot recursively dispatch Flux events.
    - [ ] Finish higher-level composition controls, dynamic styling, and remaining production-grade focus/selection/text-composition/input edge cases. Portable `onTap` now uses Android's native click-listener path instead of raw touch-up dispatch, so pointer, keyboard, and accessibility `performClick` activation converge on one platform action; tappable views are explicitly focusable and native long-click consumption prevents a completed long press from also firing tap.
- [ ] Accessibility/semantics bridge.
  - [x] Bootstrap semantic text lands on native Android Views: canonical `accessibilityLabel` and `accessibilityDescription` lower to compiler-owned `contentDescription` generation, while `tooltip` uses the platform tooltip API when available. Android refresh recomputes both label and description together when either becomes dynamic so a state-driven half cannot erase its static counterpart.
  - [x] Common typed `accessibilityHidden: bool` explicitly removes native elements and descendants from the accessibility tree without changing visual visibility: Linux lowers to `GTK_ACCESSIBLE_STATE_HIDDEN`, Android to `IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS`, and state-driven Android values refresh in place. Roles/actions/richer state semantics remain pending.
- [x] Notifications callable directly from Flux, including channels/actions where supported, with compiler-generated framework interop where required.
  - [x] `android.createNotificationChannel`, `android.notificationPermissionGranted`, `android.requestNotificationPermission`, `android.notify`, `android.notifyUrlAction`, and `android.cancelNotification` lower directly through compiler-owned framework JNI. API 33+ `POST_NOTIFICATIONS` is inferred into the generated manifest only when reachable code requires it; URL actions use an immutable/updateable Android `PendingIntent` without application-authored Java/Kotlin/JNI. Nox validation builds the action-enabled example as a signed x86_64 release APK, installs/launches it in Waydroid, and builds the all-ABI release AAB.
- [ ] WorkManager/background-job integration callable directly from Flux; generate required `Worker`/`ListenableWorker` JVM entry classes and route execution into compiled Flux without user-authored Kotlin/Java glue.
- [x] Device vibration/haptics callable directly from Flux through the appropriate Android native/framework API for the configured API level.
  - [x] `android.vibrate(durationMs)` works from ordinary Flux code, tree-shakes its JNI helper when unused, automatically adds `android.permission.VIBRATE` only when reachable, and has been compiled into signed APK/AAB artifacts on Nox; the APK also installs/launches in Waydroid without application-authored bridge code.
- [x] Intents/deep links for the current Android application surface.
  - [x] Outbound URL/settings intents use compiler-owned direct framework interop: `android.openUrl(url)` creates an `ACTION_VIEW` intent, `android.openAppSettings()` opens application details, and `android.openNotificationSettings()` opens app-specific notification settings on API 26+ with an older-device fallback. Inbound routing is likewise compiler-owned: `[android].deep_links` declares validated URI prefixes that become native `VIEW`/`DEFAULT`/`BROWSABLE` manifest filters, while typed application `onOpenUrl: fn(str) -> void` receives both the launch URL and subsequent `singleTop` intents through the generated Activity/JNI boundary. Application source needs no Activity subclass, Kotlin/Java/JNI bridge, method channel, serialization bus, or plugin registry; broader third-party/native intent interoperability remains under the platform-binding/FFI track.
- [x] Share sheet.
  - [x] `android.share(text)` opens the native Android chooser with an `ACTION_SEND` `text/plain` intent and compiler-owned UTF-8/JNI glue; Flux application code contains no Kotlin/Java/JNI bridge code.
- [x] Permissions.
  - [x] Notification permission is a typed Flux capability: API 33+ grant state can be queried/requested directly, older Android versions require no runtime prompt, and generated manifests include `POST_NOTIFICATIONS` only for reachable notification code.
  - [x] General Android permissions are declarative and typed without Gradle: `[android].permissions = ["android.permission.CAMERA", ...]` is parsed, validated, deterministically de-duplicated, and emitted as `<uses-permission>` entries; `android.permissionGranted(permission)` and `android.requestPermission(permission)` lower directly to compiler-owned framework JNI and appear in static checking plus LSP completion/signature help.
- [ ] Background services/tasks where allowed.
- [ ] Camera.
- [ ] Microphone.
- [ ] Media playback.
- [ ] Secure storage.
- [ ] Files/media picker.
- [ ] Android keyboard/IME.
  - [x] Bootstrap native text input supports compiler-managed autofocus, password input type, maximum length, live `onChange: fn(str)`, native multiline editing, compile-time `submitOnEnter` policy, and IME Done/Enter `onSubmit: fn(str)` callbacks without application Java/Kotlin code.
  - [x] Portable compile-time `TextInput.keyboardType` selects `text`, `email`, `number`, `decimal`, `phone`, or `url`; Android lowers to the corresponding native `InputType` flags and Linux lowers to the corresponding GTK input purpose, with unsupported values rejected at compile time.
  - [x] Compiler-owned Android text caching preserves caret/selection plus active IME composing ranges across the full native-view rebuilds still required for configuration/layout recreation; ordinary functional state transitions now avoid that rebuild path entirely.
  - [x] `android.showKeyboard()` / `android.hideKeyboard()` directly control the soft keyboard for the currently focused native view through compiler-owned `InputMethodManager` JNI and tree-shake away when unused.
  - [x] Android full-view rebuilds required by configuration/layout recreation preserve the currently focused stable element ID and restore focus onto its replacement without re-firing Flux focus transitions during compiler-driven replacement; ordinary state refresh preserves the existing focused view directly.
  - [x] Programmatic focus movement for the current native control through typed `android.focusNext()`, `android.focusPrevious()`, and `android.clearFocus()` calls; Android lowers these through compiler-owned `getCurrentFocus`/`focusSearch`/`requestFocus`/`clearFocus` JNI, tree-shakes the helper when unused, exposes completion/signature help, and was verified on a physical Samsung by moving focus first → second → first and clearing it.
  - [x] Explicit traversal-edge targeting through typed `android.focusFirst()` / `android.focusLast()`: compiler-owned JNI uses Android `FocusFinder` against the Activity content root, requests the first forward/last backward focus target, tree-shakes the helper when unused, and exposes both calls through static checking plus LSP completion/signature help.
  - [x] Focused `TextInput` selection/caret APIs: `android.selectionStart()` / `selectionEnd()` return the current native selection or `-1` when unavailable; `android.setCaret(position)` / `setSelection(start, end)` return `bool` and reject missing focus or invalid/out-of-range positions rather than clamping or throwing through JNI. The helper verifies an actual `EditText`, tree-shakes when unused, appears in LSP completion/signature help, and was verified on physical Samsung hardware by inserting at caret 2 (`abcd` → `abXcd`) and replacing selection 1..3 (`abXcd` → `aYcd`).
  - [x] Multiline submit policy: Android `TextInput.multiline` uses native multi-line input flags; `submitOnEnter` defaults to `false` for multiline editors and `true` for single-line fields, while an explicit `true` consumes Enter/IME Done through the typed `onSubmit` callback. Numeric/decimal/phone keyboard modes and password input reject multiline combinations that do not have coherent native text semantics. Linux now follows the same default Enter policy through native `GtkTextView`/`GtkTextBuffer` lowering; unsupported multiline placeholder/max-length/password combinations fail explicitly instead of silently degrading to a single-line entry.
  - [x] Configurable traversal wrapping: `android.focusNext()` / `focusPrevious()` keep non-wrapping native traversal by default, while an optional typed `wrap: bool` retries from the opposite native focus edge only when ordinary `focusSearch` cannot move focus. The no-argument path retains its smaller tree-shaken runtime and does not pull in `FocusFinder` edge traversal unless wrapping or explicit first/last focus is reachable.
  - [ ] Focus scopes and broader configurable traversal policy.
- [ ] Platform FFI/bindings to Kotlin/Java/NDK where needed, generated or compiler-owned for normal platform access rather than exposed as application bridge boilerplate.
- [x] Verify generated Android glue is tree-shakeable and zero/near-zero overhead beyond boundaries Android itself requires; no mandatory Flutter-style channel serialization or plugin registry.
  - [x] Android runtime helpers are emitted from reachable generated-call usage rather than linked as a permanent platform runtime. Compiler tests prove unreachable notification channel/post/action/cancel code omits `Notification.Builder`, `PendingIntent`, notification helpers, and permission machinery entirely; platform calls cross JNI directly with no serialization bus, plugin registry, or user-written bridge.
- [x] Play Store-ready release pipeline for compiler-owned upload artifacts: `flux publish android <package>` performs a non-interactive release/all-ABI AAB build, refuses development-key fallback, requires explicit version metadata plus configured upload-key credentials, enforces the current API 36+ phone/tablet submission target, and validates the final signed bundle with both `bundletool validate` and standard `jarsigner` signature verification. Play Console account/listing/policy/rollout actions remain external rather than hidden compiler side effects.
  - [x] Publishing version metadata is configurable without Gradle: `[package].version` becomes Android `versionName`, `[android].version_code` becomes `versionCode`, defaults to 1, and rejects values outside Google Play's accepted 1..=2100000000 range. The default targetSdk is API 36 for current phone/tablet Play submissions, and signed all-ABI AAB generation remains directly consumable by Play App Signing.
- [x] First real production-style Flux Android app built and tested on physical hardware: the polished `examples/mobile_showcase` application compiles from Flux source into the generated native Android activity/APK and has been repeatedly installed/launched successfully on a physical Samsung SM-S931B, including the semantic typography, native styling, scrolling, toggle state, and primary-action surface used by the README showcase.

# Milestone 16 — Desktop targets

## Linux

- [x] Native Linux window/render/input backend: the current GTK4 backend owns the real Linux application window, native flat-grid rendering, pointer/keyboard/text input, focus, and direct callback dispatch without a browser/widget-emulation runtime. A dedicated compiler regression now exercises window + Text + TextInput + Button lowering together and rejects Android/web runtime leakage.
  - [x] Bootstrap GTK4 backend opens a real native Linux window, lowers Flux flat grid/Text/Button to native controls, and dispatches native button clicks into Flux callbacks.
- [x] Wayland-first support with appropriate fallback where practical.
  - [x] Bootstrap GUI was built and launched end-to-end on a real `wayland-0` compositor (`GDK_BACKEND=wayland`); GTK4 supplies the native Wayland integration while the eventual lower-level backend remains open.
  - [x] Linux GUI startup now prefers GDK's Wayland backend before X11/other available backends when the user has not selected a backend explicitly, while preserving `GDK_BACKEND` as the authoritative opt-in override for debugging, compatibility, and non-Wayland sessions.
- [ ] Menus/tray/clipboard/file dialogs.
  - [x] Portable application clipboard writes through typed `clipboard.setText(text)`: Linux lowers directly to the native GDK clipboard on the current GTK application backend, Android reuses the compiler-owned direct `ClipboardManager`/`ClipData` JNI boundary, LSP completion/signature help expose the shared API, unreachable use tree-shakes completely, and headless use is rejected rather than silently depending on GUI state. A native Linux debug build verified the GDK lowering; clipboard reads remain sequenced behind owned string semantics.
- [ ] Multi-window.
- [x] Packaging formats/integration guidance for the current Linux target: `flux package` produces either the existing host-labelled directory bundle or a directly distributable `--format tar.gz` archive with normalized ordering/timestamps/ownership metadata; same-host identical archives are byte-reproducibility tested, extracted archives execute successfully, and distro-specific deb/rpm/Arch/AppImage/Flatpak integration remains documented as an explicit downstream layer rather than hidden compiler behavior.

## Windows

- [ ] Native Windows window/render/input backend.
- [ ] Win32/modern Windows integration as appropriate.
- [ ] Menus/tray/clipboard/file dialogs.
- [ ] Multi-window.
- [ ] MSIX/store packaging.

## macOS

- [ ] Native macOS backend.
- [ ] Menu bar/application menu conventions.
- [ ] Multi-window.
- [ ] Signing/notarization/App Store packaging.

# Milestone 17 — iOS target

- [ ] Native iOS application backend.
- [ ] IPA/archive output.
- [ ] Signing/provisioning.
- [ ] Entitlements.
- [ ] UIKit/other native integration hidden behind Flux APIs.
- [ ] Notifications.
- [ ] Deep links.
- [ ] Background modes.
- [ ] Camera/microphone/media/file APIs.
- [ ] App Store packaging/release workflow.

# Milestone 18 — Web target

Web is a supported target, not the implementation substrate for native apps.

- [ ] Browser target.
- [ ] WASM application-code backend where appropriate.
- [ ] DOM/browser-native integration where appropriate.
- [ ] Browser routing/history.
- [ ] Browser storage.
- [ ] Accessibility/semantics.
- [ ] Responsive web layout.
- [ ] Development server.
- [ ] Automatic save-triggered refresh/hot apply.
- [ ] Production bundling/tree shaking.

# Milestone 19 — Platform interoperability and plugin ecosystem

- [ ] Stable C FFI.
- [ ] Generated C header/binding support.
- [ ] Android Kotlin/Java/NDK bindings, with compiler-generated glue sufficient for ordinary platform API use directly from Flux; manual FFI remains an advanced escape hatch for third-party/native libraries, not the default Android programming model.
- [ ] Apple Swift/Objective-C bindings following the same no-user-written-bridge principle where platform ABI/runtime constraints allow it.
- [ ] Windows native API bindings.
- [ ] Linux native library bindings.
- [ ] Callbacks from native code into Flux.
- [ ] Safe ownership rules across FFI boundaries.
- [ ] Platform implementation modules behind shared interfaces.
- [ ] Package registry.
- [ ] Semantic versioning/resolution.
- [ ] Reproducible lockfile.
- [ ] Native/plugin package metadata.
- [ ] Per-platform package implementations.

# Milestone 20 — Accessibility and internationalization

- [x] Semantic roles: every built-in native element accepts compile-time `accessibilityRole` values (`label`, `heading`, `button`, `textBox`, `checkbox`, `radio`, `image`, `switch`). Linux lowers the role through GTK accessibility role-description metadata and heading level, while Android installs a compiler-owned native accessibility delegate that exposes the matching platform class semantics plus API 28+ heading state. LSP value completion exposes the role vocabulary, invalid/dynamic roles fail statically, and applications write no platform accessibility bridge code.
- [x] Accessible labels/descriptions: every built-in native element accepts typed `accessibilityLabel: str` and `accessibilityDescription: str`; Linux lowers them to GTK accessible label/description properties on the existing native control, refreshes state-derived accessible text alongside the visible UI, and the properties flow through the compiler-owned UI contract/LSP surface without exposing platform accessibility objects.
- [x] Accessibility-tree visibility: every built-in native element accepts typed `accessibilityHidden: bool`; Linux maps it to native accessible hidden state and Android maps it to view accessibility importance independently of visual `visible` state, including in-place state-driven refresh.
- [ ] Actions. Android portable tap/long-press handlers now use native click/long-click actions, so accessibility services can invoke the same compiled Flux tap path through platform `performClick`; richer named/custom semantic actions and equivalent passive-control screen-reader actions on Linux remain pending.
- [x] Focus ordering for assistive traversal on current native UI backends: every built-in element accepts a unique compile-time non-negative `accessibilityOrder: i64`; ordered elements form an explicit GTK `FLOW_TO` reading chain and Android `setAccessibilityTraversalAfter` chain while unordered elements retain platform-default ordering.
- [x] Screen-reader bridges for the current native UI backends: built-in controls remain real GTK4 and Android platform controls in the native accessibility tree, while compiler-owned `accessibilityLabel`, `accessibilityDescription`, `accessibilityHidden`, and traversal ordering lower onto those platform semantics rather than a parallel widget/emulation tree. Cross-backend regression coverage verifies the native control/accessibility surfaces; custom semantic roles/actions remain tracked separately.
- [x] Native text scaling for current application backends: Flux `Text.size` and semantic typography roles stay in scalable platform font units instead of raw pixels. Linux emits Pango point sizes so GTK's system font DPI/settings remain authoritative, while Android's generated `TextView.setTextSize(float)` path keeps scaled-pixel (`sp`) semantics and follows the user's font-size preference; unoverridden native controls continue to inherit platform/theme sizing.
- [x] Native high-contrast adaptation for current application backends: Linux binds generated theme CSS to GTK's live interface-contrast preference and adds stronger native outlines/theme foreground/background treatment when increased contrast is requested; Android API 34+ reads `UiModeManager` contrast, refreshes when it changes, and uses higher-contrast semantic palette values while preserving explicit application color overrides.
- [x] Reduced motion for the currently supported transition backend: generated native transition styling follows GTK's live system reduced-motion setting and suppresses duration/delay without requiring application code.
- [x] Keyboard-only navigation for the current native UI backends: native Button/TextInput/Toggle/Radio controls retain platform focus traversal and keyboard activation; common typed `focusable: bool` explicitly includes/excludes built-in elements and may be state-derived, with `onKey` implying focusability only when no explicit override exists. Passive Linux Text/Image elements with `onTap` become focusable and map Enter/Space to the same typed Flux tap action unless an explicit `onKey` handler takes control, while Android `onTap` lowers through a focusable native `OnClickListener` so D-pad/keyboard activation reaches the same action without synthetic touch events. No framework focus manager or platform object is exposed to Flux source.
- [x] Locale detection for current native targets: portable `locale.language()` and `locale.region()` return borrowed language/region strings without platform-specific source imports; Linux follows `LC_ALL` → `LC_MESSAGES` → `LANG` and normalizes the POSIX `C` locale to `und`/no region, while Android reads `java.util.Locale.getDefault()` through compiler-owned JNI. Both paths are statically typed, exposed through LSP completion/signature help, and tree-shaken when unreachable.
- [ ] Translation resources.
- [ ] Plural/select rules.
- [ ] Number/date/currency formatting.
- [x] RTL layout for current native application backends: compile-time `layoutDirection: "system" | "ltr" | "rtl"` application metadata sets the root GTK/Android native direction without a framework layout layer; `system` preserves locale/platform inheritance while explicit direction flows through native logical start/end layout behavior.
- [x] Locale-aware text/input behavior for current native application backends: Linux keeps GTK/Pango plus the native GTK input method authoritative for locale-sensitive shaping/input, while compiler-generated Android text controls apply the current `Configuration` locale list and propagate the same locale hints to `EditText` IMEs on API 24+, with `Locale.getDefault()` compatibility on older supported releases. Android configuration changes rebuild the native surface so locale changes are picked up without application code or a platform bridge.

# Milestone 21 — Media, persistence, and application services

- [ ] Raster image codecs.
- [ ] Vector image support.
- [ ] Video playback.
- [ ] Audio playback.
- [ ] Camera capture.
- [ ] Microphone capture.
- [ ] Hardware decoding interfaces.
- [ ] Files.
- [ ] Preferences/settings.
- [ ] Secure storage.
- [ ] SQLite interface.
- [ ] Database driver/plugin model.
- [ ] HTTP client.
- [ ] WebSocket client/server.
- [ ] Background networking.
- [ ] Serialization/code generation where useful without source generics.

# Milestone 22 — Testing

- [x] Bootstrap `flux test`: direct `.flux` tests or sorted package `tests/*.flux` programs compile through the ordinary checker/native backend and pass only on a successful real process exit; GUI `app` tests are rejected in this headless integration slice.
- [ ] Unit tests.
- [x] Bootstrap integration tests use ordinary headless `fn main() -> i64` Flux programs; `flux new` scaffolds `tests/smoke.flux` so a new package is immediately testable. Richer assertions/fixtures and package-source test imports remain follow-up work rather than hidden test-only language magic.
- [ ] UI tests.
- [ ] Golden/screenshot tests.
- [ ] Accessibility tests.
- [ ] Platform end-to-end tests.
- [ ] Deterministic test clocks/timers.
- [x] Dependency/interface fakes based on functions/interfaces rather than mocking class hierarchies: first-class function values provide lightweight single-capability injection, while ordinary interface values accept small fake implementations for grouped capabilities. `docs/testing.md` documents both patterns, and compiler regression coverage verifies the fake path uses normal typed native lowering without proxy objects, vtables, reflection, or heap allocation.
- [x] Source-level coverage through `flux test --coverage`: opt-in LLVM instrumentation is merged across executed tests and mapped back to original `.flux` source lines rather than generated C/stdin locations; ordinary test binaries remain uninstrumented and `flux doctor` reports the required coverage tools.
- [x] Benchmarks: the checked-in Flux/Rust/C native performance harness covers compute, collections, interface dispatch, and dynamic calls/control flow with output equivalence, runtime/code-size reporting, a checked-Rust threshold, and a push/PR CI performance gate.
- [x] Compiler self-tests across supported targets: `tools/flux-self-test` runs the complete Rust/compiler suite, then performs a real optimized Linux native build/execution and an Android x86_64 release APK build through the user-facing compiler. `.github/workflows/compiler-self-test.yml` provisions the Android SDK/NDK and runs the same cross-target self-test on every main push and pull request, so target-lowering/toolchain regressions fail CI rather than remaining codegen-only coverage.

# Milestone 23 — LSP and editor experience

- [x] Diagnostics reuse the same structured diagnostic model and source spans as the terminal renderer, including open-buffer project overlays; there is no separate editor-only error model.
  - [x] Bootstrap `fluxc lsp` speaks stdio JSON-RPC/LSP, negotiates UTF-8/UTF-16 positions, uses full-document `didOpen`/`didChange`/`didClose` synchronization, and publishes the compiler's existing parse/type diagnostics for open standalone buffers including related labels and structured fix data.
  - [x] Workspace/import-aware unsaved-buffer diagnostics: the project loader accepts in-memory source overlays keyed by canonical path, and `fluxc lsp` type-checks imported open documents against the real import graph while substituting every currently open unsaved Flux buffer. Diagnostics are republished across open dependent documents after overlay changes and filtered to each document's stable source ID.
- [x] Completion.
  - [x] Bootstrap `textDocument/completion` provides Flux keywords, builtin types/functions, current-module declarations, public declarations from the forward import graph, and position-aware parameters/local bindings declared earlier in the current function, including unsaved overlay signatures/details. Local suggestions are filtered by active indentation scope so completed branches/loops do not leak bindings.
  - [x] Qualified namespace completion for enum variants and interface capabilities works at incomplete member edits such as `Outcome.` / `Storage.`, including explicitly imported declarations. A tolerant probe preserves declarations/imports before the active broken top-level declaration rather than requiring the edit to parse first.
  - [x] Struct-field completion follows explicitly typed visible parameters/`let`/`var` values, transparent struct aliases, nested field paths, and single-return named function-call results while the active `value.` expression is incomplete; contracts come from the same local/imported program model. More complex arbitrary expressions can build on typed expression recovery later without introducing method semantics.
- [x] Signature help for every current parenthesized Flux call form: ordinary/imported functions, first-class concrete `fn(...) -> ...` values, namespace-qualified interface capabilities with an explicit receiver, enum payload constructors, interface packing, and compiler built-ins (`print`/`error`). Active arguments are tracked across nested calls. Declarative view composition is property-oriented rather than a call surface and remains covered by UI completion/hover.
- [x] Hover/type information.
  - [x] Bootstrap `textDocument/hover` reports exact declaration types/signatures and safely resolves unambiguous usage-site hover across the loaded forward import graph, including unsaved overlays. Scope-resolved local shadowing and reverse-workspace discovery remain pending with richer reference indexing.
  - [x] Struct member hover is receiver-type-aware for nested field paths, so repeated field names across unrelated structs resolve to the field selected by the actual typed receiver instead of requiring global field-name uniqueness.
  - [x] Local hover resolves parameters/bindings by the active function and lexical visibility rules already shared with completion, so same-named locals in other functions no longer make an otherwise unambiguous usage disappear.
- [x] Go to definition.
  - [x] Bootstrap `textDocument/definition` navigation uses the shared semantic database and resolves declarations/usages only when the target name is unambiguous; when the current file imports modules, typed overlay project analysis resolves public imported declarations to their actual source-file URI/range. Scope-aware shadow resolution and reverse-workspace discovery remain pending.
  - [x] Struct member go-to-definition follows the receiver's concrete/aliased struct type through nested field access and navigates directly to the selected field declaration, including imported project sources.
  - [x] Local go-to-definition uses the active function plus lexical visibility rather than global name uniqueness, resolving same-named parameters/bindings in separate functions safely.
- [x] Find references.
  - [x] Bootstrap `textDocument/references` returns identifier occurrences only after unambiguous semantic target resolution; strings are excluded and Flux has no comment syntax. From an importing document, references cover the entire typed loaded dependency graph, including imported declarations and unsaved overlays. Reverse dependents outside that graph still require workspace indexing.
  - [x] Function-local parameter/binding/pattern/loop references are resolved through lexical visibility rather than global name uniqueness, so same-named locals in other functions/scopes are excluded from the result.
- [x] Rename.
  - [x] Bootstrap `textDocument/rename` emits workspace edits only for an unambiguous semantic target and a valid non-keyword Flux identifier; from an importing document it safely edits every occurrence in the typed loaded dependency graph and rejects collisions with existing semantic symbols. Ambiguous/shadowed names are refused rather than edited unsafely; reverse dependents outside the loaded graph remain pending.
  - [x] Function-local parameter/binding/pattern/loop rename is lexical-scope aware, edits only occurrences resolving to the selected local, and refuses names colliding with another local/parameter in the same function.
- [x] Semantic highlighting through full-document LSP semantic tokens: Flux keywords, strings, numbers, operators, builtin types/functions, and semantically resolved declarations/unambiguous usages receive standard token categories with negotiated UTF-8/UTF-16 positions. There is intentionally no comment token category because comments are invalid Flux syntax.
- [x] Formatting through `textDocument/formatting`, returning the canonical Flux formatter as a whole-document LSP text edit without a second formatting implementation.
- [x] Code actions/fixes for machine-applicable compiler diagnostics: `textDocument/codeAction` returns preferred `quickfix` workspace edits directly from each diagnostic's structured replacement fix.
- [x] Inlay hints where they add value: `textDocument/inlayHint` reports compiler-inferred types for struct/enum pattern bindings and range-loop variables, while deliberately omitting already-explicit `let`/`var`/parameter types.
- [ ] Incremental workspace analysis.
  - [x] Reusable typed project-analysis caching: unchanged source graphs are reused across LSP completion/hover/signature/navigation/refactoring requests after validating every loaded source against the current unsaved overlay or disk text. Open/change/close invalidates cached graphs containing that path, and manifest-backed caches also validate the current `flux.toml` contents.
  - [x] Per-module parse reuse inside changed project graphs: the project analysis cache keeps parsed ASTs keyed by canonical source path plus exact source text, so invalidating/changing one module reuses unchanged module ASTs and reparses only changed sources. Parse-cache hit/miss statistics are covered by project tests.
  - [ ] Incremental type checking inside a changed project graph. Merged whole-program type analysis still reruns after any module AST changes; safely reusing unaffected typed module state requires explicit semantic dependency facts.
- [ ] Package/module awareness.
  - [x] Open-buffer import graphs use the compiler's real module loader and in-memory overlays, preserving module visibility/import semantics across unsaved files.
  - [x] Bootstrap workspace/package discovery considers canonical open-file targets plus the nearest enclosing `flux.toml`, chooses the broadest valid typed analysis containing the current document, and powers imported completion/hover/signature help, go-to-definition, and dependency-graph references/rename.
  - [ ] Reverse-dependency/scope indexing for dependents outside the current loaded graph and fully resolved usage-site shadowing across editor services. Typed project analyses are now cached for unchanged graphs; per-module incremental rebuilds remain tracked above.
- [x] UI syntax awareness.
  - [x] Contextual `textDocument/completion` inside flat `view` blocks uses compiler-owned built-in element/property contract metadata: grid declaration and built-in element kinds are suggested at layout indentation, built-in properties are typed and exclude already-declared properties, and composed view parameters are offered as typed properties when the current source/project parses.
  - [x] UI contract hover recognizes flat element-kind and property positions: built-in element/property types come directly from compiler contracts and remain available even while the surrounding view is temporarily incomplete; valid composed views show their typed parameter contract.
  - [x] Composed-view property navigation maps a property usage such as `Badge.label:` to the declared `label` parameter on `view Badge(...)`, including public imported views resolved through typed project overlays.
  - [x] Malformed-view contract recovery reuses the real Flux parser on the complete top-level prefix before the currently edited view; for recovered imports it then loads dependencies through the normal project loader with package containment and unsaved overlays. Property completion, hover, and property-definition navigation therefore retain both earlier local and public imported view contracts while the current view is incomplete.
- [x] Hot-reload status integration: `fluxc run` atomically publishes versioned starting/running/compiling/compile-error/restarted/exited status with generation, mode, runner PID, timestamp, and reason; `fluxc lsp` advertises `fluxHotReloadStatus` and serves the newest applicable status through `flux/hotReloadStatus` without coupling the runner and editor into one process.

# Milestone 24 — Debugger

- [x] Source-level breakpoints. Bootstrap `flux debug` compiles an isolated native debug binary and launches GDB with original `.flux` paths/line tables; repeated `--break file.flux:line` or function breakpoints can be installed before the interactive debugger starts.
- [x] Stepping. GDB `step`/`next` operate on the compiler-preserved Flux source lines rather than requiring developers to inspect generated C.
- [x] Stack frames. Native backtraces resolve frames to original Flux files and source lines through the compiler-emitted debug locations.
- [x] Locals. The debugger embeds a Flux-aware GDB support layer: `flux-locals` enumerates the currently visible parameters/locals using source-language names and values, and `flux-print name` resolves one visible Flux local without requiring the developer to know compiler-generated `flux__local_*` symbols.
- [x] Watches. `flux-watch name` tracks a visible Flux local and reports it with its source-language name whenever execution stops; `flux-unwatch name` removes the watch and out-of-scope transitions are explicit instead of silently binding a different native symbol.
- [x] Expression evaluation. Bootstrap `flux-eval expression` evaluates side-effect-free primitive Flux expressions directly over visible debugger locals instead of forwarding source text into GDB/C: `i64`/`bool`/`str` values, parentheses, unary `!`/`-`, checked integer arithmetic/division, comparisons/equality, and short-circuit `&&`/`||` follow Flux semantics. Function calls, assignment/platform effects, and aggregate/member evaluation remain intentionally unavailable until they can preserve Flux ownership/effect rules.
- [ ] Async/task debugging.
- [ ] Ownership-aware value inspection.
- [x] Native debug metadata. Generated C carries source-aware `#line` directives and debug/profile Clang modes retain DWARF line tables that resolve native instructions back to `.flux` files; this is now exercised through the user-facing debugger path.
- [x] Predictable bootstrap debug/profile/release native build behavior: `debug` uses no optimization plus full debug info/frame pointers, `profile` uses optimized code with debug info/frame pointers, and `release` uses aggressive optimization/LTO.
- [x] Platform debugger integration where useful. On the current Linux development target, `flux debug <target>` verifies GDB availability, builds with Flux debug metadata, opens the debugger with inherited terminal I/O, supports preconfigured source/function breakpoints plus optional immediate run, embeds Flux-aware local/watch commands, and cleans up its temporary binary/support script after the session. `flux doctor` reports GDB availability.

# Milestone 25 — Profiler and devtools

- [x] CPU profiling. Bootstrap `flux profile <target>` compiles with optimized profile settings plus isolated native `gprof` instrumentation, runs the exact instrumented binary with inherited terminal I/O, prints a CPU report on exit, keeps normal `--mode profile` builds free from instrumentation overhead, and cleans its temporary binary/data. `flux doctor` reports profiler availability; Flux-aware symbol/source presentation and lower-overhead production profiling remain separately tracked below.
- [x] Allocation profiling. Bootstrap `flux profile <target> --alloc` builds the ordinary optimized profile-mode native binary and runs it under glibc `memusage`, reporting heap total/peak, stack peak, allocation-call counts/bytes/failures, and block-size distribution without adding allocator/profiling runtime overhead to normal builds. Temporary binaries are removed afterward and `flux doctor` reports profiler availability; per-allocation stack attribution and leak graphs remain under the richer memory-tooling follow-up.
- [x] Bootstrap memory leak tooling through `flux profile <target> --leaks`: the CLI builds an isolated optimized native binary with Clang AddressSanitizer, enables leak detection only for that run, preserves Flux source locations through existing debug metadata, fails distinctly on sanitizer findings, and removes the temporary binary afterward. Full retained-object graph visualization remains naturally downstream of general owned heap values rather than blocking useful leak diagnostics now.
- [ ] Task/async timeline.
- [ ] Frame/render timeline.
- [ ] Layout diagnostics.
- [ ] Network inspection.
- [x] Native symbolization. Bootstrap CPU profiles demangle compiler-generated `flux__fn_` application symbols back to Flux function names while leaving compiler/runtime helpers visibly native; profile-build debug symbols remain available to ordinary native tooling.
- [x] Source mapping. `flux profile` collects sampled native addresses from gprof output and resolves them through `addr2line` against the exact instrumented binary, replacing compiler-stdin locations with original `.flux` file/line locations wherever DWARF has a source mapping while retaining a safe native fallback for prologue/runtime instructions.
- [ ] UI inspector without exposing a widget-object hierarchy.
- [ ] Performance overlays.
- [x] Low-overhead production profiling hooks: `flux profile <target> --sample` builds the ordinary optimized/profile native binary with debug information and frame pointers but no profiler instrumentation, samples it through Linux `perf record --call-graph dwarf`, prints a demangled `perf report --stdio`, cleans temporary artifacts, and leaves normal Flux binaries unchanged. `flux doctor` reports `perf` availability and the command fails clearly when the tool or kernel performance-counter permissions are unavailable.

# Milestone 26 — CLI/build/developer workflow

- [x] `flux new`: the user-facing `flux` binary shares the canonical CLI implementation and scaffolds a minimal manifest-backed native GUI package without overwriting non-empty directories.
- [x] `flux run`: the user-facing command provides the automatic development watch/build/restart loop and hot-reload status contract.
- [x] Bootstrap `fluxc build` compatibility entry remains available while the product CLI transitions to `flux`.
- [x] `flux build`: user-facing native builds support debug/profile/release modes and package/source targets.
- [x] `flux test`: user-facing native integration-test discovery/execution for direct `.flux` tests and package `tests/*.flux`, with debug/profile/release mode selection and real exit-status reporting.
  - [x] Opt-in `flux test --coverage` uses Clang/LLVM instrumentation without affecting ordinary test binaries, merges per-test profiles, and maps generated-C execution regions back onto original `.flux` source lines for a concise source-level coverage summary. `flux doctor` reports both required LLVM coverage tools, and an end-to-end test rejects regressions back to generated `<stdin>` reporting.
- [x] `flux format`: user-facing canonical formatting and `--check` mode use the existing formatter implementation.
- [x] `flux analyze`: user-facing static analysis is an explicit alias over the same project-aware compiler diagnostics as `flux check`, including `--json` for tooling/CI.
- [x] `flux devices`: the CLI reports Linux desktop display readiness, connected ADB Android devices with their real transport states, and a running local Waydroid container as a runnable Android target; external device probes are bounded so an unresponsive transport cannot stall the command forever.
- [x] `flux doctor`: doctor reports Linux host/Clang/linker/GTK/display readiness plus Android SDK, NDK, build-tools including D8, `javac` for compiler-generated Android runtime glue, ADB, optional Waydroid readiness, GDB/gprof, native crash symbolization/debug-symbol tools, and LLVM coverage tools; Neovim dogfood availability remains reported separately.
- [x] `flux clean`: target-aware cleanup removes the default native build artifact plus the runner's last-known development-status artifact and is safely idempotent when nothing exists.
- [x] Bootstrap `flux package`: manifest-backed packages build into a deterministic host-labelled bundle directory containing the package-named native executable plus the exact `flux.toml`, or a reproducible `.tar.gz` wrapping that same bundle via `--format tar.gz`; release is the default, build modes are supported, existing outputs are never overwritten, and raw source files are rejected rather than silently treated as distributable packages. Dependency vendoring, richer platform metadata, signing, and store packaging remain in the distribution/platform tracks.
- [x] Debug/profile/release modes through `flux build --mode ...` and `flux run --mode ...` (with `fluxc` compatibility); builds default to `release`, while the development runner defaults to `debug` for fast rebuilds and native debug information.
- [x] Cross-compilation configuration: native `flux build` and `flux package` accept explicit Clang `--target <triple>` plus optional `--sysroot <directory>`, target/sysroot selections participate in native cache identity, default package names carry explicit target triples, host-execution commands reject cross-target options rather than pretending to run foreign binaries, and the configured path is regression-tested with the host Clang triple. Android remains a dedicated first-class target pipeline rather than being collapsed into generic Clang flags.
- [x] Bootstrap native build cache: generated C + build mode + compiler package version + host OS/architecture are content-addressed into a persistent native artifact cache (`$FLUX_CACHE_DIR`, XDG cache, or `~/.cache/flux`), transparently reused by build/run/test/package across output paths. Cache identity also includes the active Clang/linker identity and GTK version/compiler/linker flags when GTK is linked, while sidecar size/content hashes reject corrupt cached binaries before restore. Semantic/codegen incremental reuse remains tracked under the compiler/hot-reload caches; eviction, full sysroot/SDK content fingerprints, and explicit cache-management commands remain follow-up work.

## Package management and ecosystem

The first Flux package ecosystem must require **no Flux-operated hosting bill or always-on service**. The bootstrap registry uses ordinary public GitHub repositories plus GitHub Releases: package authors host immutable `.fluxpkg` source archives as release assets in their own repositories, while a small public Flux registry repository maps human-friendly package names and versions to those assets and their content hashes. This is an implementation backend rather than a language-level dependency on GitHub: manifests, lockfiles, package identities, and the registry abstraction must remain portable to another static host or mirror later without changing Flux source imports.

- [x] Extend `flux.toml` with a strict `[dependencies]` table supporting SemVer registry dependencies plus explicit local `path` and Git `rev` development dependencies; registry packages are referenced by package identity rather than raw download URLs. The bootstrap parser now exposes typed registry/path/Git dependency metadata, accepts exact/caret/tilde SemVer requirements plus `*`, requires relative local paths and revision-pinned Git sources, rejects duplicate/invalid names and mixed/unknown source shapes, and keeps actual resolution/fetching/lockfiles/package-import semantics in the following roadmap items.
- [x] Add an unambiguous package import namespace, separate from existing relative `.flux` imports, so dependency modules have stable package-qualified identities and cannot be confused with local files. `import "pkg:<dependency>/<module.flux>"` is reserved for manifest-declared dependencies; local `path` dependencies load through their own validated `flux.toml`, remain contained within that dependency root, preserve dependency-local relative imports, and receive module identities from the dependency package's own name. Registry/Git package imports are recognized but fail explicitly until deterministic resolution/fetching supplies their source tree rather than falling back to local files.
- [ ] Implement deterministic dependency resolution with one resolved version per package name in a build graph initially, SemVer constraint solving, cycle detection, and concise conflict diagnostics that show the dependency paths imposing incompatible requirements.
- [ ] Add generated `flux.lock` files recording exact package versions, immutable source/release identity, SHA-256 content hashes, and resolver/lockfile format version; lockfiles are committed for applications and libraries, never contain host-specific absolute paths/timestamps, and support `--locked` CI builds that fail rather than re-resolve.
- [ ] Add first-class dependency commands under the existing `flux` CLI: `flux add`, `flux remove`, `flux fetch`, `flux update`, `flux outdated`, `flux tree`, and `flux why`; ordinary `flux check`/`build`/`run`/`test` automatically fetch a missing locked dependency rather than requiring a separate install step.
- [ ] Add a global content-addressed package cache alongside the existing Flux build cache so identical dependency contents are stored once across projects, hash-verified before use, and usable through a strict `--offline` path; add `flux vendor` only as an explicit self-contained/offline snapshot escape hatch rather than creating per-project `node_modules`-style copies by default.
- [ ] Define canonical reproducible `.fluxpkg` source archives and use public GitHub Release assets as the zero-hosting-cost bootstrap blob store; downloads are addressed and verified by immutable version/release metadata plus SHA-256 rather than trusting mutable branch archives.
- [ ] Create a small public GitHub-backed Flux registry index containing package name, owner/source repository, available versions, Flux compatibility constraints, release-asset location, SHA-256, dependency metadata, and yank state; resolution is local/cacheable and must not require an always-on Flux database/API server.
- [ ] Implement `flux publish` for the bootstrap registry using the publisher's GitHub authentication: validate/check/test the package, create the canonical `.fluxpkg`, upload it to the package repository's GitHub Release, then prepare/submit the corresponding registry metadata change without requiring package authors or Flux maintainers to operate separate storage infrastructure.
- [ ] Make published package versions immutable; a broken version may be yanked from new resolution without deleting/replacing its bytes so existing lockfiles remain reproducible. Package-name ownership and explicit transfer rules belong to registry metadata, not artifact storage.
- [ ] Package fetch/add/publish validation must never execute dependency-controlled lifecycle hooks, install scripts, build scripts, or arbitrary shell commands. Bootstrap registry packages contain Flux source plus declared resources only; arbitrary native libraries/JVM glue/platform bridges require a future explicit audited native-package ABI rather than being smuggled through installation.
- [ ] Add workspace support with one root dependency resolution/`flux.lock`, local workspace-member package identities, and workspace-wide check/test/build commands, while preventing undeclared local paths from silently shadowing registry packages.
- [ ] Keep the registry transport/provider abstract behind Flux-owned metadata and lockfile semantics, with regression coverage proving that cached/offline installs are content-addressed and that a future static mirror or non-GitHub registry backend can be introduced without changing `flux.toml`, Flux source imports, or existing lockfile package identities.

- [ ] Reproducible builds.
  - [x] Same-host/same-toolchain native reproducibility for current debug/profile/release modes: generated C is streamed to Clang over stdin so PID-specific temporary source paths cannot leak into DWARF, and cache-isolated builds of identical Flux source are regression-tested as byte-identical in every mode.
  - [ ] Cross-machine/toolchain reproducibility contract with pinned compiler/SDK/runtime identities, normalized environment inputs, dependency lockfiles, and documented verification metadata.
- [x] CI-friendly machine output through `fluxc check --json`, preserving a clean stdout JSON contract with no human diagnostic text on stderr; `flux check --json` / `flux analyze --json` now emit the same explicitly versioned diagnostics envelope.

# Milestone 27 — Distribution

- [x] Android signing/store publishing guidance and automation hooks: `docs/android-release.md` documents upload-key configuration, Play App Signing boundaries, current target-API policy, secret handling, deterministic `-o` artifacts, and CI usage; `flux publish android ... --json` emits a clean machine-readable success object only after release AAB construction plus bundle/signature verification, while failures retain non-zero process status. Verification deliberately avoids `jarsigner -strict`, whose CA-chain policy rejects the self-signed upload keys used by ordinary Android signing.
- [ ] iOS signing/App Store workflow.
- [ ] Windows packaging/store workflow.
- [ ] macOS signing/notarization/store workflow.
- [x] Linux packaging guidance for the current GTK4 backend is documented in `docs/linux-packaging.md`, covering release bundles, runtime dependencies, conventional filesystem/desktop integration, icons, archives/distro packages, reproducibility expectations, and the boundary between current compiler automation and downstream packaging.
  - [x] Bootstrap host bundle from `flux package` provides the first runnable Linux distribution shape without claiming dependency-free/self-contained GTK packaging.
- [ ] Web production deployment.
- [x] Debug symbol separation/upload for current native Linux artifacts: `flux symbols split <binary> [-o directory]` leaves the input untouched and emits a stripped executable plus detached GNU `.debug` file with a debug link, providing an upload/retention-ready artifact for external crash services. The workflow is covered by a real Flux debug-binary E2E rather than only command construction.
- [x] Crash symbolization for current native Linux artifacts: `flux symbolize <binary-or-debug-file> <address>...` resolves native instruction addresses through preserved debug metadata back to demangled Flux function names and original `.flux` source locations, and is E2E-verified against both a live debug binary and its detached symbol file.
- [ ] Optional obfuscation where compatible with diagnostics/debugging.
- [x] Version/build-number management for current distribution targets: `[package].version` is validated package metadata, is preserved in Linux bundles, becomes Android `versionName`, and `[android].version_code` is validated against Play's accepted integer range and becomes Android `versionCode`; future platform-specific mappings belong to their target packaging milestones.

# Milestone 28 — Server/headless application target

Flux must remain equally credible for headless native binaries.

- [ ] HTTP server.
- [ ] TLS server.
- [ ] WebSocket server.
- [ ] TCP/UDP sockets.
  - [x] Bootstrap Linux/server TCP socket lifecycle through compiler-owned `net.tcpConnect(host, port)`, `net.tcpListen(host, port, backlog)`, `net.tcpAccept(listener)`, `net.localPort(socket)`, and `net.close(socket)`: connection/listener creation and accept return explicit `(i64, error)` results, constant port/backlog ranges are checked statically with runtime guards for dynamic values, host resolution supports IPv4/IPv6 through native `getaddrinfo`, ephemeral listen ports can be queried, unreachable networking support tree-shakes completely, LSP completion/signature help expose the typed surface, and Android lowering rejects the server-only capability. Buffered byte/string I/O, nonblocking/readiness integration, and ownership-typed socket resources remain pending with byte buffers/owned values/concurrency rather than being hidden behind unsafe borrowed scratch storage.
  - [x] Bootstrap Linux/server UDP socket lifecycle through compiler-owned `net.udpConnect(host, port)` and `net.udpBind(host, port)`, sharing `net.localPort(socket)` and `net.close(socket)`: connect/bind return explicit `(i64, error)` results, known-invalid ports fail statically with runtime guards retained for dynamic values, IPv4/IPv6 resolution lowers directly through native `getaddrinfo`/`SOCK_DGRAM`, ephemeral bind ports are queryable, unreachable UDP support tree-shakes completely, LSP completion/signature help expose the typed surface, and native E2E coverage exercises both bind and connect. Datagram byte I/O, nonblocking/readiness integration, and ownership-typed socket resources remain pending with byte buffers/owned values/concurrency.
- [x] Linux/server signals/process lifecycle: compiler-owned `process.terminationRequested()` lazily installs SIGINT/SIGTERM handlers whose signal-context work is limited to setting a `sig_atomic_t` flag, enabling ordinary Flux control flow to perform graceful shutdown work outside the handler; `process.exit(code)` provides explicit `0..=255` process termination with compile-time checking for known constants and a runtime guard for dynamic values. Both capabilities lower directly to POSIX/C, are tree-shaken when unreachable, participate in LSP completion/signature help, and remain rejected on Android until portable mobile process semantics are defined.
- [x] Bootstrap Linux service/daemon integration: `flux package --format systemd` emits a headless native service bundle containing the optimized Flux executable plus a generated systemd unit bound to that bundle, with restart-on-failure policy, explicit SIGTERM shutdown, bounded stop timeout, and user-service `default.target` enablement. GUI `app` packages are rejected, service names are constrained to safe unit-name characters, and compiler regression coverage verifies the emitted unit and executable.
- [ ] Structured concurrency for request handling.
- [ ] Efficient I/O.
- [ ] Database connectivity.
- [x] Static/self-contained deployment where platform permits: `flux package --format static` emits one statically linked optimized executable for headless packages, keeps static/dynamic native cache identities separate, rejects GUI applications rather than pretending GTK is self-contained, supports the existing explicit Clang target/sysroot path, and fails clearly when the selected toolchain lacks a static C runtime. Compiler E2E coverage executes the artifact and verifies the Linux ELF has no dynamic interpreter when `readelf` is available.
- [x] Container-friendly builds: `flux package --format container` emits a headless-only native container build context containing the optimized Flux executable plus a minimal `Containerfile`, without requiring Docker/Podman during compilation or conflating this with the separate static/self-contained deployment goal. Compiler regression coverage verifies the generated context and executable, while GUI `app` packages are rejected from this server-oriented format.
- [x] Bootstrap server observability hooks: compiler-owned `process.cpuMillis()` reports current-process user+system CPU time and `process.peakResidentMemoryBytes()` reports Linux peak RSS in bytes through direct `getrusage` lowering. Both are allocation-free `i64` scalar reads, participate in LSP completion/signature help, reject Android lowering with the existing desktop/server process capability boundary, and tree-shake completely when unreachable; richer request-level metrics/tracing can build on ordinary Flux code as the server I/O/concurrency model lands.

# Milestone 29 — Flux 1.0 stability

- [ ] Stable grammar.
- [ ] Stable type/memory model.
- [x] Stable package format: `flux.toml` now has an explicit compiler-owned package-format compatibility version (`PACKAGE_FORMAT_VERSION = 1`); `flux new` writes `format_version = 1`, existing unversioned manifests remain version-1 compatible, and explicitly unsupported versions fail before build rather than being silently reinterpreted. The versioning/additive-change contract is documented in `docs/package-format.md` and covered by manifest/scaffolding regression tests.
- [ ] Stable native/platform ABI policy.
- [ ] Stable UI API compatibility policy.
- [x] Stable formatter: canonical formatting is deterministic/idempotent, the CLI/LSP/library share one implementation, `flux format --check` provides the CI contract, and formatter compatibility is explicitly versioned through public `FORMATTER_VERSION` plus `flux format --version`. Version 1 changes only when already-canonical source intentionally acquires different canonical bytes; the stability contract and editor/CI guidance are documented in `docs/formatting.md` and regression-tested.
- [x] Stable diagnostics contract for tooling: machine analysis output is pinned to documented `schema_version: 1`, exposes one shared versioned envelope for successful and failed `flux check --json` / `flux analyze --json` runs, ships a checked-in JSON Schema, keeps diagnostic/span/label/note/fix field meanings explicit, and defines additive-vs-breaking compatibility rules in `docs/diagnostics.md`. The version constant and envelope serializer are public so compiler-integrating tools do not need to duplicate the wire contract.
- [ ] Android, iOS, Windows, macOS, Linux, web, and headless/server targets meet documented support tiers.
- [ ] Production-quality LSP/debugger/profiler.
- [ ] Automatic save-triggered hot reload is production-ready for development workflows.
- [ ] Package ecosystem and platform interoperability are documented and tested.
- [ ] Flux can build substantial real applications without resorting to an OOP architecture.
