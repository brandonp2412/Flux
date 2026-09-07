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
- [ ] Immutable values by default; mutation is explicit and locally visible.
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
- [x] Incremental parse snapshot API suitable for editor use, with stable source identity, source fingerprints, unchanged-source reuse, and reparsing on edits.
- [x] Semantic query database shared by compiler and LSP, indexing function/parameter/binding/loop symbols and analyzed signatures from one typed program.
- [x] Deterministic comment-preserving formatter and canonical Flux formatting rules, exposed through `fluxc format` and `fluxc format --check`.

# Milestone 2 — Functional language core

Flux should be capable of modelling substantial programs without object-oriented machinery.

## Data and algebraic types

- [x] `struct` value types with named fields, nested acyclic value layout, native C lowering, formatter support, and semantic indexing.
- [x] Struct literals, field access, and functional update/copy syntax with single evaluation of the base value.
- [x] Closed enums / tagged unions with typed positional payloads, namespace-qualified constructors, semantic indexing, deterministic native tagged-union lowering, and cross-value-type cycle diagnostics.
- [ ] Exhaustive `match` expressions and statements.
  - [x] Exhaustive enum `match` statements with typed payload bindings, duplicate/missing-arm diagnostics, guaranteed-return analysis, formatter/semantic support, single scrutinee evaluation, and native `switch` lowering.
  - [ ] `match` expressions that produce a value.
- [ ] Pattern matching over structs, enums, lists, and multi-values.
  - [x] Enum variant payload patterns in `match` statements, including `_` ignored payload positions.
  - [x] Flat struct destructuring patterns with shorthand fields, renaming, ignored bindings, inferred field types, aliases, and single source-expression evaluation.
  - [ ] Nested struct patterns and struct patterns inside `match` arms.
  - [ ] List/map/record patterns after those value types exist.
  - [ ] Multi-value patterns outside call destructuring.
- [x] Destructuring bindings beyond multi-return calls: struct value destructuring is supported; broader nested/list/record patterns remain tracked above.
- [x] Transparent concrete type aliases with forward/chained resolution, cycle diagnostics, formatter/tooling support, and zero runtime representation.
- [x] Compile-time constants with forward references, constant folding, cycle diagnostics, alias-aware typing, and zero-runtime-storage lowering for `i64`, `bool`, and `str`.

## Interfaces and capabilities

- [ ] `interface` declarations consisting of function signatures/capabilities.
- [ ] Explicit implementation of interfaces by concrete data types/modules without inheritance.
- [ ] Static dispatch where the concrete type is known.
- [ ] Efficient interface/dynamic dispatch where runtime polymorphism is required.
- [ ] Interface values usable as function arguments and return values.
- [ ] Interface composition without class inheritance.
- [ ] Platform capabilities expressed through interfaces so portable code does not import Android/iOS/etc APIs directly.

## Functions

- [ ] Functions as first-class values.
- [ ] Closures with safe capture semantics.
- [ ] Anonymous functions.
- [ ] Concise single-expression function syntax.
- [ ] Named parameters.
- [ ] Optional parameters with explicit defaults.
- [ ] Required named parameters.
- [ ] Function type aliases.
- [ ] Partial application / ergonomic function binding if it remains predictable and allocation-free where possible.
- [ ] Pipeline/composition syntax only if it materially improves functional code and lowers transparently.

## Modules/packages

- [ ] Modules and imports.
- [ ] Explicit public/private visibility.
- [ ] Package manifest (`flux.toml`).
- [ ] Stable module naming rules.
- [ ] Cyclic dependency diagnostics.
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
- [ ] Struct patterns.
- [ ] Enum/tagged-union patterns.
- [ ] Relational/logical patterns where readable.
- [ ] Pattern guards.
- [ ] Exhaustiveness checking.
- [ ] Pattern-based variable declarations/assignments.

## Expression ergonomics

- [ ] String interpolation.
- [ ] Multiline strings.
- [ ] Raw strings.
- [ ] Conditional expression (`condition ? a : b`) or a clearer Flux-native equivalent.
- [ ] Integer truncating division equivalent to Dart `~/` if useful.
- [ ] Concise range syntax beyond current `start..end` where appropriate.
- [ ] Switch/match expressions returning values.
- [ ] Assignment expressions only where they remain readable and analyzable.
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
- [ ] Application metadata.
- [ ] Assets/resources.
- [ ] Environment/configuration.
- [ ] Platform permissions.
- [ ] Lifecycle events as functions/capabilities, not class callbacks.
- [ ] Background/foreground transitions.
- [ ] Multiple windows where the target supports them.
- [ ] Platform capability discovery.

