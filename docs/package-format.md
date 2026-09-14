# Package format

Flux package manifests use `flux.toml`. The manifest schema has an explicit compatibility version so compiler upgrades cannot silently reinterpret an existing package.

## Version 1

New packages created by `flux new` write `format_version = 1` in the `[package]` table. Existing manifests that omit `format_version` are treated as version 1 for compatibility.

```toml
[package]
format_version = 1
name = "example"
version = "1.0.0"
entry = "src/main.flux"
assets = "assets"
```

The compiler rejects an explicitly declared package-format version it does not support before building the package. It does not silently downgrade, upgrade, or reinterpret an unknown version.

The package-format version covers the schema and semantics of `flux.toml`; it is independent of `[package].version`, which is the application's own release version.

## Compatibility policy

Version 1 includes the current `[package]` fields (`format_version`, `name`, optional `version`, `entry`, and optional `assets`), the optional `[dependencies]`, `[constants]`, `[translations]`, `[native]`, and `[platform.<target>]` tables, and the current optional `[android]` configuration surface. Existing version-1 fields keep their meanings and validation rules. When present, `assets` names a relative directory inside the package root; package commands preserve its contents under the portable runtime resource root `assets/`.

`[dependencies]` is an additive version-1 extension. Registry entries use quoted exact/caret/tilde SemVer requirements or `"*"`; local development entries use `{ path = "relative/path" }` and may add `version = "^1.2.3"` (or another supported SemVer requirement) when they must satisfy the same version contract as a registry package; Git development entries use `{ git = "repository-url", rev = "immutable-revision" }`. The manifest parser validates names, source shape, required fields, duplicates, relative local paths, and dependency requirement syntax.

Package imports use the reserved quoted namespace `pkg:<dependency>/<module.flux>`, for example `import "pkg:math/src/lib.flux"`. This is intentionally distinct from ordinary quoted relative imports, so a dependency can never be mistaken for a same-named local source file. Declared local `path` dependencies are loadable now: the dependency must contain its own valid `flux.toml`, imported modules stay inside that dependency root, its ordinary relative imports remain package-local, and loaded source identities use the dependency package's own `[package].name` rather than the caller's dependency alias. When a path dependency declares `version`, Flux applies SemVer precedence and exact/caret/tilde/wildcard matching to the dependency package's `[package].version` before loading imported source. The same resolver deterministically chooses the highest compatible version from a candidate set, ignores build metadata for precedence, and does not admit prerelease candidates for a stable-only requirement. Registry imports use exact checksum-verified releases recorded in `flux.lock`: with a provider configured, resolution/materialization uses that provider; without one, the loader replays the immutable locked repository/version/asset/SHA-256 identity and materializes it from the shared cache or exact asset URL. Revision-pinned Git fetch transport remains pending. The compiler reports unresolved Git state instead of falling back to a local path.

## Per-platform package modules

A package can keep one stable import/module contract while selecting a native implementation for a supported target at compile time:

```toml
[platform.linux]
modules = ["src/notifications.flux=platform/linux/notifications.flux"]

[platform.android]
modules = ["src/notifications.flux=platform/android/notifications.flux"]
```

Each mapping is `logical/module.flux=target/implementation.flux`. Both sides must be normalized package-relative `.flux` paths: absolute paths, `.`/`..` traversal, non-Flux files, and duplicate logical mappings are rejected. Ordinary relative imports and `pkg:<dependency>/...` imports continue to name the logical module; the loader selects the target implementation before type checking/code generation and preserves the logical package/module identity for symbols and generated ABI names. Path dependencies use their own package's platform mappings, so an application does not need to know how a dependency implements the same source-level contract on Linux versus Android. The logical package entry may also be mapped when the target needs a different entry implementation.

Selection is compile-time only. Non-selected implementations are not loaded into the target program, and Flux inserts no runtime target dispatcher, serialized plugin protocol, method channel, or application-authored native bridge. Normal native analysis/build selects Linux mappings; Android application builds select Android mappings before native/JNI lowering. Future native targets must extend this compiler-owned selection model rather than exposing platform implementation objects to application source.

Relative imports inside a selected implementation resolve as though that implementation occupied its logical module path, not from the physical target-specific file. This lets every target implementation import the same nearby shared Flux interface/capability module while portable callers keep one logical import. Portable capability contracts can therefore be ordinary Flux interfaces in an unmapped shared module or dependency: each selected implementation imports that interface, keeps its concrete provider private, and exposes only the stable logical-module wrapper/interface to application code. A target implementation may call first-class native APIs such as `android.*` while the Linux implementation uses Linux facilities; compile-time selection plus existing static/native interface lowering adds no target switch, object hierarchy, method channel, serialization bus, or user-maintained bridge at runtime.

