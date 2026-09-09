# Flux

Flux is an experimental compiled language for building native applications and services from one codebase without a widget-emulation runtime. The long-term target is maximally optimized platform-native binaries for desktop, server, and other supported targets.

## Language direction

- Python-like readability and low ceremony.
- Function bodies are delimited by `{}`.
- Control-flow bodies use indentation (`if`, `for`, etc.).
- Strict static typing; implicit type coercions are deliberately minimized, unused bindings are compile errors, and Flux has no warning-only lint tier.
- Flux source naming is lower camelCase for values, functions, built-ins, properties, events, and environment bindings (`requestCount`, `onPress`, `windowHeight`). Type-like names remain PascalCase. Tooling emits camelCase only; ordinary user-defined identifiers may still parse in legacy snake_case for compatibility, but language-owned names use the camelCase spelling.
- No comment syntax and no ternary/conditional expression syntax.
- Function-first architecture built from data, functions, and interfaces rather than classes, inheritance, mixins, or widget/controller object hierarchies.
- No generics in the Flux language.
- No exception / try-catch model. Recoverable failures are represented explicitly in return values.
- Rust-like memory-safety goals: ownership, borrowing, lifetime validation, and no unchecked dangling references in safe Flux.
- Native ahead-of-time compilation.
- A flat, HTML-like declarative UI surface with grid-first layout rather than deeply nested widget trees; the bootstrap parser now supports top-level `view` declarations with sibling grid placement syntax.
- Tooling is part of the language product: LSP, debugger, and profiler are first-class deliverables.

## Current bootstrap milestone

The repository currently contains a dependency-free Rust bootstrap compiler with:

- parsing for functions, structs, closed payload enums, typed bindings, calls, field access, `if` / `elif` / `else`, exclusive `start..end` and inclusive `start..=end` integer range `for` loops, indentation-based `while` loops, statically scoped `break` / `continue`, and bootstrap `app ViewName` GUI entry declarations;
- immutable `let` bindings by default plus explicit typed local mutation through `var` and statically checked assignment;
- explicit multi-value function returns and strictly typed destructuring bindings;
- static checking for `i64`, `bool`, `str`, `error`, `void`, named struct value types, transparent concrete type aliases, and compile-time constants, with ordinary escaped, `r"..."` raw, and triple-quoted multiline string literals sharing the same `str` representation;
- struct literals, field access, `Type { ..base, field: value }` functional updates, and struct destructuring patterns with inferred field types and single-evaluation native lowering;
- namespace-qualified enum construction such as `Outcome.Ok(42)`, with typed payload validation and native tag/union representation;
- exhaustive enum `match` statements with typed payload bindings, guaranteed-return analysis, and single-evaluation native `switch` lowering;
- positional and named-only function parameters with required named arguments, compile-time defaults, and zero-runtime-overhead call reordering;
- first-class named function values and concrete `fn(...) -> ...` function types, lowered to typed native function pointers without callable objects;
- shell-inspired typed call flow: parentheses-free positional calls, `|` value pipelines, scalar-result `>`/`>>` file redirection, and trailing `&` detached execution without turning functions into untyped subprocess commands;
- concrete no-generics list syntax with homogeneous `T[]` literals, `...source` spread elements, list-only `if condition: value` / `if condition: first else: second` construction items, checked positive/negative indexing, full Python-style zero-copy slicing including steps and `[::-1]` reversal through native strided views, filtered list comprehensions, zero-cost `length` / `isEmpty` / `isNotEmpty` properties, checked `first` / `last` / `single` access, `any` / `every` boolean aggregation, typed `map` / `filter` / `where` transforms, typed `fold` / `reduce` scalar reductions, typed `concat`, stable scalar `distinct`, one-level nested-list `flatten`, immutable scalar `sorted`, zero-copy `chunked`, compiler-fused transform pipelines that avoid intermediate list buffers, typed `for value in list:` / `for index, value in list:` iteration, and zero-copy `take` / `skip` views that compose through typed pipelines; the bootstrap currently keeps list values immutable/local until ownership-safe escaping storage is implemented;
- function signature, return, and argument validation;
- structured parse/type/codegen diagnostics with stable source IDs, reusable source-span metadata, and safe multi-error parser/type-checker recovery;
- terminal diagnostics that show the offending source, exact carets, related declaration labels and suggested fixes, automatically colorize interactive terminals, and wrap/crop to the current terminal width;
- parser recovery that reports syntax errors from later malformed functions instead of stopping at the first one;
- a native bootstrap backend that emits C and invokes Clang with optimization enabled;
- checked `i64` arithmetic at runtime: addition, subtraction, multiplication, negation, and division fail explicitly on overflow or invalid division instead of relying on C signed-overflow behavior;
- CLI commands for checking, deterministic formatting, emitting C, building native executables, automatically running/rebuilding development targets, and serving bootstrap LSP diagnostics over stdio, including package-root/`flux.toml` targets;
- primitive constant folding for `i64`, `bool`, and `str`: top-level constants support forward references with no runtime global storage, while pure literal/constant subexpressions in ordinary code and static UI/application metadata collapse before native emission with checked arithmetic and boolean short-circuiting preserved;
- compiler tests and runnable native examples, including typed shell-style call flow, local immutable lists/indexing/slicing/comprehensions, nested structs, struct destructuring, zero-cost type aliases, folded constants, payload enums, exhaustive matching, named/default parameters, higher-order functions, explicit mutation/`while`, flat-grid UI syntax, typed built-in UI properties, parameterized view composition, and native GTK4/Wayland Text/Button/TextInput/Image/Toggle/Radio controls with typed callback dispatch, live text-change/submit callbacks, hover/leave/focus events, button keyboard shortcuts, autofocus, password masking, maximum input length, file-backed images, tooltips, accessible labels/descriptions, Pango-backed family/slant/decoration/spacing typography, static or state-driven transforms, and read-only window/orientation/display-scale bindings for responsive property expressions.