# Milestone 9 — UI language and flat layout

The UI model must avoid Flutter-style deeply nested widget construction and object hierarchies.

- [ ] `view` declarations/functions.
- [ ] Views compose through functions/data rather than subclassing.
- [ ] Flat element declarations.
- [ ] Grid-first layout.
- [ ] Flex/flow layout where grid is not appropriate.
- [ ] Absolute/overlay positioning when explicitly requested.
- [ ] Constraints, min/max sizing, intrinsic sizing.
- [ ] Responsive breakpoints/container queries.
- [ ] Text.
- [ ] Rich text.
- [ ] Images/vector images.
- [ ] Buttons.
- [ ] Text inputs.
- [ ] Toggles/check/radio controls.
- [ ] Lists/grids with virtualization.
- [ ] Scrolling.
- [ ] Overlays/popovers/tooltips.
- [ ] Dialogs/sheets.
- [ ] Menus/context menus.
- [ ] Native platform control mapping where beneficial without exposing platform classes to Flux code.

# Milestone 10 — Functional UI state and rendering

- [ ] Local view state without `StatefulWidget`-style objects.
- [ ] Immutable state updates by default.
- [ ] Derived/computed state.
- [ ] Explicit effects.
- [ ] Shared state/store primitives only where necessary.
- [ ] State ownership/lifetime rules integrated with the compiler.
- [ ] Fine-grained dependency tracking or equivalent efficient invalidation.
- [ ] Minimal re-render/re-layout work after state changes.
- [ ] Stable element identity rules.
- [ ] No mandatory controller classes.
- [ ] No build-context object threaded through user code.

# Milestone 11 — Automatic save-triggered hot reload

Flux hot reload should feel closer to React development than Flutter's manual reload workflow.

- [ ] `flux run` watches the project automatically in development mode.
- [ ] Saving a source file automatically triggers incremental recompilation.
- [ ] No hot-reload hotkey is required for the normal workflow.
- [ ] Apply compatible function/view changes to the running process automatically.
- [ ] Preserve compatible application/UI state across reloads.
- [ ] Fast Refresh-style boundary detection: reset only state that is no longer structurally compatible.
- [ ] Automatically fall back to a controlled restart when a change cannot be safely hot-applied.
- [ ] Surface reload/restart reason in tooling without interrupting normal development.
- [ ] Debounce/coalesce rapid editor saves.
- [ ] Watch dependency/module changes.
- [ ] Stable development ABI/function indirection strategy.
- [ ] Incremental compiler cache.
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
- [ ] Typography.
- [ ] Spacing.
- [ ] Borders/radii.
- [ ] Shadows.
- [ ] Clipping.
- [ ] Transforms.
- [ ] Light/dark themes.
- [ ] Design tokens/variables.
- [ ] System theme integration.
- [ ] Device/window/orientation queries.
- [ ] Adaptive components where platform differences are valuable.
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
- [ ] Wayland-first support with appropriate fallback where practical.
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

- [ ] `flux test`.
- [ ] Unit tests.
- [ ] Integration tests.
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

- [ ] Diagnostics.
- [ ] Completion.
- [ ] Signature help.
- [ ] Hover/type information.
- [ ] Go to definition.
- [ ] Find references.
- [ ] Rename.
- [ ] Semantic highlighting.
- [ ] Formatting.
- [ ] Code actions/fixes.
- [ ] Inlay hints where they add value.
- [ ] Incremental workspace analysis.
- [ ] Package/module awareness.
- [ ] UI syntax awareness.
- [ ] Hot-reload status integration.

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
- [ ] Predictable debug/profile/release behavior.
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

- [ ] `flux new`.
- [ ] `flux run`.
- [x] Bootstrap `fluxc build` equivalent exists through Cargo invocation.
- [ ] `flux build`.
- [ ] `flux test`.
- [ ] `flux format`.
- [ ] `flux analyze`.
- [ ] `flux devices`.
- [ ] `flux doctor`.
- [ ] `flux clean`.
- [ ] `flux package`.
- [ ] Debug/profile/release modes.
- [ ] Cross-compilation configuration.
- [ ] Build cache.
- [ ] Reproducible builds.
- [ ] CI-friendly machine output.

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
