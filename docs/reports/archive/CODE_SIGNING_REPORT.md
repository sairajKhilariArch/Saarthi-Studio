# Code Signing and Trust Report — Phase 9

**Version:** 3.88.15
**Commit:** `0af8acb`
**State:** **BLOCKED for every production signing identity**

## 1. Summary

| Platform | Required control | State | Evidence |
|---|---|---|---|
| Windows | Authenticode signature + timestamp | **BLOCKED** | No `.pfx`/certificate, no timestamp URL, no `signtool` invocation in any workflow |
| Windows | Signature verification | **BLOCKED** | No signed MSI/zip exists to verify |
| macOS | Developer ID Application signature | **BLOCKED** | No Apple Developer identity or keychain profile |
| macOS | Hardened runtime + entitlements | **NOT CONFIGURED** | No `codesign --options runtime`, no entitlements file |
| macOS | Notarization | **BLOCKED** | No App Store Connect API key / notarytool credentials |
| macOS | Stapling + Gatekeeper (`spctl`) | **BLOCKED** | Nothing notarized |
| Android | Release keystore signature | **BLOCKED** | `android/app/build.gradle.kts` has no release `signingConfig`; CI builds `app-release-unsigned.apk` |
| Android | `apksigner verify` | **BLOCKED** | No signed release APK |
| Linux | Signed repository metadata / manifest | **BLOCKED** | No GPG key or signing step; `SHA256SUMS.txt` is checksum-only |

No certificate, private key, keystore, password or notarization credential is
committed, printed or requested in chat. The repository contains no signing
secrets.

## 2. What the pipeline does instead

- CI builds the real unsigned artifacts and validates their structure.
- Android's job was corrected: it no longer claims a debug-signed release and
  copies `app-release-unsigned.apk` explicitly.
- `smoke-apk.sh` requires a signature by default; CI sets
  `SMOKE_ALLOW_UNSIGNED=1` and prints:
  `UNSIGNED diagnostic build (allowed by SMOKE_ALLOW_UNSIGNED; not installable as production)`.
- `release-manifest.json` records all five signing properties as `false`.
- `check-publish-gates.sh` re-validates the bundle and exits **3 BLOCKED** if any
  signing property is not `true`. The `publish` job calls it before creating a
  release. Therefore an unsigned bundle cannot be published by this workflow,
  even if an operator dispatches with `publish=true`.

Local gate output on a complete 14-file unsigned fixture:

```
BLOCKED: the bundle is complete and verified, but publication is refused because:
  BLOCKED  Windows Authenticode signature
  BLOCKED  macOS Developer ID signature
  BLOCKED  macOS notarization and stapling
  BLOCKED  Android release keystore signature
  BLOCKED  signed Linux release manifest
PUBLISH_GATE=3
```

This is the correct outcome, not a pipeline failure to hide.

## 3. Signature verification status

| Verification | Result |
|---|---|
| Windows `signtool verify /pa` | Not run — no signed artifact |
| macOS `codesign --verify --deep --strict` | Not run — no signed app |
| macOS `spctl --assess` | Not run — no notarized app |
| Android `apksigner verify --print-certs` | Not run — unsigned APK |
| Linux `gpg --verify` | Not run — no detached signature |

No claim of signed distribution is made anywhere in the UI, README or release
notes. `RELEASE_NOTES.md` explicitly states Windows Authenticode and macOS
Developer ID/notarization are not configured.

## 4. What would unblock each row

| Platform | Credentials / infrastructure needed |
|---|---|
| Windows | Organization-validated Authenticode certificate, protected CI secret, timestamp service URL, `signtool` step before upload, `signtool verify` gate, manifest status set from the verified result |
| macOS | Apple Developer Program membership, Developer ID Application certificate in a temporary keychain, hardened-runtime entitlements, `notarytool` API key, `stapler`, `spctl` gate, manifest status set from the verified result |
| Android | Release keystore generated outside the repo, base64/secret-store upload, Gradle `signingConfig` from environment, `apksigner verify` gate, AAB upload key for Play |
| Linux | Project GPG key, detached signature over `SHA256SUMS.txt`/manifest, public key publication, `gpg --verify` gate |

The pipeline must also stop hard-coding signing status as `false`; the finalize
job should consume per-platform verification results. That wiring is
intentionally not faked in this phase.

## 5. Risk statement

An unsigned package can be modified in transit or replaced on a mirror without
detection by the operating system. SHA-256 checksums detect corruption relative
to the published file, but they do not establish who published it. Users should
treat current Windows/macOS/Android artifacts as test builds, not trusted
production installers.

**Publication gate: BLOCKED. No public release was created.**
