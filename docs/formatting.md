# Formatting

Flux ships one canonical formatter as part of the compiler. The CLI, LSP, and library all reuse the same implementation; editor integrations should not maintain a second Flux formatting engine.

## Commands

```sh
flux format path/to/file.flux
flux format path/to/file.flux --check
flux format --version
```

`flux format` rewrites a valid Flux source file into canonical form. `--check` leaves the file untouched and exits unsuccessfully when the input is not already canonical, making it suitable for CI. `flux format --version` prints the formatter compatibility version as an integer.

## Stability contract

The canonical formatter is deterministic and idempotent: formatting the same valid source with the same formatter version produces the same bytes, and formatting already-canonical output again does not change it.

Formatter compatibility begins at version `1`. A compiler change that intentionally changes the canonical bytes produced for source that was already canonical under the current version must increment `FORMATTER_VERSION`. Bug fixes that only make previously rejected or incorrectly handled syntax format correctly do not require a version increase unless they rewrite previously canonical valid source.

The compatibility version describes formatting behavior, not the Flux grammar version. New language syntax can be added while formatter version `1` remains current when existing canonical source retains its canonical representation.

Flux source deliberately has no comments, so the formatter has no comment-preservation mode. Invalid source is diagnosed through the compiler parser rather than partially rewritten.

## Integration guidance

Editor integrations should use `textDocument/formatting` from `flux lsp` when possible. Tools that invoke the CLI may record `flux format --version` alongside generated or checked source when they need to detect canonical-format changes across compiler upgrades.
