# C interoperability

Flux can emit a C header for the current stable scalar export ABI:

```sh
flux emit-c-header path/to/package -o flux_api.h
```

The header declares `FLUX_C_ABI_VERSION` (currently `2` for project/package compilation) so native consumers can pin the scalar export contract they were compiled against. It contains declarations for `pub fn` exports, typedefs for public transparent aliases of the supported scalar ABI types, and typed macros for public compile-time scalar constants. Private functions/constants and `main` are not exposed. Current supported ABI types are `i64`, `bool`, `str`, and `error`. Multiple return values lower to a generated C result struct whose fields are named `v0`, `v1`, and so on; fields and function signatures preserve a directly named public scalar alias through generated `flux__alias_<Name>` typedefs. Chained public aliases preserve their declared alias relationship in the header, and alias dependencies are emitted before aliases that refer to them even when Flux source uses forward alias declarations.

`i64` lowers to `int64_t`, `bool` to C `bool`, and function `str` / `error` values lower to borrowed `const char *`. C callers must not free or retain those borrowed function-result pointers beyond the lifetime guaranteed by the Flux value that produced them. Public compile-time `str` constants instead expand to ordinary escaped C string literals with static storage duration.

Generated constants use compiler-prefixed names such as `flux__const_DEFAULT_LIMIT`. ABI v2 callable exports use deterministic package/module-qualified linker names of the form `flux__abi_<module-hex>__fn_<Name>`; `<module-hex>` is the lowercase hexadecimal encoding of the stable module identity documented by the package loader. For example, two packages that both export `parse` no longer publish the same linker symbol. Generated C keeps its shorter internal source-level identifier and binds it to the qualified linker symbol with a compiler-owned assembler label, while the generated header declares the qualified identifier directly. Multi-return header struct tags use the same module qualification. The generated header is C++ compatible through an `extern "C"` block. Compiler regression coverage compiles generated headers as C11 and compiles package C output to an object before inspecting the exported linker symbols, so source-level or module-identity changes cannot quietly desynchronize the foreign ABI.

Public exports containing lists, function values, interfaces, structs, or enums are rejected by `emit-c-header` for now. Those representations need explicit ownership and ABI contracts before Flux can promise them across a foreign boundary. Importing arbitrary C functions into Flux, library/link metadata, callbacks, and cross-FFI ownership rules remain part of the broader platform interoperability milestone.
