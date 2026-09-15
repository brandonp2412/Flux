# Windows release workflow

Flux produces a Windows MSIX package with:

```text
flux package path/to/package --format msix --target x86_64-pc-windows-gnu
```

The package contains the native executable, staged assets, a versioned full-trust
`AppxManifest.xml`, `[Content_Types].xml`, and an AppX block map with SHA-256
hashes for every packaged file. Archive entry order is deterministic and ZIP
metadata is stripped so unsigned artifacts are reproducible for a fixed native
toolchain.

For a release-signed package, provide a publisher certificate and the exact
publisher identity used in the manifest:

```text
flux package path/to/package --format msix \
  --target x86_64-pc-windows-gnu \
  --certificate publisher.pfx \
  --publisher "CN=Example Publisher"
```

Flux stages the unsigned package, invokes Windows `signtool` with SHA-256, and
verifies the signed result before replacing the output. Certificate provisioning,
publisher identity registration, Store listing, Store validation, and submission
remain release-owner actions performed with Microsoft tooling and accounts.

The `--publisher` value is checked before packaging: it must be the exact
certificate-subject-shaped value beginning with `CN=`, without surrounding
whitespace or control characters. Keep it identical to the subject used by the
PFX certificate; Flux does not silently rewrite certificate identity metadata.
