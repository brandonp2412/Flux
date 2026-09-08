# Flux Roadmap

Flux aims to provide the practical application-development reach of Flutter while remaining a native, ahead-of-time compiled, function-first language. The language is built around functions, immutable data, explicit state, interfaces/capabilities, strict static typing, and composition. It does not use classes, inheritance, widget subclasses, mixins, controller hierarchies, or exception-based recoverable errors.

## Roadmap maintenance rule

This file is the source of truth for Flux development.

At the **start of every Flux development session**:

1. Read `ROADMAP.MD` before choosing work.
2. Inspect the repository state and run the existing test suite.
3. Update this roadmap to reflect the actual repository state, newly discovered work, changed sequencing, or completed prerequisites before or alongside implementation.
4. Choose one coherent highest-value unchecked subsystem that advances the current milestone.
5. Validate the change with compiler tests and native end-to-end examples where applicable.
6. Check off completed items and record meaningful follow-up work before committing.
7. Commit roadmap updates together with the implementation they describe when practical.

Do not let the roadmap become an aspirational document that diverges from the compiler.

## Priority 1 — Minimal end-to-end manual dogfooding

**This is the immediate project priority.** Until this milestone is complete, prefer work that directly enables a developer to write a small Flux GUI program in a real editor, receive first-class Flux language tooling, compile it, launch it as a real native application, interact with it, and visually verify the result. Other roadmap work should be sequenced behind this unless it is a prerequisite for this path or fixes a correctness regression in an already-supported feature.

The minimum acceptance path is deliberately small: one Linux desktop target first, one native window, the existing flat grid layout, `Text` and `Button`, one click-driven state update/repaint, and the existing save-triggered development loop. This milestone does **not** require completing the full UI toolkit, ownership model, Android target, animation, navigation, package ecosystem, debugger, or profiler before manual dogfooding starts.

- [x] **P0 manual E2E:** the first dogfood path is now assembled and individually validated on development host: open `examples/hello_app.flux` through `tools/flux-nvim`, receive Flux syntax plus attached semantic LSP services, run it via `:FluxRun`/`flux run`, get the native GTK4/Wayland window, click the typed state-transition button, and rely on the existing save-triggered controlled-restart loop. The canonical manual sequence and recovery checks live in `docs/manual-e2e.md`; state-preserving in-process Fast Refresh remains explicitly beyond this minimal P0 gate.
  - [x] Compiler-backed flat `view`/grid syntax with typed built-in UI properties and composed views.
  - [x] `flux lsp` foundation (plus `fluxc` compatibility) with diagnostics, semantic highlighting, completion, hover, signature help, navigation/refactoring, formatting, fixes, inlay hints, and UI syntax awareness.
  - [x] `flux run` automatic save detection with rebuild and safe controlled restart on successful recompilation.
  - [x] Minimal application entry/runtime that opens a real native Linux window from a Flux program: bootstrap `app ViewName` selects a zero-parameter root view and replaces `fn main`; the Linux bootstrap lowers it to a GTK4 application/window while keeping GTK objects out of Flux source.
  - [x] Bootstrap native lowering/render path for the existing flat grid: explicit rows/columns/spans/gap lower to `GtkGrid`, fixed tracks feed native size requests, and `fr` tracks expand in the corresponding axis. A future lower-level renderer may replace GTK without changing Flux grid source.
  - [x] Render usable `Text` and `Button` primitives with native GTK4 controls, typed literal/constant/state-derived properties, spacing/margins, and a checked-in `examples/hello_app.flux` dogfood app.
  - [x] Pointer/click dispatch from the native Wayland window to a typed Flux `Button.on_press` callback: the development host dogfood launch produced the Flux callback output from real button activations.
  - [x] Minimal functional local UI state and invalidation/repaint: explicit `state clicked: bool = false` belongs to the view instance, `on_press: clicked => !clicked` is a typed state transformation, state-dependent Text/Button properties lower to a native refresh function, and the development host Wayland dogfood test toggled the app render and then restored it pixel-identically on the second activation (apart from unrelated desktop-panel changes).
  - [x] Editor integration/setup for the first dogfood editor: checked-in dependency-free Neovim runtime detects `.flux`, supplies immediate syntax colouring, launches `flux lsp` for semantic highlighting/full editor services, and exposes `:FluxRun`; `tools/flux-nvim` starts this experience without modifying global editor configuration.
  - [x] Checked-in `examples/hello_app.flux` manual acceptance app plus `docs/manual-e2e.md` documents the editor → compile/run → native window → click/state update → save-triggered rebuild/restart path, including failure/recovery expectations and non-interactive preflight checks.

When choosing work, pull the smallest prerequisites from later milestones forward as needed to finish this P0 path. Do not expand those prerequisites into their full eventual subsystem before the manual E2E is working.

## Non-negotiable language/product principles