## Package constants and resources

`[constants]` declares package-scoped compile-time primitive values. Names use ordinary Flux identifier syntax, and values are `i64`, `bool`, or quoted `str` literals:

```toml
[constants]
apiVersion = 3
debugMenu = false
productName = "Example"
```

Flux source reads these values through the explicit `package` namespace, for example `package.apiVersion`. They participate in ordinary constant expressions and parameter defaults and are substituted during compilation, so there is no runtime configuration lookup or generated mutable global. Each package owns its namespace: source loaded from a path dependency resolves `package.*` against that dependency's own `flux.toml`, not the importing application's manifest.

Localized string resources use `[translations]`; `locale.text`, `locale.select`, and `locale.plural` resolve the compiled entries described in the language reference. Both tables are package-owned manifest data rather than platform-specific bridge APIs.

## Native/plugin metadata

Packages that carry native integration metadata may declare a `[native]` table:

```toml
[native]
plugin = true
libraries = ["sqlite3", "ssl"]
search_paths = ["native/lib"]
```

`plugin` marks the package as a native/plugin package for tooling and future package-resolution policy; it defaults to `false`. `libraries` records logical native library names without accepting arbitrary linker command fragments. Names are restricted to portable ASCII linker-name characters, sorted, and de-duplicated. `search_paths` records package-relative native library locations, rejects absolute paths and `.`/`..` traversal, resolves against the package root, and is likewise normalized deterministically.

For Linux native compilation, package-aware build, run, test, debug, profile, and packaging flows translate these validated values into compiler-owned library search and `-l` linker arguments. They pair with typed `extern c "symbol" fn ...` declarations in Flux source; the manifest alone never creates callable symbols or bypasses the compiler's restricted native-import type checks. Native libraries are linked but are not automatically bundled for redistribution. Android and other platform bindings remain separate compiler-owned integrations, and ownership-sensitive FFI shapes remain rejected until their cross-boundary lifetime rules are explicit.

## Source package archives and cache

The package-ecosystem bootstrap defines `.fluxpkg` format version 1 as a reproducible gzip-compressed POSIX tar source archive. Entries are sorted, timestamps are fixed to the Unix epoch, numeric owner/group are normalized to zero, and root `.git`, `target`, and `dist` trees are omitted. Symlinks are rejected so a published source package cannot silently depend on a host path outside the package tree. The archive creator returns the SHA-256 digest of the exact bytes that were produced.

Already-obtained archives can be placed in the Flux package cache by their lowercase SHA-256 digest. Cache insertion verifies the source bytes before storing them, and cache lookup re-hashes the stored archive before returning it; corrupt or tampered entries fail closed. `FLUX_PACKAGE_CACHE_DIR` selects the package-cache root explicitly, otherwise package storage lives below the existing Flux/XDG cache location. Verified extraction rejects absolute/parent-traversal archive entries plus symbolic/hard links, extracts into a temporary sibling directory, requires a root `flux.toml`, and only then atomically installs the source tree. Registry release materialization is cache-first: online fetching uses the immutable metadata/lock asset URL and verifies SHA-256 before admitting bytes to the cache. Strict offline mode can replay exact registry identities directly from `flux.lock`; it reads only verified cached archives (plus cached provider metadata when live resolution is requested) and never attempts a network request.

`flux fetch <package-dir|flux.toml> [--offline]` resolves the transitive registry graph to one highest compatible non-yanked version per package and fills the global content-addressed cache when a provider is configured; without one, it replays the exact immutable registry releases already committed in `flux.lock`. `flux vendor <package-dir|flux.toml> [-o directory] [--offline]` is the explicit self-contained snapshot escape hatch: it materializes the already-verified graph under `packages/<name>/<version>/` and writes deterministic `flux.vendor.lock` metadata. Vendoring is never the default dependency layout. Registry graph resolution includes registry dependencies reached through local path dependencies, enforces each release's Flux compatibility requirement, and rejects registry dependency cycles. Revision-pinned Git dependencies resolve to exact commits, are archived through the same canonical `.fluxpkg` producer, enter the same SHA-256-addressed cache, and can be fetched or vendored again in strict offline mode from their immutable lock identity.

