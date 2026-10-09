# macOS distribution

The macOS app jobs in `.github/workflows/release.yml` require Developer ID
signing and Apple notarization for both Apple Silicon and Intel artifacts.
A certificate alone is insufficient for the Gatekeeper malware-check warning.
Existing downloaded releases are unchanged by workflow edits.

To replace only the macOS packages on an existing release, manually run the
Release workflow from the reviewed source ref and set `macos_release_tag` to
the existing tag. Its version must match `pubspec.yaml`. This mode builds both
architectures, publishes only verified DMG/ZIP files, preserves other platform
assets, and records the actual source commit in the release notes without moving
the tag. Leave the input empty to retain the normal draft-release workflow.
Increment the build number in both Flutter manifests before rebuilding packages.

## Repository configuration

Actions secrets (never commit these values):

- `MACOS_CERTIFICATE_P12_BASE64`: base64 PKCS#12 certificate **and private key**.
- `MACOS_CERTIFICATE_PASSWORD`: password protecting that export.
- `APPLE_NOTARY_PRIVATE_KEY`: App Store Connect API private key used by notarytool.

Actions variables:

- `MACOS_SIGNING_IDENTITY`: Developer ID Application certificate name or SHA-1.
- `APPLE_TEAM_ID`: expected certificate team.
- `APPLE_NOTARY_KEY_ID` and `APPLE_NOTARY_ISSUER_ID`: notarization API identity.

Only trusted release refs should use these credentials. They are supplied to
credential checking and packaging steps, not builds or pull request tests.
The packaging script imports credentials into a disposable keychain, restores
the previous search list and removes the keychain on exit. CI runners are ephemeral.

## Packaging and verification

`scripts/package_macos.py` copies the input app before changing it, checks the
requested architecture and library paths, signs nested code before the app
with secure timestamps and Hardened Runtime, and checks each code signature's
Developer ID identity and team. It submits an app archive to Apple, staples the
accepted ticket to the app, then creates and signs the DMG, notarizes/staples it,
and runs Gatekeeper checks. The downloadable ZIP is made from the stapled app.
An invalid, pending or timed-out notarization fails the job; no unsigned fallback
is uploaded. Notarization IDs/logs and hashes are retained as CI artifacts.

For a local build, set `MACOS_SIGNING_IDENTITY`, `APPLE_TEAM_ID`, and an existing
`APPLE_NOTARY_PROFILE`; optionally set `MACOS_SIGNING_KEYCHAIN` and
`APPLE_NOTARY_KEYCHAIN`. Then run from the repository root:

```sh
python3 scripts/package_macos.py \
  --app apps/flutter/build/macos/Build/Products/Release/pocket_codex.app \
  --architecture arm64 --output /absolute/output/pocket-codex-macos-arm64.dmg
python3 -m unittest discover -s scripts -p 'test_package_macos.py'
```

This is direct distribution, not Mac App Store submission. It does not remove
the normal first-open downloaded-app confirmation or grant access to protected
macOS resources. See [Apple notarization documentation](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).

## Release-only theme regression

The theme animation must not read RenderObject debug getters outside debug mode.
The native Profile test exercises the actual toggle and snapshot host with assertions
disabled (as in Release; Flutter Driver cannot attach to Release), without starting the bridge, a Codex host or network connections:

```sh
cd apps/flutter
fvm flutter drive --profile -d macos \
  --driver=test_driver/integration_test.dart \
  --target=integration_test/theme_toggle_native_test.dart
```

This builds a test harness. Rebuild the normal app before packaging a release.