- [x] Python-like readability with low ceremony.
- [x] Function bodies use `{}`.
- [x] Control-flow blocks use indentation.
- [x] Strict static typing.
- [x] No source-language generics.
- [x] No `try` / `catch` / exceptions for recoverable failures.
- [x] Explicit multi-value returns and `error` values form the recoverable-error model.
- [x] Functions, data, and interfaces are the primary abstraction model.
- [x] No classes or inheritance in Flux.
- [x] No mixins or class-oriented extension hierarchy.
- [x] No widget/controller/view-model object hierarchies in UI code.
- [x] Immutable values by default; local mutation is explicit and locally visible through typed `var` bindings and assignment; ordinary `let` bindings, parameters, destructured bindings, and loop variables remain immutable.
- [ ] Rust-class memory-safety goals without requiring Rust-level annotation density in normal application code.
- [ ] Native AOT output for application and server targets.
- [ ] Portable source should lower to real platform implementations rather than a browser/VM/widget-emulation wrapper.
- [ ] The LSP, debugger, profiler, formatter, test runner, package tooling, and hot reload are first-class parts of the language product.

## Current compiler baseline

Implemented on `main`:

- [x] Rust bootstrap compiler.
- [x] Functions and explicit return types.
- [x] Primitive `i64`, `bool`, `str`, `error`, and `void` types.
- [x] Strict typed bindings.
- [x] Expressions and function calls.
- [x] Indentation-based `if` / `elif` / `else`.
- [x] Exclusive integer range `for` loops.
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

Milestone 1 is complete. Parser recovery synchronizes at function boundaries, type checking batches independent errors while suppressing misleading cascades, diagnostics carry stable source identity plus rich JSON metadata, and all declaration/control-flow/expression syntax retains precise spans. Editor consumers can reuse parse snapshots and query one analyzed semantic database for symbols/signatures. The canonical formatter preserves comments, normalizes indentation/expression spacing, is idempotent, and supports CI check mode. Development can now move into the functional language core.

- [x] Multi-diagnostic parser recovery instead of stopping at the first syntax error.
- [x] Multi-diagnostic type checking where safe to continue.
- [x] Stable file/source identifiers in spans, not only line/column.
- [x] Full token ranges for declarations, bindings, types, and control-flow constructs.
- [x] Diagnostic labels/notes/fix suggestions.
- [x] Machine-readable diagnostic output.
- [x] Human terminal diagnostics render source excerpts, precise labels/fixes, ANSI color when appropriate, and width-aware wrapping/cropping without changing JSON diagnostics.
- [x] Incremental parse snapshot API suitable for editor use, with stable source identity, source fingerprints, unchanged-source reuse, and reparsing on edits.
- [x] Semantic query database shared by compiler and LSP, indexing function/parameter/binding/loop symbols and analyzed signatures from one typed program.
- [x] Deterministic comment-preserving formatter and canonical Flux formatting rules, exposed through `fluxc format` and `fluxc format --check`.

# Milestone 2 — Functional language core

Flux should be capable of modelling substantial programs without object-oriented machinery.

## Data and algebraic types

- [x] `struct` value types with named fields, nested acyclic value layout, native C lowering, formatter support, and semantic indexing.
- [x] Struct literals, field access, and functional update/copy syntax with single evaluation of the base value.
- [x] Closed enums / tagged unions with typed positional payloads, namespace-qualified constructors, semantic indexing, deterministic native tagged-union lowering, and cross-value-type cycle diagnostics.
- [x] Exhaustive `match` expressions and statements.
  - [x] Exhaustive enum `match` statements with typed payload bindings, duplicate/missing-arm diagnostics, guaranteed-return analysis, formatter/semantic support, single scrutinee evaluation, and native `switch` lowering.
  - [x] Value-producing enum `match` expressions for typed bindings and returns, using indentation-based arms, exhaustive/type-consistent results, pattern bindings/struct patterns, formatter/semantic support, and native `switch` lowering.
- [ ] Pattern matching over structs, enums, lists, and multi-values.
  - [x] Enum variant payload patterns in `match` statements, including `_` ignored payload positions.
  - [x] Flat struct destructuring patterns with shorthand fields, renaming, ignored bindings, inferred field types, aliases, and single source-expression evaluation.
  - [x] Nested struct destructuring patterns with recursive type validation, formatter/semantic support, and single root-expression evaluation.
  - [x] Struct patterns inside enum `match` arms, including nested struct patterns, static payload-type validation, formatter/semantic support, and direct native field projection.
  - [ ] List/map/record patterns after those value types exist.
  - [ ] Multi-value patterns outside call destructuring.
- [x] Destructuring bindings beyond multi-return calls: struct value destructuring is supported; broader nested/list/record patterns remain tracked above.
- [x] Transparent concrete type aliases with forward/chained resolution, cycle diagnostics, formatter/tooling support, and zero runtime representation.
- [x] Compile-time constants with forward references, constant folding, cycle diagnostics, alias-aware typing, and zero-runtime-storage lowering for `i64`, `bool`, and `str`.

## Control flow

- [x] `break` / `continue` in `for` loops with static scope validation through nested `if` / `match` blocks and native lowering.
- [x] Indentation-based `while` loops with statically boolean conditions, loop-scoped `break` / `continue`, and native lowering; useful mutation is explicit through typed `var` bindings rather than making ordinary bindings mutable.
- [x] Value-producing conditional/match expressions for functional code.
  - [x] Value-producing `match` expressions.
  - [x] Python-style value-producing conditional expressions (`a if condition else b`) with strict branch typing and native conditional lowering.

