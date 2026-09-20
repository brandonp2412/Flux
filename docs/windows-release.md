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

For a release-signed package, use the publishing command with the publisher
certificate and the exact publisher identity used in the manifest:

```text
flux publish windows path/to/package \
  --target x86_64-pc-windows-gnu \
  --certificate publisher.pfx \
  --publisher "CN=Example Publisher"
```

`flux publish windows` always produces a release-mode MSIX, stages the unsigned
package, invokes Windows `signtool` with SHA-256, verifies the signed result, and
only then reports a successful publish artifact. Use `-o path/to/app.msix` for a
deterministic artifact path. The lower-level `flux package --format msix` command
remains available when an unsigned package is useful for local inspection or when
signing is orchestrated separately.

Before creating or staging the package, Flux verifies that the certificate path
exists, is a regular non-empty file, and can be opened. It does not inspect or
silently repair certificate identity metadata; `signtool` remains responsible
for validating the PFX contents and signature.

The `--publisher` value is checked before packaging: it must be the exact
certificate-subject-shaped value beginning with `CN=`, without surrounding
whitespace or control characters. Keep it identical to the subject used by the
PFX certificate; Flux does not silently rewrite certificate identity metadata.

## CI / automation

Use `--json` when another tool needs a stable stdout contract:

```text
flux publish windows . \
  --certificate publisher.pfx \
  --publisher "CN=Example Publisher" \
  -o dist/my-app.msix \
  --json
```

Successful JSON output contains the artifact path, Windows target triple,
publisher identity, package format, release build mode, signing status, and
verification status. Command failure uses the normal non-zero process status.

A Windows release runner therefore needs the Flux compiler/toolchain, the Windows
SDK `signtool`, and the PFX supplied from a protected secret or secure runner
location. Do not commit production signing material to the repository. The
Windows CI self-test separately requires the Windows SDK `MakeAppx` unpacker to
accept a generated MSIX, which catches package-container regressions independently
of certificate availability.

## Microsoft Store boundary

Flux prepares and verifies the signed MSIX artifact. Publisher-account creation,
certificate issuance/provisioning, Partner Center application identity, Store
listing and policy metadata, certification submission, rollout, and release are
account-owned Microsoft Store operations. They are intentionally not performed
implicitly by the compiler: those steps require release-owner credentials,
commercial/account choices, and Store-side state that does not belong in a
reproducible source build.
