# C interoperability

Flux can emit a C header for the current stable scalar export ABI:

```sh
flux emit-c-header path/to/package -o flux_api.h
```

The header contains declarations for `pub fn` exports only. Private functions and `main` are not exposed. Current supported ABI types are `i64`, `bool`, `str`, and `error`, including transparent aliases of those types. Multiple return values lower to a generated C result struct whose fields are named `v0`, `v1`, and so on.

`i64` lowers to `int64_t`, `bool` to C `bool`, and `str` / `error` to borrowed `const char *`. C callers must not free or retain those borrowed pointers beyond the lifetime guaranteed by the Flux value that produced them.

The generated header is C++ compatible through an `extern "C"` block and uses the compiler ABI symbol names, for example `flux__fn_parse`.

Public exports containing lists, function values, interfaces, structs, or enums are rejected by `emit-c-header` for now. Those representations need explicit ownership and ABI contracts before Flux can promise them across a foreign boundary. Importing arbitrary C functions into Flux, library/link metadata, callbacks, and cross-FFI ownership rules remain part of the broader platform interoperability milestone.