## Interfaces and capabilities

The interface track is now the active Milestone 2 subsystem. Declarations come first so type checking, semantic tooling, and later implementation/dispatch rules share one contract model; declaring an interface must not create an object hierarchy or runtime allocation by itself.

- [x] `interface` declarations consisting of statically validated function signatures/capabilities, with named-only parameter metadata, formatter/semantic support, namespace diagnostics, and zero-runtime lowering until implementation/dispatch semantics are introduced.
- [x] Explicit implementation of interfaces by concrete data types without inheritance, using exhaustive capability-to-free-function mappings with concrete first-parameter receiver validation, exact named-parameter/return contracts, formatter/semantic support, and zero-runtime conformance metadata. Module implementations remain naturally downstream of modules/packages.
- [x] Static interface dispatch where the concrete receiver type is known, using `Interface.capability(receiver, ...)` syntax and compile-time lowering directly to the mapped free function, including named arguments and multi-value returns with no vtable/dynamic lookup.
- [x] Efficient bootstrap interface/dynamic dispatch for runtime polymorphism: interface values are closed-world tagged by-value unions and `Interface.capability(interface_value, ...)` lowers to a tag switch calling the mapped free function, with no heap allocation, vtable, reflection, or object identity. Cross-module/stable ABI refinement remains part of later IR/ABI work.
- [x] Interface values usable as function arguments and return values via explicit `Interface(concrete)` packing; bindings, conditional selection, scalar returns, named capability arguments, and multi-value dynamic dispatch are supported. Struct/enum embedding remains intentionally blocked until ownership/layout ABI rules mature.
- [x] Interface composition without class inheritance using `interface Child: ParentA, ParentB { ... }`; compatible capabilities flatten into one contract for implementation/static/dynamic dispatch, while unknown parents, cycles, duplicate parents, and conflicting inherited signatures are rejected.
- [ ] Platform capabilities expressed through interfaces so portable code does not import Android/iOS/etc APIs directly. Blocked on modules/imports plus the first real platform backend; do not invent placeholder platform APIs before those exist.

## Functions

- [x] Named functions as first-class values: typed function references can be stored, passed, returned, and invoked through native function-pointer lowering.
- [ ] Closures with safe capture semantics.
- [ ] Anonymous functions.
- [x] Concise single-expression function syntax using `fn name(args) -> type { expression }`, preserving brace-delimited bodies while lowering through the ordinary implicit return/type-check/native ABI path, including exact multi-return call forwarding.
- [x] Named-only parameters using a `*` delimiter, with statically validated named call arguments and native ABI reordering.
- [x] Optional positional/named parameters with explicit compile-time primitive defaults, including top-level constant references and call-site default expansion.
- [x] Required named parameters: named-only parameters without defaults must be supplied explicitly.
- [x] Function type aliases through concrete `type Mapper = fn(...) -> ...` aliases; first-class function values currently support zero or one return value.
- [ ] Partial application / ergonomic function binding if it remains predictable and allocation-free where possible.
- [ ] Pipeline/composition syntax only if it materially improves functional code and lowers transparently.

## Modules/packages

- [x] Modules and imports using explicit relative `.flux` paths, deterministic recursive loading, duplicate suppression, preserved per-file source IDs, transitive merged semantic/type checking, formatter support, and project-aware CLI check/build/emit-C. Public cross-module declarations are reachable only through the importing module's explicit transitive forward import graph; loading two sibling modules into one project no longer leaks their public symbols to each other.
- [x] Explicit public/private visibility: top-level functions, constants, aliases, structs, enums, and interfaces are module-private by default; `pub` exports cross-file API, public signatures cannot leak private named types, formatter/source spans preserve visibility, and imports/implementation blocks cannot be `pub`.
- [x] Package manifest (`flux.toml`) with dependency-free `[package]` parsing, required `name`/`entry`, optional `version`, strict schema diagnostics, validated package-root containment, checked-in native example coverage, and project-aware CLI targeting by source file, package directory, or manifest path.
- [x] Stable module naming rules: imports use normalized relative `.flux` paths without `.`/`..` segments, package imports cannot escape the manifest root, and every loaded source receives a deterministic package-qualified module name derived from `[package].name` plus its root-relative source path (direct source projects use the entry-directory-relative path).
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

- [ ] `value[index]` indexing with strict index/key typing.
- [ ] Optional indexing (`value?[index]` or Flux-equivalent) for optional receivers.
- [ ] `first` and `last` conveniences for built-in sequences.
- [ ] `single` / exactly-one validation.
- [ ] `length` and `is_empty` / `is_not_empty` conveniences.
- [ ] Safe first/last forms for possibly empty collections instead of hidden runtime traps.
- [ ] Slices/ranges with clear bounds semantics.
- [ ] Indexed iteration without manual counter plumbing.
- [ ] `map`, `filter`/`where`, `fold`, `reduce`, `any`, `every`, `take`, `skip`, and related functional sequence operations.
- [ ] Lazy iteration where it avoids needless allocations.

