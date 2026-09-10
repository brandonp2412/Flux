# Android release publishing

Flux can prepare a Google Play upload bundle without Gradle or application-authored Java/Kotlin/JNI:

```sh
export FLUX_ANDROID_KEYSTORE_PASSWORD='...'
export FLUX_ANDROID_KEY_PASSWORD='...'
flux publish android .
```

`FLUX_ANDROID_KEY_PASSWORD` is optional when the key password matches the keystore password. Keep both values in the CI secret store or shell environment; do not commit them to `flux.toml`.

## Package configuration

A publishable package needs explicit version and upload-key metadata:

```toml
[package]
name = "my-app"
version = "1.0.0"
entry = "src/main.flux"

[android]
application_id = "nz.example.myapp"
version_code = 1
target_sdk = 36
keystore = "signing/upload.jks"
key_alias = "upload"
```

The keystore path may be relative to the package root or absolute. `flux publish android` never falls back to Flux's development key. It requires an explicit `[package].version`, a configured keystore/key alias, signing credentials in the environment, and `target_sdk >= 36` for the current phone/tablet publishing workflow.

Google Play requires new phone/tablet apps and updates submitted from August 31, 2026 to target Android 16 / API 36 or higher: <https://developer.android.com/google/play/requirements/target-sdk>. New Play apps use Android App Bundles and Play App Signing: <https://developer.android.com/guide/app-bundle> and <https://developer.android.com/guide/app-bundle/faq#play_app_signing>.

## What the command verifies

`flux publish android` always builds a release AAB containing all Flux-supported Android ABIs. It signs the bundle with the configured release/upload key, runs `bundletool validate`, and then verifies the JAR signature with `jarsigner -verify`. The verifier intentionally does not use `jarsigner -strict`: Android upload keys are normally self-signed, and strict JAR verification treats the expected lack of a public CA chain as an error. A failed compile, target-API policy check, missing signing credential, invalid bundle, or signature verification returns a non-zero exit status and leaves no successful publish result.

By default the bundle is written below `build/android/release/`. Use `-o path/to/app.aab` to choose a deterministic CI artifact path.

## CI / automation

Use `--json` when another tool needs a stable stdout contract:

```sh
flux publish android . -o dist/my-app.aab --json
```

Successful output is a single JSON object containing the artifact path, application ID, version name, version code, target SDK, format, build mode, and verification status. Human build progress is suppressed from stdout in this mode, and command failure is reported through the normal non-zero process status.

A minimal CI job therefore only needs the Flux/Android toolchain, `bundletool`, the upload keystore, and the two signing environment variables. Store the keystore itself as a protected CI secret or provision it from a secure runner location; do not commit production signing material to the repository.

Flux produces the signed upload AAB. Creating the Play Console application, store listing, policy declarations, rollout track, review submission, and Play-side release are intentionally external account actions and are not performed implicitly by the compiler.
