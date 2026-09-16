# Flux package registry index

This directory is the static, GitHub-hosted index consumed by Flux's
`StaticRegistryProvider`. It deliberately contains metadata only; package
archives live on immutable GitHub Releases and are addressed by the `asset`
URL and verified by the `sha256` field in each release record.

Each package has this layout:

```text
<package>/
  owner.txt
  versions.txt
  <version>.toml
```

`versions.txt` contains one SemVer per line. `<version>.toml` is registry
metadata format 1 and must contain the package name, GitHub source repository,
Flux compatibility requirement, immutable release asset, SHA-256 digest,
dependency requirements, and yank state. Versions and metadata are immutable:
publishing a new release adds a new version, while a broken release is marked
`yanked = true` rather than replaced or removed. `owner.txt` is the explicit
package-name ownership record used by the publisher.

The compiler can consume this directory directly with `FLUX_REGISTRY_DIR`, or
the same tree can be served as static HTTPS content and selected with
`FLUX_REGISTRY_URL`. Metadata is cached by the compiler and exact lockfile
entries remain usable offline, so the index does not require a database or an
always-on service.

Validate a checked-out index locally, or in CI, with:

```text
flux registry validate registry
```

The command checks package ownership records, sorted version lists, matching
release metadata, and rejects unlisted metadata files before publication.

Registry changes should be made by the publisher workflow after release and
archive verification. Do not add mutable branch archives, install hooks,
native build instructions, credentials, or generated build output here.