Static registry release metadata is compiler-owned format version 1. Each package/version record contains package name, owner, HTTPS source repository, exact SemVer version, Flux compatibility requirement, immutable HTTPS release-asset URL, SHA-256, dependency requirements, and a yank flag. The parser is strict: unknown fields such as install/build hooks fail rather than being ignored. Registry lookup is behind a provider interface: `FLUX_REGISTRY_DIR` selects a deterministic checked-out/local mirror, while `FLUX_REGISTRY_URL` selects a static HTTPS index whose `<package>/versions.txt` lists versions and whose `<package>/<version>.toml` stores release metadata. Static metadata is cached under the Flux package cache so `--offline` never contacts the configured URL. Configured providers feed exact release metadata into lock generation and ordinary package imports. Existing exact registry lock entries are provider-independent: ordinary imports and fetch can replay them without the original provider.

`flux publish package <package-dir|flux.toml> [--repo owner/name] [--registry owner/name]` is the bootstrap GitHub publication path. It requires authenticated `gh`, refreshes and validates the package lock, checks the package, runs its Flux tests, requires an explicit package version, permits only registry SemVer dependencies, and rejects native plugin/library/search-path declarations rather than allowing an unaudited native package boundary. The publisher creates the canonical `{name}-{version}.fluxpkg`, uploads it to the package repository's `v{version}` GitHub Release, and treats a pre-existing release as a valid retry only when downloading the existing asset produces the identical SHA-256. It then clones the configured registry repository (`--registry` or `FLUX_REGISTRY_GITHUB_REPO`), forks it when necessary, writes deterministic `<package>/<version>.toml` plus `versions.txt`, and submits a pull request. A published version's metadata is immutable: an identical re-write is idempotent, while different metadata for the same version fails. `<package>/owner.txt` is the explicit package-name ownership gate; changing ownership requires a deliberate registry change before a different GitHub owner can publish a new version. Yank state remains registry metadata, so new resolution can exclude a broken version without deleting the immutable release bytes needed by existing locks. Hosting the canonical public Flux registry repository itself remains follow-up work.

## Reproducible lockfile

Packages with dependencies use a compiler-owned `flux.lock`. Run `flux lock <package-dir|flux.toml>` after changing dependency declarations or resolved local package metadata. Package-aware analysis/build commands reject a missing or stale lockfile rather than silently resolving a different graph.

Lockfile format version 1 is generated deterministically. Entries are ordered by dependency path through the package graph. Loadable path dependencies record the declared relative source plus the resolved dependency package name and exact package version when one is declared; transitive path dependencies are included recursively. Git declarations record their immutable revision. When a registry provider is configured, registry entries record the exact selected version, immutable repository/release identity, release-asset URL, and SHA-256. Those exact entries are parsed and structurally validated on later builds without re-resolving: manifest requirements must still accept the locked version, immutable identity/hash fields must remain valid, and stale exact locks are never silently downgraded when the provider is absent. A new lock created without any provider still retains the SemVer requirement until exact resolution is available. Re-running `flux lock` with unchanged inputs produces byte-identical output. Editing a transitive path dependency's package metadata makes the root lockfile stale and requires regeneration before the package can load again.

Backward-compatible additions, such as a new optional field with a well-defined default, may remain in package format version 1. A change that removes or renames a supported field, changes an existing field's meaning incompatibly, makes previously optional metadata mandatory without a compatible default, or otherwise requires existing valid manifests to be rewritten must use a new package-format version.

Unknown fields continue to fail explicitly rather than being ignored, which prevents misspellings or newer manifest features from degrading silently on an older compiler.

Application UI can address a declared resource as `asset://relative/path`. Linux development runs bind that URI to the declared source directory, directory/tar packages place the same tree beside the executable under `assets/`, and Android APK/AAB builds embed it in the platform asset store. Resource paths stay relative to the declared root; packaging rejects symbolic links so a package cannot accidentally capture files outside its resource tree.

## Tooling

`flux new` emits the current package-format version. `flux lock` writes the dependency lockfile described above. `flux fetch` and `flux vendor` use the registry/provider/cache contract above, including strict `--offline` operation, while `flux publish package` performs the authenticated GitHub Release plus registry-PR workflow described above. Package-aware commands such as `flux check`, `flux build`, `flux run`, `flux test`, and `flux package` all use the same manifest parser and lock validation. When a registry provider is configured, their ordinary analysis path resolves and checksum-verifies the selected graph, materializes exact package sources under the global content-addressed cache, and loads `pkg:` imports directly from those verified roots. When no provider is configured, the same commands validate and replay exact registry entries from `flux.lock`; `flux fetch --offline` uses only those locked identities plus verified cache contents.
