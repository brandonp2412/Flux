# Flux grammar compatibility

Flux source grammar has an explicit compatibility version. The current grammar version is **1** and is exposed by both the compiler library as `GRAMMAR_VERSION` and the user-facing CLI:

```text
flux grammar --version
```

The command prints only the integer version so editors, build systems, package tooling, and other language integrations can record or compare it without scraping human-oriented compiler output.

## Version 1 contract

The parser implementation is the source of truth for accepted Flux syntax. The formatter may canonicalize spelling and whitespace, but it does not define a separate grammar.

A grammar version describes how already-valid Flux source is parsed. Within one grammar version, compiler releases may add syntax only when the addition does not change the parse or meaning of source that was already valid under that version. Fixing parser recovery or improving diagnostics without changing the accepted parse of valid source does not require a version change.

The grammar version must increase before a release intentionally makes any of these incompatible changes:

- removes or rejects syntax that was valid under the current grammar version;
- changes precedence, associativity, grouping, or tokenization so existing valid source parses differently;
- turns an identifier spelling that was previously valid in the same position into a reserved keyword or other special token;
- changes an existing syntactic form so the same valid source denotes a different AST shape or language construct.

Purely additive syntax may remain in the same grammar version when it only gives meaning to source that was previously invalid and cannot alter the parse of existing valid programs. New syntax must still follow Flux's non-negotiable language rules, including brace-delimited function bodies, indentation-based control flow, no comments, no ternary expressions, and no source-language generics.

## Tooling guidance

Tools that persist parsed source, generated edits, or compatibility metadata may record `flux grammar --version` alongside the compiler version. A tool that encounters a grammar version it does not understand should avoid assuming source compatibility and should reparse with a compatible Flux compiler instead.

Grammar compatibility is independent of the formatter version, diagnostics JSON schema version, package-format version, and application/package release version. Each contract is versioned separately so tooling can respond precisely to the compatibility surface that changed.
