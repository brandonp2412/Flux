# Testing Flux programs

Flux testing uses the same compiler, type system, native backend, and error model as ordinary programs. `flux test` executes headless `.flux` programs and treats a zero process exit as success. Package tests are discovered from `tests/*.flux` and are analyzed in the package's real manifest/module context. A package test can import code under test with `import "pkg:self/src/module.flux"`; the reserved `self` package name is resolved inside that package root and cannot escape it. Unit tests therefore stay ordinary Flux programs rather than introducing test-only declaration syntax or a parallel runtime.

## Native UI and accessibility tests

Pass `--ui` to allow a test source to declare an `app`. The test is compiled through the ordinary native application backend, launched as a real native process, and passes only if it exits successfully. UI tests must terminate explicitly when their assertions finish; the runner fails a UI test after 30 seconds instead of leaving a stuck native event loop behind. This keeps UI tests in ordinary Flux code: lifecycle callbacks and application functions perform the test work, and no widget-test object hierarchy or alternate renderer is introduced.

Pass `--accessibility` to run the native UI test with an additional compiler-owned accessibility audit before launch. The audit requires an accessible name for interactive controls and for Image, Button, TextInput, Toggle, and Radio elements unless an element is explicitly `accessibilityHidden: true`. Existing visible `text`, `label`, `title`, or Image `alt` properties satisfy the name where appropriate, while `accessibilityLabel` is the explicit universal override. Audit failures identify the view, element, and source line. `--accessibility` implies `--ui`.

## Golden screenshots and platform E2E

`./tools/flux-golden-test` builds the checked-in mobile showcase through the ordinary Android release backend, installs it on an ADB-connected x86_64 Waydroid device, forces the canonical 1080x1920/360-dpi portrait surface, captures the real native UI, and compares decoded RGBA pixels exactly against `assets/readme/flux-mobile-showcase.png`. Set `FLUX_SCREENSHOT_DEVICE` to select a device explicitly. A mismatch fails and writes `target/flux-golden-diff.png`, so the README image doubles as an executable visual regression baseline rather than a separately rendered test fixture. Python Pillow is required only by this screenshot comparison tool.

`./tools/flux-platform-e2e` exercises both current native application targets end to end: it builds and executes the Linux release fixture and requires its real process output, then builds the Android release showcase, installs it through ADB, launches the generated `FluxActivity`, requires Android ActivityManager to report `Status: ok`, and verifies the application process is alive. It auto-selects an x86_64 Waydroid device or accepts `FLUX_E2E_DEVICE`. This is intentionally a real platform harness rather than a mocked renderer or generated-code-only check.

## Deterministic time

Pass `--deterministic-time` to `flux test` when a test depends on clocks or structured timers. The test process starts with monotonic time at `0` and Unix time at `946684800000` (2000-01-01T00:00:00Z). `time.sleep` and `time.until` advance virtual time immediately instead of waiting for wall time. `time.after` and `time.every` use deadlines on the same virtual clock, so advancing time wakes due timer workers; normal builds and tests without the flag continue to use the native platform clocks and sleeps.

The deterministic clock is compiler-owned test infrastructure selected by the `flux test` runner through a test-build macro injected into generated native C only for that invocation. Ordinary builds compile the test-clock selector to false rather than performing a runtime environment lookup. Flux application source does not gain a mutable clock object, and unreachable time support still tree-shakes normally.

## Dependency fakes without mock objects

Flux does not need class-mocking frameworks to substitute dependencies in tests. Prefer explicit dependency injection through first-class functions when a dependency is one operation, and through interfaces when several related capabilities belong together.

A clock, identifier source, parser, policy decision, or similar single operation can be passed as a typed function value. Production code receives the real function; tests pass a deterministic fake function with the same signature. This keeps the dependency visible in the function signature and lowers through the ordinary native function-value path.

For a multi-operation dependency, define an interface and provide a small fake value type with an ordinary `impl` mapping to fake free functions. Tests pack that fake as the interface value and pass it to the code under test. This uses Flux's normal static/dynamic interface checking and native dispatch semantics; there is no generated proxy object, reflection layer, runtime patching, or class hierarchy.

The compiler regression `function_and_interface_dependencies_support_test_fakes` covers both forms and verifies that the fake implementation lowers through ordinary native functions without a vtable or heap allocation.

## Compiler supported-target self-test

Run:

```sh
./tools/flux-self-test
```

The self-test runs all Rust/compiler tests, builds and executes the optimized Linux package example, then builds a release Android x86_64 APK. CI runs the same command so regressions in either supported native target fail the compiler gate.
