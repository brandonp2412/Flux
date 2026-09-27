# Platform support tiers

Flux publishes a versioned platform-support policy so tooling and users can distinguish implemented, validated targets from preview and unavailable targets without inferring support from backend source files.

The current policy version is `1`. Query it with:

```sh
flux platforms --version
flux platforms --json
```

`--json` emits schema version `1`, the policy version, and one record per roadmap target.

## Tier definitions

- **validated** — the target has an implemented user-facing build path and repository-level validation appropriate to its scope. This does not imply that every optional Flux API is portable to the target; target-specific compile-time rejections remain part of the contract.
- **preview** — a user-facing backend/artifact path exists and is regression-tested, but it is not in the primary platform end-to-end support gate yet. Preview targets may have larger documented capability gaps.
- **unavailable** — Flux does not currently provide a user-facing build backend for the target. This is an explicit non-support status, not a promise that partial internal code is usable.

## Current matrix

| Target | Tier | Scope | Current boundary |
| --- | --- | --- | --- |
| Linux | validated | native app | GTK4 native application build/run/package and the primary development workflow |
| Android | validated | native app | APK/AAB native application build plus device/Waydroid end-to-end validation |
| Windows | preview | native app | Win32 lowering and MSIX/release tooling exist, but Windows is not yet in the primary platform end-to-end gate |
| Web | preview | browser | browser build/deployment path exists; parity with native application targets remains incomplete |
| Headless/server | validated | Linux native | Linux native headless/server programs, including static packaging when the selected toolchain supplies a static runtime |
| iOS | unavailable | none | no user-facing iOS build backend |
| macOS | unavailable | none | no user-facing macOS build backend |

The machine-readable table in `src/platform_support.rs` is the canonical tier inventory. Documentation must not upgrade a target independently of that table and its validation. A tier change is a support-policy change: update the table, this document, roadmap evidence, and target tests together.

## 1.0 roadmap implication

The Flux 1.0 roadmap item covering Android, iOS, Windows, macOS, Linux, web, and headless/server remains incomplete while any listed target is `unavailable`. The tier contract makes that gap explicit; it does not convert an unavailable backend into supported functionality.
