# Flux package registry index layout

This directory documents and validates the static index format used by the public Flux package registry at `https://github.com/brandonp2412/flux-registry`. The compiler reads that hosted index by default from `https://raw.githubusercontent.com/brandonp2412/flux-registry/main`.

The index contains metadata only; package archives live on immutable GitHub Releases and are addressed by the `asset` URL and verified by the `sha256` field in each release record.

Each package has this layout:

```text
<package>/
  owner.txt
  versions.txt
  <version>.toml
```

`versions.txt` contains one SemVer per line. `<version>.toml` is registry metadata format 1 and must contain the package name, GitHub source repository, Flux compatibility requirement, immutable release asset, SHA-256 digest, dependency requirements, and yank state. Versions and metadata are immutable: publishing a new release adds a new version, while a broken release is marked `yanked = true` rather than replaced or removed. `owner.txt` is the explicit package-name ownership record used by the publisher.

`FLUX_REGISTRY_DIR` may select a checked-out/private/test index and `FLUX_REGISTRY_URL` may select another static HTTPS mirror. Metadata is cached by the compiler and exact lockfile entries remain usable offline, so registry resolution requires no database or always-on Flux service.

Validate an index locally, or in CI, with:

```text
flux registry validate <index-directory>
```

The command checks package ownership records, sorted version lists, matching release metadata, and rejects unlisted metadata files before publication.

Registry changes should be made by the publisher workflow after release and archive verification. Do not add mutable branch archives, install hooks, native build instructions, credentials, or generated build output here.
