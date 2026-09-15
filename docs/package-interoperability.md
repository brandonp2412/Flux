# Package ecosystem and platform interoperability

Flux packages are source packages first. A package contains Flux modules,
resources, and a strict `flux.toml`; dependency resolution, lockfiles, native
builds, and platform selection are compiler-owned operations. Package loading
does not execute package-controlled install, build, or lifecycle scripts.

## Dependencies and reproducibility

Declare registry, local path, or revision-pinned Git dependencies in
`flux.toml`. Normal analysis produces a deterministic `flux.lock` containing
exact versions and immutable source identities. Registry archives and Git
snapshots are canonical `.fluxpkg` files addressed by SHA-256, and the global
cache is content-addressed. Use `--locked` in CI to reject stale or missing
lock data; use `flux vendor` when a self-contained offline source snapshot is
required.

Registry metadata is a static index rather than a required service. The
compiler accepts a checked-out index through `FLUX_REGISTRY_DIR` or the same
layout from a static mirror through `FLUX_REGISTRY_URL`. Exact lock entries can
be replayed from verified cache content without contacting a registry.

Dependency modules use the explicit `pkg:<package>/<module.flux>` namespace.
Local and Git packages retain their own manifest roots, imports, constants,
platform mappings, and package-qualified module identities; sibling modules do
not accidentally leak symbols across those boundaries.

## Native interoperability

Native libraries are opt-in package metadata. A package may declare validated
library names and search paths, while Flux source uses typed `extern c`
declarations for the callable boundary. The compiler owns linker arguments and
rejects unsafe paths and ownership-sensitive foreign signatures. Use
`flux emit-c-header` for the documented public C boundary; lists, owned
resources, and other unfinished ownership-sensitive values are intentionally
not part of that ABI.

Generated platform glue is private implementation detail. Android framework
entry points and JNI helpers, Linux GTK/Wayland integration, Windows Win32
integration, and the web DOM backend are emitted by the compiler. Flux source
does not need application-authored Java/Kotlin/JNI, method channels,
serialization buses, plugin registries, or platform bridge layers.

## Platform-selected modules

Portable code imports one logical module. A package can provide target-selected
implementations for that module, and the compiler loads only the selected
implementation. The implementation may call a first-class target capability
such as `android.*`; unselected implementations and unreachable target helpers
are excluded by normal reachability/tree-shaking. This keeps platform choice a
compile-time concern with no runtime target dispatcher.

The current contracts and regression coverage are split across
[`package-format.md`](package-format.md), [`native-abi.md`](native-abi.md),
[`cross-compilation.md`](cross-compilation.md), and compiler tests for
dependency graphs, native package metadata, platform-module selection, and
direct native lowering. This document describes how those pieces fit together;
it does not claim support tiers for targets whose backend or release workflow
is still marked incomplete in `ROADMAP.MD`.