The C backend is a bootstrap implementation, not the final backend architecture. The intended next backend milestone is a direct typed IR suitable for LLVM-class optimization and target-specific lowering.

## Create a project

Scaffold a minimal native GUI package that is immediately ready for the P0 dogfood loop:

```sh
./tools/flux new my-flux-app
./tools/flux-nvim my-flux-app/src/main.flux
```

Inside Neovim, run `:FluxRun my-flux-app` to compile and launch the package with automatic save-triggered rebuilds. `flux new` also creates `tests/smoke.flux`, so the package can immediately exercise the bootstrap integration-test runner with `./tools/flux test my-flux-app`. `flux new` refuses to overwrite a non-empty directory. `./tools/flux` is a repo-local bootstrap launcher; after `cargo build`, the same CLI is available directly as `target/debug/flux`. The compatibility binary `fluxc` remains during bootstrap development.

Create the first host-native distribution bundle with `./tools/flux package my-flux-app`. By default it writes `dist/<name>-<version>-<os>-<arch>/` containing the package-named native executable and the exact `flux.toml`; `-o <directory>` chooses another destination and `--mode` selects debug/profile/release. Existing bundle directories are not overwritten. This is intentionally a bootstrap host bundle, not yet a self-contained distro/store package: GTK and other native runtime dependencies remain host responsibilities.