`first`, `last`, etc. may use property-like syntax on compiler-known collection types while lowering to ordinary functions. This is syntax ergonomics, not an object model.

## Cascades / fluent mutation without builder objects

Dart's `..` cascade is useful because it avoids repeating a target. Flux should keep the ergonomic idea but apply it to values/functions rather than encouraging mutable object builders.

- [ ] Cascade syntax analogous to `..` for repeated operations on one target.
- [ ] Optional-aware cascade analogous to `?..`.
- [ ] Define whether cascades mutate an explicitly mutable value or produce successive transformed values.
- [ ] Prefer a functional transformation form when it preserves clarity and optimization.
- [ ] Ensure cascade expressions evaluate the target exactly once.

## Collection literals and control flow

- [ ] List/array literals.
- [ ] Set literals.
- [ ] Map literals.
- [ ] Spread elements analogous to Dart `...`.
- [ ] Optional-aware spread analogous to Dart `...?`.
- [ ] Null/optional-aware collection elements analogous to modern Dart `?element`.
- [ ] `if` / `else` elements inside collection literals.
- [ ] `for` elements/comprehensions inside collection literals.
- [ ] Pattern guards inside collection construction where useful.
- [ ] Compile these forms directly without constructing intermediate collections unnecessarily.

## Records, patterns, and destructuring

Flux intentionally keeps function multi-return values narrow today. General records should be added only when they do not blur explicit multi-return/error semantics.

- [ ] Anonymous immutable records for lightweight aggregate data.
- [ ] Named record fields.
- [ ] Positional record fields.
- [ ] Record destructuring patterns.
- [ ] List patterns.
- [ ] Map patterns.
- [x] Struct patterns for bindings and enum match payloads; list/map/record pattern forms remain pending with their value types.
- [ ] Enum/tagged-union patterns.
- [ ] Relational/logical patterns where readable.
- [ ] Pattern guards.
- [ ] Exhaustiveness checking.
- [ ] Pattern-based variable declarations/assignments.

## Expression ergonomics

- [ ] String interpolation. Runtime interpolation is sequenced after owned-string/buffer semantics: the bootstrap `str` currently lowers to borrowed native string pointers, so Flux must not introduce hidden scratch-buffer allocation/lifetime behavior merely for interpolation syntax. Compile-time interpolation may arrive earlier only if it shares the eventual syntax and semantics cleanly.
- [ ] Multiline strings.
- [ ] Raw strings.
- [x] Python-style conditional expression (`a if condition else b`) as the Flux-native value conditional.
- [ ] Integer truncating division equivalent to Dart `~/` if useful.
- [ ] Concise range syntax beyond current `start..end` where appropriate.
- [x] Match expressions returning values; broader switch-like pattern expression ergonomics can build on the same lowering.
- [ ] Assignment expressions only where they remain readable and analyzable. Ordinary assignment is now a statement restricted to explicitly mutable `var` bindings; do not conflate that with value-producing assignment expressions.
- [ ] Strong constant folding for literal/expression-heavy syntax.

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

- [ ] Define copyable primitive/value types.
- [ ] Move semantics for non-copy values.
- [ ] Immutable borrows.
- [ ] Exclusive mutable borrows.
- [ ] Lifetime inference.
- [ ] Prevent use-after-move.
- [ ] Prevent dangling borrows.
- [ ] Prevent aliasing violations.
- [ ] Deterministic destruction/resource release.
- [ ] Safe closures/captures under the ownership model.
- [ ] Thread-safety/sendability rules.
- [ ] Narrow, explicit `unsafe` escape hatch only where necessary.
- [ ] First borrow checker over normalized semantics/IR rather than parser syntax.

# Milestone 5 — Standard library and built-in concrete containers

Flux has no generics, so common containers need compiler-known/specialized implementations and ergonomic concrete typing.

- [ ] Owned string type plus immutable string views/slices.
- [ ] Arrays/lists.
- [ ] Maps.
- [ ] Sets.
- [ ] Byte buffers.
- [ ] Paths and filesystem APIs.
- [ ] Date/time/duration.
- [ ] JSON parsing/encoding.
- [ ] URI/URL support.
- [ ] HTTP primitives.
- [ ] TLS integration.
- [ ] WebSocket support.
- [ ] Cryptographic primitives through audited platform/native libraries.
- [ ] Process/environment APIs for desktop/server targets.
- [ ] Iteration protocol/interface.
- [ ] Efficient specialized collection implementations generated by compiler/stdlib tooling rather than source generics.

# Milestone 6 — Concurrency

- [ ] Task model.
- [ ] Structured concurrency.
- [ ] Cancellation propagation.
- [ ] Timers.
- [ ] Channels/message passing.
- [ ] Worker/thread execution.
- [ ] Data-race prevention integrated with ownership.
- [ ] Event-loop integration for UI targets.
- [ ] Server-oriented high-throughput I/O runtime.

# Milestone 7 — Typed Flux IR and optimizing native backend

