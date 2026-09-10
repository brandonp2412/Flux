# Machine diagnostics contract

`flux check --json` and `flux analyze --json` emit one JSON object on standard output and no human diagnostic text on standard error. The process exits successfully when `ok` is `true` and non-zero when `ok` is `false`.

The top-level object is versioned independently from the compiler implementation. A machine-readable JSON Schema for this contract is checked in as [`diagnostics.schema.json`](diagnostics.schema.json):

```json
{
  "schema_version": 1,
  "ok": false,
  "source_id": 123,
  "diagnostics": []
}
```

## Schema version 1

Every top-level object contains these required fields:

- `schema_version`: integer schema version. Version 1 is the stable initial contract.
- `ok`: boolean indicating whether analysis completed without diagnostics.
- `source_id`: deterministic compiler source identifier for the resolved entry source.
- `diagnostics`: ordered array of diagnostic objects. It is empty when `ok` is `true`.

Every diagnostic object contains these required fields:

- `stage`: one of `parse`, `type`, or `codegen`.
- `message`: human-readable summary text.
- `span`: the primary compiler source span or `null` for a global diagnostic.
- `labels`: zero or more secondary labelled spans.
- `notes`: zero or more explanatory strings.
- `fixes`: zero or more machine-applicable replacement fixes.

A span contains integer `source_id`, `line`, `column`, and `length` fields. A label contains `span` and `message`. A fix contains `span`, `replacement`, and `message`.

Tooling should branch on `schema_version` before interpreting fields and should ignore unknown additional fields within a supported schema version. Additive fields that do not change existing field types or meanings may be introduced without a version bump. Removing or renaming a required field, changing its type or meaning, or changing the envelope shape requires a new schema version.

The JSON diagnostic contract is distinct from LSP wire encoding. The LSP converts compiler spans into the negotiated editor position encoding while reusing the same diagnostic semantics and fix data.
