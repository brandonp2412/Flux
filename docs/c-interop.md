# C interoperability

Flux can emit a C header for the current stable scalar export ABI:

```sh
flux emit-c-header path/to/package -o flux_api.h
```

The header contains declarations for `pub fn` exports, typedefs for public transparent aliases of the supported scalar ABI types, and typed macros for public compile-time scalar constants. Private functions/constants and `main` are not exposed. Current supported ABI types are `i64`, `bool`, `str`, and `error`. Multiple return values lower to a generated C result struct whose fields are named `v0`, `v1`, and so on; fields and function signatures preserve a directly named public scalar alias through generated `flux__alias_<Name>` typedefs.

`i64` lowers to `int64_t`, `bool` to C `bool`, and function `str` / `error` values lower to borrowed `const char *`. C callers must not free or retain those borrowed function-result pointers beyond the lifetime guaranteed by the Flux value that produced them. Public compile-time `str` constants instead expand to ordinary escaped C string literals with static storage duration.

Generated constants use compiler-prefixed names such as `flux__const_DEFAULT_LIMIT`, while callable exports use compiler ABI symbol names such as `flux__fn_parse`. The generated header is C++ compatible through an `extern "C"` block.

Public exports containing lists, function values, interfaces, structs, or enums are rejected by `emit-c-header` for now. Those representations need explicit ownership and ABI contracts before Flux can promise them across a foreign boundary. Importing arbitrary C functions into Flux, library/link metadata, callbacks, and cross-FFI ownership rules remain part of the broader platform interoperability milestone.
