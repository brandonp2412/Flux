# Flux UI API compatibility

Flux has an explicit compatibility version for its compiler-owned portable UI API. The current UI API version is **1** and is exposed by the compiler library as `UI_API_VERSION` and by the user-facing CLI:

```text
flux ui --version
```

The command prints only the integer version so editors, templates, project tooling, and compatibility checks can compare the UI contract without scraping compiler diagnostics or backend details.

## What the UI API version covers

Version 1 covers the source-level portable UI contract that is shared across native targets: `view` and `app` UI semantics, built-in element names, compiler-owned element property names and static types, event callback signatures, view state/derived-state contracts, grid/flow layout directives, portable focus/input/accessibility properties, semantic presentation values, and compiler-owned UI environment bindings.

The version describes the Flux source API, not GTK, Android View, JNI, CSS, Pango, or any other backend implementation detail. Flux may replace or optimize native lowering without changing the UI API version when existing valid UI source keeps the same portable meaning.

## Version 1 compatibility rules

Within one UI API version, compiler releases may add a new built-in element, property, semantic value, optional app metadata field, or platform capability when the addition does not change the meaning or validity of existing portable UI source. Backend bug fixes, performance improvements, accessibility improvements that preserve the documented source contract, and new target implementations also do not require a version increase.

The UI API version must increase before a release intentionally makes an incompatible portable UI change such as:

- removing or renaming an existing built-in element, property, layout directive, event, environment binding, or semantic value;
- changing the static type, required/optional status, callback signature, or default source-level meaning of an existing UI property;
- making previously valid portable UI source invalid for reasons other than correcting behavior that contradicted the documented contract;
- changing state, derived-state, composition, layout, focus, input, or accessibility semantics so the same valid source has materially different portable behavior;
- changing a portable property into a target-specific API, or exposing native toolkit objects/classes as part of the ordinary Flux UI programming model.

Explicit target-specific capabilities such as `android.*` are versioned by their platform/API support policy rather than by pretending every platform API is portable UI. Adding a new target does not by itself change the portable UI API version; that target must implement the applicable versioned portable contract or clearly reject unsupported features according to its documented support tier.

## Relationship to other compatibility contracts

UI API compatibility is independent of the grammar version, formatter version, diagnostics JSON schema version, package-format version, and native/FFI ABI policy. A syntax addition can require a grammar change without changing UI semantics, while a source-compatible backend rewrite can change native implementation details without changing either grammar or UI API versions.

Tools that generate or persist Flux UI source may record `flux ui --version` alongside the compiler version. A tool that encounters a UI API version it does not understand should avoid assuming that generated UI properties or semantic defaults remain compatible until it is updated for that version.
