# Flux type and memory model

Flux type and memory-model compatibility is version **1**. The version is
reported by `flux memory --version` and is independent of grammar, formatter,
package, UI, and native ABI compatibility.

The stable rules for the current language are:

- `i64`, `bool`, immutable `str`/`error` handles, capture-free function
  pointers, closed interface values, and recursively copyable structs/enums
  are `Copy`.
- `i64`, `bool`, explicit `error` values, capture-free function pointers, and
  recursively sendable aggregates are `Send`. Borrowed strings and values
  containing borrowed string storage are not `Send` across detached tasks.
- A direct transfer of a non-`Copy` local moves it. Reads after the move are
  rejected using normalized CFG reaching-definition facts.
- Normalized typed call boundaries retain each argument's value identity,
  reaching definitions, and named aggregate field path. This is provenance
  for future consuming-call/partial-move checking; ordinary calls do not
  consume values in the bootstrap model.
- Bootstrap list/view values are immutable borrowed descriptors. They cannot
  escape through returns or aggregate storage, and an owner cannot move while
  a derived view is live. `borrow` makes this reborrow explicit.
- Safe Flux code has no tracing-GC or reference-counting requirement. Native
  lowering remains allocation-free wherever the source semantics require no
  allocation.

The following shapes are deliberately outside version 1 until their ownership
contracts are specified and implemented: owned strings and collections,
first-class references, exclusive mutable borrows, partial moves, consuming
calls beyond the bootstrap `drop` boundary, general escaping closures, owned
resources/destruction, and dynamically transferable JSON or media values.

Adding a new type that obeys these rules is additive. Changing whether an
existing type is `Copy`/`Send`, changing move or borrow validity, introducing
an implicit ownership escape, or changing the native lifetime obligation is a
breaking type/memory-model change and requires a new compatibility version.