- [ ] Typed Flux IR with source spans.
- [ ] Ownership facts represented in IR.
- [ ] Explicit control-flow graph.
- [ ] SSA or equivalent optimization-friendly value representation.
- [ ] Preserve debug locations through lowering.
- [ ] Constant folding.
- [ ] Dead-code elimination.
- [ ] Inlining.
- [ ] Escape analysis.
- [ ] Bounds-check elimination where proven safe.
- [ ] Devirtualization/interface specialization.
- [ ] Target-aware optimization pipeline.
- [ ] Native object generation through an LLVM-class backend initially.
- [ ] LTO.
- [ ] Tree shaking/dead symbol elimination.
- [ ] Remove bootstrap C as the semantic backend after parity is proven.

# Milestone 8 — Application model

- [ ] `app` declaration / application entry model.
  - [x] Bootstrap `app ViewName` process entry selects one visible zero-parameter root view, is formatter/LSP-keyword aware, rejects competing `fn main`, and lowers to the Linux GUI runtime. Metadata/arguments/multiple windows remain pending.
- [x] Bootstrap application metadata: `app ViewName(title: ..., width: ..., height: ...)` accepts compile-time typed values/constants, validates positive window dimensions, formats canonically, and lowers title/default size into the native Linux window. Package identity/icons/permissions remain in their dedicated later tracks rather than being hidden in this bootstrap surface.
- [ ] Assets/resources.
- [ ] Environment/configuration.
- [ ] Platform permissions.
- [ ] Lifecycle events as functions/capabilities, not class callbacks.
  - [x] Bootstrap `app` metadata accepts named typed `on_start: fn() -> void` and `on_exit: fn() -> void` callbacks; Linux invokes start during native activation and exit through the native application shutdown signal with no application/controller object exposed to Flux source.
  - [ ] Foreground/background, window lifecycle, suspension/restoration, and capability-rich lifecycle context.
- [ ] Background/foreground transitions.
- [ ] Multiple windows where the target supports them.
- [ ] Platform capability discovery.

# Milestone 9 — UI language and flat layout

The UI model must avoid Flutter-style deeply nested widget construction and object hierarchies. Sequencing note: the compiler-backed surface syntax is being prototyped early, before rendering backends, because the flat layout grammar is a core language constraint and should not be retrofitted after a widget-tree API takes shape.

- [ ] `view` declarations/functions.
  - [x] Bootstrap top-level `view Name { ... }` declarations with visibility, source spans, formatter support, project merging, and semantic symbols.
  - [x] Parameterized `view Name(data: Type, optional: Type = default) { ... }` declarations: parameters are statically typed, defaults use the ordinary constant-default rules, view bodies can reference parameters, and parameter symbols retain editor spans/types.
  - [ ] View-producing/function composition beyond declarative view parameters only if it adds value without creating a widget-builder API.
- [x] Views compose through typed data rather than subclassing: a declared Flux view can be used as another view's flat sibling element type, arguments are named properties checked against the target view's parameters/defaults, module visibility is enforced, and recursive composition cycles are rejected.
- [ ] Flat element declarations.
  - [x] Bootstrap sibling element syntax (`Type name at row,column`) with unique local names and optional property expressions; nested child elements are syntactically rejected.
- [ ] Grid-first layout.
  - [x] Bootstrap grid grammar with explicit columns/rows, logical fixed units, `fr` tracks, `auto`, gap, explicit padding, explicit row/column placement, and row/column spans. Rendering, constraints, and responsive behavior remain pending.
  - [ ] Statically type/check element properties against built-in or interface-defined element contracts instead of treating property expressions as untyped view metadata.
    - [x] Bootstrap built-in element contracts validate known property names and property expression types, including typed function callbacks, without introducing widget classes; semantic symbols expose the property type for editor tooling.
    - [x] Declared Flux views act as custom element contracts derived directly from their typed parameters/defaults; custom-view property types flow into semantic/editor symbols.
    - [ ] Interface-defined platform/native element contracts after platform capability rules are mature.
  - [x] Validate placement against declared track counts and define overlap policy: row/column spans that escape the declared grid are static errors, and ordinary sibling grid regions may not overlap. Intentional overlap belongs to the separate explicit overlay/absolute-positioning model.
  - [ ] Consider named grid areas only if they materially improve readability without reintroducing hierarchy.
- [ ] Flex/flow layout where grid is not appropriate.
- [ ] Absolute/overlay positioning when explicitly requested.
- [ ] Constraints, min/max sizing, intrinsic sizing.
- [ ] Responsive breakpoints/container queries.
- [ ] Text.
  - [x] Bootstrap Linux GTK4 `Text` lowering for literal/constant/state-derived `text: str`, `selectable: bool`, compile-time positive `size: i64`, and `bold: bool`, including grid placement, native wrapping/alignment, and Pango-backed typography without exposing GTK/Pango objects to Flux source.
- [ ] Rich text.
- [ ] Images/vector images.
- [ ] Buttons.
  - [x] Bootstrap Linux GTK4 `Button` lowering for compile-time `text: str` / `enabled: bool` plus typed named `fn() -> void` `on_press` dispatch.
