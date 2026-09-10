# Testing Flux programs

Flux testing uses the same compiler, type system, native backend, and error model as ordinary programs. The current bootstrap `flux test` runner executes headless `.flux` programs and treats a zero process exit as success. Package tests are discovered from `tests/*.flux`; dedicated in-language unit-test declarations and UI/golden test APIs remain roadmap work.

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