Native Clang outputs are content-addressed and reused across `flux build`, `flux run`, `flux test`, and `flux package` when the generated C, build mode, Flux compiler version, and host target are unchanged. The cache uses `$FLUX_CACHE_DIR/native` when configured, then the XDG cache directory or `~/.cache/flux/native`; this is a bootstrap artifact cache, not yet incremental semantic/codegen compilation or a managed remote cache. Generated C is streamed directly to Clang rather than compiled from PID-named temporary files, so identical debug/profile/release builds are byte-reproducible on the same host/toolchain; cross-machine reproducibility still requires pinned toolchain/runtime/dependency identities.

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
./tools/flux build examples/hello.flux -o hello
./hello
```

Build modes are explicit and predictable: `flux build` defaults to `release`, while `--mode debug`, `--mode profile`, and `--mode release` select no-optimization/full-debug, optimized-with-debug/frame-pointers, and aggressive optimization/LTO profiles respectively. `flux run` defaults to `debug` for development but accepts the same `--mode` override.

Run in development mode with automatic save detection:

```sh
./tools/flux run examples/hello.flux
```

The first native GUI dogfood example uses `app HelloApp(...)` instead of `fn main` and lowers its existing flat `view` grid to GTK4 native controls on Linux. Bootstrap application metadata may set a compile-time `title: str`, `width: i64`, and `height: i64`; dimensions must be positive and may reference compile-time constants:

```sh
./tools/flux build examples/hello_app.flux -o hello-app --mode debug
./hello-app
```

`examples/hello_app.flux` opens a real native window containing `Text` and `Button` plus explicit view-local state. Its button uses the functional transition `onPress: clicked => !clicked`; the compiler type-checks that next-state expression and the Linux runtime refreshes the state-derived native label/button properties without rebuilding the window/grid. `examples/counter_app.flux` extends the same model to `i64` state and `count => count + 1`, with state-derived conditional labels. Bootstrap native UI state intentionally remains limited to safe scalar `bool`/`i64` values until owned strings and aggregate ownership semantics exist. GTK4 is a bootstrap Linux platform backend, not a source-language widget model: Flux code remains flat/function-first and does not import or construct GTK objects. The backend uses GTK's native Wayland integration when launched on Wayland.

The bootstrap runner watches imported Flux modules automatically, debounces rapid saves, recompiles on change, and restarts only after a successful replacement build. Compiler errors keep the last good process untouched and the watcher remains active until the next save. State-preserving hot apply is a later development-ABI milestone; the current runner is the controlled-restart foundation for it.

Run native Flux integration tests. A package target discovers sorted `tests/*.flux` programs; a direct `.flux` target runs that one test. Each test is an ordinary headless `fn main() -> i64` program and passes only when its native process exits successfully:

```sh
./tools/flux test examples/package
./tools/flux test examples/hello.flux
```

Check or explicitly analyze without producing a binary:

```sh
./tools/flux check examples/hello.flux
./tools/flux analyze examples/hello.flux
```

Inspect the currently runnable Flux target, verify native GUI prerequisites, and clean generated target artifacts when needed:

```sh
./tools/flux devices
./tools/flux doctor
./tools/flux clean examples/package
```

`flux devices` currently reports the honest bootstrap device surface: Linux desktop plus whether the active Wayland/X11 session is launch-ready. `flux clean` removes the target's default native binary and last-known development-run status and is safe to run repeatedly.

Human diagnostics use the terminal width (`COLUMNS` when supplied, otherwise the interactive terminal width) and enable ANSI color only for an appropriate terminal. `NO_COLOR` disables color; `FORCE_COLOR=1` can force it. Long source lines and paths are cropped around the relevant span instead of overflowing, while diagnostic messages, labels, notes, and fixes wrap to fit.

Format source deterministically, or verify canonical formatting in CI:

```sh
./tools/flux format examples/hello.flux
./tools/flux format examples/hello.flux --check
```

For the first zero-install editor dogfood path on Linux, use the checked-in Neovim launcher:

```sh
./tools/flux-nvim examples/hello_app.flux
```

It uses the repository's Neovim runtime, gives `.flux` files immediate syntax colouring, starts the built `flux lsp`, and leaves the user's global Neovim configuration untouched. Inside the editor, `:FluxRun` opens a terminal split running the current Flux target with automatic save-triggered rebuild/restart. The complete manual acceptance sequence is documented in `docs/manual-e2e.md`.

Editors can also launch the bootstrap language server directly over stdio:

```sh
./tools/flux lsp
```

The current LSP slice publishes parse/type diagnostics using the same source spans, labels, and fix metadata as the compiler, including real import-graph type checking with every open unsaved Flux buffer substituted as an in-memory project overlay. It also serves canonical whole-document formatting, machine-applicable quick fixes, safe declaration/unambiguous-symbol hover and go-to-definition, dependency-graph references/rename, typed signature help for every current parenthesized call form (ordinary/imported functions, first-class function values, interface capabilities, enum payload constructors, interface packing, and compiler built-ins), full-document semantic highlighting, focused inferred-type inlay hints, and completion for keywords/builtins/current-module plus public forward-import declarations and lexically visible local bindings. Qualified completion for enum/interface namespaces works during incomplete edits such as `Outcome.` and `Storage.`, including reachable imported declarations; struct member completion also survives incomplete edits for explicitly typed parameters/`let`/`var` values, transparent aliases, nested field paths such as `user.profile.`, and named single-return function results such as `loadUser().`. Flat `view` blocks receive contextual completion and hover from the compiler's own UI contracts: grid declarations, built-in element kinds, typed element properties, and composed-view parameters; same-file custom-view property completion keeps earlier contracts available while the currently edited view is malformed, and built-in UI hover remains available during incomplete edits. Inlay hints are intentionally limited to types Flux actually infers, such as pattern and range-loop bindings, rather than repeating explicit annotations. Imported ordinary-function completion, hover, and signature help use unsaved overlays as well. Ambiguous/shadowed navigation and rename are deliberately refused rather than guessed. Reverse-dependent discovery, fully resolved shadow-aware usage navigation, more complex recovered-expression member completion, and richer UI navigation remain later LSP milestones.

Tooling/CI can request structured diagnostics without parsing human text:

```sh
./tools/flux check examples/hello.flux --json
```

Packages can select their entry source with `flux.toml`:

```toml
[package]
name = "package-example"
version = "0.1.0"
entry = "src/main.flux"
```

The package directory or manifest can then be passed directly to project-aware commands:

```sh
./tools/flux check examples/package
./tools/flux build examples/package -o package-example
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
fn loadConfig(path: str) -> (str, error) {
    return load(path)
}
```

Forwarding is positional and strictly checked: both arity and every return type must match the enclosing function signature.

When a function needs the successful values before returning, Flux provides a narrow explicit propagation form:

```flux
fn loadConfig(path: str) -> (str, error) {
    let data: str, err: error = load(path) else return
    print(data)
    return data, nil
}
```

`else return` is only valid on a multi-value destructuring binding when the final value is `error` and the called function's complete return shape exactly matches the enclosing function. It evaluates the call once, returns that exact result when the error is non-`nil`, and otherwise continues with the destructured bindings. It is ordinary control flow, not exception handling or stack unwinding.

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

## Roadmap

`ROADMAP.MD` is the source of truth for Flux development and is updated at the start of every development session. It covers the complete language/compiler plan, Dart-inspired non-OOP ergonomics, ownership, native UI, Android/iOS/desktop/web/server targets, automatic save-triggered hot reload, testing, LSP, debugger, profiler, packaging, and Flux 1.0 criteria.

See `ROADMAP.MD` for planned work and `docs/language.md` for the evolving language specification.