- [ ] Text inputs.
- [ ] Toggles/check/radio controls.
  - [x] Bootstrap native `Toggle` control with typed `label: str`, `checked: bool`, `enabled: bool`, and `on_change` event; Linux lowers to `GtkCheckButton`, while `on_change: state => next_expression` reuses the same functional transition/state refresh model as buttons. Radio/group semantics remain pending.
- [ ] Lists/grids with virtualization.
- [ ] Scrolling.
- [ ] Overlays/popovers/tooltips.
- [ ] Dialogs/sheets.
- [ ] Menus/context menus.
- [ ] Native platform control mapping where beneficial without exposing platform classes to Flux code.

# Milestone 10 — Functional UI state and rendering

- [ ] Local view state without `StatefulWidget`-style objects.
  - [x] Bootstrap explicit primitive view state (`state name: type = compile_time_value`) is compiler/formatter/semantic metadata and the Linux root-view runtime stores `bool` and `i64` state per app instance without source-level state objects/controllers; owned/string/aggregate state remains sequenced behind the ownership model.
- [ ] Immutable state updates by default.
  - [x] Bootstrap UI events use a functional transition form (`on_press: state => next_expression`): the next state expression is strictly type-checked against the target state and evaluated before native property refresh; bool/i64 transitions support primitive unary/binary arithmetic, comparisons, logic, checked integer division, and conditionals.
- [ ] Derived/computed state.
  - [x] Bootstrap native Text/Button properties can derive values from bool/i64 local state through statically typed primitive expressions and conditionals, enabling counter/toggle-style UI without a view-model object.
- [ ] Explicit effects.
- [ ] Shared state/store primitives only where necessary.
- [ ] State ownership/lifetime rules integrated with the compiler.
- [ ] Fine-grained dependency tracking or equivalent efficient invalidation.
- [ ] Minimal re-render/re-layout work after state changes.
  - [x] Bootstrap GTK4 root-view refresh updates only state-dependent native Text/Button properties and does not reconstruct the window/grid; the dogfood toggle produced a localized render delta and the second toggle restored the app region to its original pixels.
- [ ] Stable element identity rules.
- [ ] No mandatory controller classes.
- [ ] No build-context object threaded through user code.

# Milestone 11 — Automatic save-triggered hot reload

Flux hot reload should feel closer to React development than Flutter's manual reload workflow.

- [ ] `flux run` watches the project automatically in development mode.
  - [x] Bootstrap `fluxc run` builds and launches a native development binary, then continuously watches the real project/import graph plus `flux.toml` where applicable.
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

- [ ] Pointer/mouse input.
- [ ] Touch input.
- [ ] Keyboard input.
- [ ] Focus model.
- [ ] Hover.
- [ ] Gestures.
- [ ] Drag and drop.
- [ ] Keyboard shortcuts.
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
  - [x] Bootstrap `Text.color: str` accepts compile-time `#RRGGBB` values, is statically typed/LSP-visible, and lowers to native Pango foreground attributes on Linux; dynamic/theme colors and broader element styling remain pending.
- [ ] Typography.
  - [x] Bootstrap native `Text.size: i64` and `Text.bold: bool` contracts lower to Pango attributes on Linux; invalid/non-positive sizes fail at compile/codegen time instead of silently rendering.
- [ ] Spacing.
  - [x] Flat-grid `grid padding: N` is source-level layout data and lowers to native root-grid margins; `grid gap` already lowers to native row/column spacing.
- [ ] Borders/radii.
- [ ] Shadows.
- [ ] Clipping.
- [ ] Transforms.
- [ ] Light/dark themes.
- [ ] Design tokens/variables.
- [ ] System theme integration.
- [ ] Device/window/orientation queries.
- [ ] Adaptive components where platform differences are valuable.
  - [x] Bootstrap `Button.primary: bool` maps an explicit compile-time primary action to the native GTK `suggested-action` style class without exposing platform widget APIs in Flux source.
- [ ] Animation timelines.
- [ ] Easing/springs.
- [ ] Property transitions.
- [ ] Layout transitions.
- [ ] Gesture-driven animation.
- [ ] Frame synchronization.
- [ ] Reduced-motion accessibility handling.

# Milestone 15 — Android target

Android is the first application-platform target.

- [ ] Android target triple/backend.
- [ ] `flux build android`.
- [ ] `flux run android`.
- [ ] APK output.
- [ ] AAB output.
- [ ] Debug/profile/release variants.
- [ ] Android manifest generation.
- [ ] Package/application ID configuration.
- [ ] minSdk/targetSdk configuration.
- [ ] Signing configuration.
- [ ] Android lifecycle integration.
- [ ] Activity/window integration hidden behind Flux application APIs.
- [ ] Native rendering/input backend.
- [ ] Accessibility/semantics bridge.
- [ ] Notifications.
- [ ] Intents/deep links.
- [ ] Share sheet.
- [ ] Permissions.
- [ ] Background services/tasks where allowed.
- [ ] Camera.
- [ ] Microphone.
- [ ] Media playback.
- [ ] Secure storage.
- [ ] Files/media picker.
- [ ] Android keyboard/IME.
- [ ] Platform FFI/bindings to Kotlin/Java/NDK where needed.
- [ ] Play Store-ready release pipeline.
- [ ] First real production-style Flux Android app built and tested on physical hardware.

