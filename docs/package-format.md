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

Version 1 includes the current `[package]` fields (`format_version`, `name`, optional `version`, `entry`, and optional `assets`), the optional `[dependencies]`, `[constants]`, `[translations]`, and `[native]` tables, and the current optional `[android]` configuration surface. Existing version-1 fields keep their meanings and validation rules. When present, `assets` names a relative directory inside the package root; package commands preserve its contents under the portable runtime resource root `assets/`.

`[dependencies]` is an additive version-1 extension. Registry entries use quoted exact/caret/tilde SemVer requirements or `"*"`; local development entries use `{ path = "relative/path" }` and may add `version = "^1.2.3"` (or another supported SemVer requirement) when they must satisfy the same version contract as a registry package; Git development entries use `{ git = "repository-url", rev = "immutable-revision" }`. The manifest parser validates names, source shape, required fields, duplicates, relative local paths, and dependency requirement syntax.

Package imports use the reserved quoted namespace `pkg:<dependency>/<module.flux>`, for example `import "pkg:math/src/lib.flux"`. This is intentionally distinct from ordinary quoted relative imports, so a dependency can never be mistaken for a same-named local source file. Declared local `path` dependencies are loadable now: the dependency must contain its own valid `flux.toml`, imported modules stay inside that dependency root, its ordinary relative imports remain package-local, and loaded source identities use the dependency package's own `[package].name` rather than the caller's dependency alias. When a path dependency declares `version`, Flux applies SemVer precedence and exact/caret/tilde/wildcard matching to the dependency package's `[package].version` before loading imported source. The same resolver deterministically chooses the highest compatible version from a candidate set, ignores build metadata for precedence, and does not admit prerelease candidates for a stable-only requirement. Registry and Git imports remain unavailable until their fetch transports land; the compiler reports that unresolved state instead of falling back to a local path.

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

## Reproducible lockfile

Packages with dependencies use a compiler-owned `flux.lock`. Run `flux lock <package-dir|flux.toml>` after changing dependency declarations or resolved local package metadata. Package-aware analysis/build commands reject a missing or stale lockfile rather than silently resolving a different graph.

Lockfile format version 1 is generated deterministically. Entries are ordered by dependency path through the package graph. Loadable path dependencies record the declared relative source plus the resolved dependency package name and exact package version when one is declared; transitive path dependencies are included recursively. Git declarations record their immutable revision, and registry declarations record their current SemVer requirement until registry transport supplies an exact fetched version/checksum. Re-running `flux lock` with unchanged inputs produces byte-identical output. Editing a transitive path dependency's package metadata makes the root lockfile stale and requires regeneration before the package can load again.

Backward-compatible additions, such as a new optional field with a well-defined default, may remain in package format version 1. A change that removes or renames a supported field, changes an existing field's meaning incompatibly, makes previously optional metadata mandatory without a compatible default, or otherwise requires existing valid manifests to be rewritten must use a new package-format version.

Unknown fields continue to fail explicitly rather than being ignored, which prevents misspellings or newer manifest features from degrading silently on an older compiler.

Application UI can address a declared resource as `asset://relative/path`. Linux development runs bind that URI to the declared source directory, directory/tar packages place the same tree beside the executable under `assets/`, and Android APK/AAB builds embed it in the platform asset store. Resource paths stay relative to the declared root; packaging rejects symbolic links so a package cannot accidentally capture files outside its resource tree.

## Tooling

`flux new` emits the current package-format version. `flux lock` writes the dependency lockfile described above. Package-aware commands such as `flux check`, `flux build`, `flux run`, `flux test`, and `flux package` all use the same manifest parser and lock validation, so they enforce the same compatibility and reproducibility contract.
