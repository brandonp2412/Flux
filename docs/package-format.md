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
```

The compiler rejects an explicitly declared package-format version it does not support before building the package. It does not silently downgrade, upgrade, or reinterpret an unknown version.

The package-format version covers the schema and semantics of `flux.toml`; it is independent of `[package].version`, which is the application's own release version.

## Compatibility policy

Version 1 includes the current `[package]` fields (`format_version`, `name`, optional `version`, and `entry`), the optional `[dependencies]` table, and the current optional `[android]` configuration surface. Existing version-1 fields keep their meanings and validation rules.

`[dependencies]` is an additive version-1 extension. Registry entries use quoted exact/caret/tilde SemVer requirements or `"*"`; local development entries use `{ path = "relative/path" }`; Git development entries use `{ git = "repository-url", rev = "immutable-revision" }`. The manifest parser validates names, source shape, required fields, duplicates, relative local paths, and registry requirement syntax. Dependency resolution/fetching and lockfile semantics are separate package-management features, so accepting this metadata does not imply that a dependency graph is resolved yet.

Backward-compatible additions, such as a new optional field with a well-defined default, may remain in package format version 1. A change that removes or renames a supported field, changes an existing field's meaning incompatibly, makes previously optional metadata mandatory without a compatible default, or otherwise requires existing valid manifests to be rewritten must use a new package-format version.

Unknown fields continue to fail explicitly rather than being ignored, which prevents misspellings or newer manifest features from degrading silently on an older compiler.

## Tooling

`flux new` emits the current package-format version. Package-aware commands such as `flux check`, `flux build`, `flux run`, `flux test`, and `flux package` all use the same manifest parser and therefore enforce the same compatibility contract.