# Milestone 16 — Desktop targets

## Linux

- [ ] Native Linux window/render/input backend.
  - [x] Bootstrap GTK4 backend opens a real native Linux window, lowers Flux flat grid/Text/Button to native controls, and dispatches native button clicks into Flux callbacks.
- [ ] Wayland-first support with appropriate fallback where practical.
  - [x] Bootstrap GUI was built and launched end-to-end on development host against its real `wayland-0` compositor (`GDK_BACKEND=wayland`); GTK4 supplies the native Wayland integration while the eventual lower-level backend remains open.
- [ ] Menus/tray/clipboard/file dialogs.
- [ ] Multi-window.
- [ ] Packaging formats/integration guidance.

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
- [ ] Android Kotlin/Java/NDK bindings.
- [ ] Apple Swift/Objective-C bindings.
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

- [ ] Semantic roles.
- [ ] Accessible labels/descriptions.
- [ ] Actions.
- [ ] Focus ordering.
- [ ] Screen-reader bridges.
- [ ] Text scaling.
- [ ] High contrast.
- [ ] Reduced motion.
- [ ] Keyboard-only navigation.
- [ ] Locale detection.
- [ ] Translation resources.
- [ ] Plural/select rules.
- [ ] Number/date/currency formatting.
- [ ] RTL layout.
- [ ] Locale-aware text/input behavior.

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
- [ ] Dependency/interface fakes based on functions/interfaces rather than mocking class hierarchies.
- [ ] Coverage.
- [ ] Benchmarks.
- [ ] Compiler self-tests across supported targets.

# Milestone 23 — LSP and editor experience

- [x] Diagnostics reuse the same structured diagnostic model and source spans as the terminal renderer, including open-buffer project overlays; there is no separate editor-only error model.
  - [x] Bootstrap `fluxc lsp` speaks stdio JSON-RPC/LSP, negotiates UTF-8/UTF-16 positions, uses full-document `didOpen`/`didChange`/`didClose` synchronization, and publishes the compiler's existing parse/type diagnostics for open standalone buffers including related labels and structured fix data.
  - [x] Workspace/import-aware unsaved-buffer diagnostics: the project loader accepts in-memory source overlays keyed by canonical path, and `fluxc lsp` type-checks imported open documents against the real import graph while substituting every currently open unsaved Flux buffer. Diagnostics are republished across open dependent documents after overlay changes and filtered to each document's stable source ID.
- [ ] Completion.
  - [x] Bootstrap `textDocument/completion` provides Flux keywords, builtin types/functions, current-module declarations, public declarations from the forward import graph, and position-aware parameters/local bindings declared earlier in the current function, including unsaved overlay signatures/details. Local suggestions are filtered by active indentation scope so completed branches/loops do not leak bindings.
  - [x] Qualified namespace completion for enum variants and interface capabilities works at incomplete member edits such as `Outcome.` / `Storage.`, including explicitly imported declarations. A tolerant probe preserves declarations/imports before the active broken top-level declaration rather than requiring the edit to parse first.
  - [x] Struct-field completion for explicitly typed visible parameters/`let`/`var` values, including transparent struct aliases, remains available at incomplete `value.` edits and reuses loaded local/imported struct contracts. Richer member completion for arbitrary value-producing expressions remains a later refinement.
- [x] Signature help for every current parenthesized Flux call form: ordinary/imported functions, first-class concrete `fn(...) -> ...` values, namespace-qualified interface capabilities with an explicit receiver, enum payload constructors, interface packing, and compiler built-ins (`print`/`error`). Active arguments are tracked across nested calls. Declarative view composition is property-oriented rather than a call surface and remains covered by UI completion/hover.
- [ ] Hover/type information.
  - [x] Bootstrap `textDocument/hover` reports exact declaration types/signatures and safely resolves unambiguous usage-site hover across the loaded forward import graph, including unsaved overlays. Scope-resolved local shadowing and reverse-workspace discovery remain pending with richer reference indexing.
- [ ] Go to definition.
  - [x] Bootstrap `textDocument/definition` navigation uses the shared semantic database and resolves declarations/usages only when the target name is unambiguous; when the current file imports modules, typed overlay project analysis resolves public imported declarations to their actual source-file URI/range. Scope-aware shadow resolution and reverse-workspace discovery remain pending.
- [ ] Find references.
  - [x] Bootstrap `textDocument/references` returns identifier occurrences only after unambiguous semantic target resolution; strings/comments are excluded. From an importing document, references cover the entire typed loaded dependency graph, including imported declarations and unsaved overlays. Reverse dependents outside that graph still require workspace indexing.
- [ ] Rename.
  - [x] Bootstrap `textDocument/rename` emits workspace edits only for an unambiguous semantic target and a valid non-keyword Flux identifier; from an importing document it safely edits every occurrence in the typed loaded dependency graph and rejects collisions with existing semantic symbols. Ambiguous/shadowed names are refused rather than edited unsafely; reverse dependents outside the loaded graph remain pending.
- [x] Semantic highlighting through full-document LSP semantic tokens: Flux keywords, strings, numbers, comments, operators, builtin types/functions, and semantically resolved declarations/unambiguous usages receive standard token categories with negotiated UTF-8/UTF-16 positions.
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

- [ ] Source-level breakpoints.
- [ ] Stepping.
- [ ] Stack frames.
- [ ] Locals.
- [ ] Watches.
- [ ] Expression evaluation.
- [ ] Async/task debugging.
- [ ] Ownership-aware value inspection.
- [ ] Native debug metadata.
- [x] Predictable bootstrap debug/profile/release native build behavior: `debug` uses no optimization plus full debug info/frame pointers, `profile` uses optimized code with debug info/frame pointers, and `release` uses aggressive optimization/LTO. Source-level Flux debug-location mapping remains tracked separately under native debug metadata.
- [ ] Platform debugger integration where useful.

# Milestone 25 — Profiler and devtools

- [ ] CPU profiling.
- [ ] Allocation profiling.
- [ ] Memory graph/leak tooling.
- [ ] Task/async timeline.
- [ ] Frame/render timeline.
- [ ] Layout diagnostics.
- [ ] Network inspection.
- [ ] Native symbolization.
- [ ] Source mapping.
- [ ] UI inspector without exposing a widget-object hierarchy.
- [ ] Performance overlays.
- [ ] Low-overhead production profiling hooks.

# Milestone 26 — CLI/build/developer workflow

- [x] `flux new`: the user-facing `flux` binary shares the canonical CLI implementation and scaffolds a minimal manifest-backed native GUI package without overwriting non-empty directories.
- [x] `flux run`: the user-facing command provides the automatic development watch/build/restart loop and hot-reload status contract.
- [x] Bootstrap `fluxc build` compatibility entry remains available while the product CLI transitions to `flux`.
- [x] `flux build`: user-facing native builds support debug/profile/release modes and package/source targets.
- [x] `flux test`: user-facing native integration-test discovery/execution for direct `.flux` tests and package `tests/*.flux`, with debug/profile/release mode selection and real exit-status reporting.
- [x] `flux format`: user-facing canonical formatting and `--check` mode use the existing formatter implementation.
- [x] `flux analyze`: user-facing static analysis is an explicit alias over the same project-aware compiler diagnostics as `flux check`, including `--json` for tooling/CI.
- [ ] `flux devices`.
- [x] `flux doctor`: bootstrap Linux doctor reports host support, Clang, GTK4, optional Neovim dogfood availability, and the active Wayland/X11 display before native app testing.
- [ ] `flux clean`.
- [ ] `flux package`.
- [x] Debug/profile/release modes through `flux build --mode ...` and `flux run --mode ...` (with `fluxc` compatibility); builds default to `release`, while the development runner defaults to `debug` for fast rebuilds and native debug information.
- [ ] Cross-compilation configuration.
- [ ] Build cache.
- [ ] Reproducible builds.
- [x] CI-friendly machine output through `fluxc check --json`, preserving a clean stdout JSON contract with no human diagnostic text on stderr.

# Milestone 27 — Distribution

- [ ] Android signing/store publishing guidance and automation hooks.
- [ ] iOS signing/App Store workflow.
- [ ] Windows packaging/store workflow.
- [ ] macOS signing/notarization/store workflow.
- [ ] Linux packaging guidance.
- [ ] Web production deployment.
- [ ] Debug symbol separation/upload.
- [ ] Crash symbolization.
- [ ] Optional obfuscation where compatible with diagnostics/debugging.
- [ ] Version/build-number management.

# Milestone 28 — Server/headless application target

Flux must remain equally credible for headless native binaries.

- [ ] HTTP server.
- [ ] TLS server.
- [ ] WebSocket server.
- [ ] TCP/UDP sockets.
- [ ] Signals/process lifecycle.
- [ ] Service/daemon integration.
- [ ] Structured concurrency for request handling.
- [ ] Efficient I/O.
- [ ] Database connectivity.
- [ ] Static/self-contained deployment where platform permits.
- [ ] Container-friendly builds.
- [ ] Profiling/observability hooks.

# Milestone 29 — Flux 1.0 stability

- [ ] Stable grammar.
- [ ] Stable type/memory model.
- [ ] Stable package format.
- [ ] Stable native/platform ABI policy.
- [ ] Stable UI API compatibility policy.
- [ ] Stable formatter.
- [ ] Stable diagnostics contract for tooling.
- [ ] Android, iOS, Windows, macOS, Linux, web, and headless/server targets meet documented support tiers.
- [ ] Production-quality LSP/debugger/profiler.
- [ ] Automatic save-triggered hot reload is production-ready for development workflows.
- [ ] Package ecosystem and platform interoperability are documented and tested.
- [ ] Flux can build substantial real applications without resorting to an OOP architecture.
