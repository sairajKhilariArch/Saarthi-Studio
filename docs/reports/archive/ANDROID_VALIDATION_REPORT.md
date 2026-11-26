# Sarthi Studio — Phase 8 Android Validation

**Date:** 2026-11-25
**Android app version:** 1.51.1 (version code 61)
**Branch:** `platform/phase-8-cross-platform-2026-11-25`

## Status summary

| State | Result |
|---|---|
| Implemented | Yes |
| Built | Debug APK, unsigned release APK and AAB produced by Android CI |
| Installed | Debug APK installed on the CI emulator by the instrumentation runner |
| Executed | Yes, API 35 x86_64 emulator |
| Tested | JVM unit tests, lint, instrumentation, APK/AAB assembly |
| Blocked | Physical device; local build (no JDK/Android SDK on this machine); manual rotation, keyboard, background and touch checks; release signing key |

## Existing architecture

The Android client is a single-activity Java app with its own SQLite store and OpenAI-compatible router transport. It does not embed the desktop shell, does not expose desktop filesystem access and does not execute shell commands. It has a separate optional Desktop Host transport over HTTPS with a bearer token.

- Local history: `ChatDatabase` + `DatabaseMigrations`, backup/restore, paged history and search.
- Credentials: AES-GCM key in Android Keystore (`SartiStudioSettingsKeyV1` alias); `allowBackup="false"`.
- Network: direct router or desktop host; retry/fallback policy in `NetworkPolicy`.
- Attachments: bounded text/image policy; speech input/output optional.

## Defects fixed in Phase 8

1. **Release signing contradicted the artifact contract.** `build.gradle.kts` signed the `release` build with the auto-generated debug keystore, producing `app-release.apk`, while `BUILDING.md`, README and CI expect `app-release-unsigned.apk`. The debug key is not a release identity. The signing config was removed; CI now uploads the unsigned release APK/AAB and signing happens with the project's release keystore at distribution.
2. **The network-security-config could not match LAN addresses.** It listed `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16` and similar CIDR values under `<domain>`. Android matches host names there, so the documented `http://192.168.1.10:3001/v1` setup was blocked while the source looked permissive. The platform now permits cleartext and `CleartextPolicy` enforces the boundary in code before every connection: localhost, loopback, RFC 1918, link-local and IPv6 unique-local are allowed; public HTTP is refused with an HTTPS instruction.
3. **Android 15 edge-to-edge had no inset handling.** With `targetSdk 35`, the header and composer could sit under the status/navigation bars. `applySystemBarInsets()` pads the root by the live system-bar and display-cutout insets on API 30+, with the legacy getters on API 26–29.
4. **Test coverage** for `CleartextPolicy` was added (loopback, all private ranges, public refusal, HTTPS, malformed octets).

## CI evidence

Workflow: **Android CI** run `45686438710` on this branch.
Job: **Test, lint, build, and verify Android** — **success**.

| Step | Result |
|---|---|
| JVM regression tests | success |
| Android lint | success — 0 errors, 51 warnings |
| Debug APK assembly | success |
| Instrumentation APK assembly | success |
| Release APK and AAB (R8, unsigned) | success |
| Debug APK upload | success |
| Release APK/AAB upload | success |
| Emulator API 35, x86_64, Pixel 2 profile | success |
| Instrumentation suite | success |
| Release validation gate | success |

Artifacts downloaded and inspected:

- `app-debug.apk` — 119 KB
- `app-release-unsigned.apk` — 69 KB
- `app-release.aab` — 123 KB
- `lint-results-debug.html` — 51 warnings, no errors

The instrumentation run executed **6 tests** (`OK (6 tests)`) covering application launch, database migration, history paging, conversation search and backup/restore, per the files under `android/app/src/androidTest`.

## Platform behaviour

- **Back button:** the root activity uses Android's default finish; dialogs are `AlertDialog`s that dismiss themselves. There is no custom screen stack to intercept, so no custom `OnBackInvokedCallback` was added.
- **Rotation:** handled by `configChanges="orientation|screenSize|keyboardHidden"`; the activity is not recreated. No manual rotation test was run.
- **Keyboard:** `windowSoftInputMode="adjustResize"` plus the new insets padding; not exercised on a device.
- **Safe areas:** implemented as above.
- **Background behaviour:** one-shot executor requests with an active connection that Stop can close; no background service or polling loop. Background/foreground transitions were not instrumented.
- **Network interruption:** retry and ordered model fallback are unit-tested; a real radio loss was not simulated.
- **Permissions:** only `INTERNET` and `ACCESS_NETWORK_STATE`; no storage, location, camera or shell permission.
- **Desktop-only features (terminal, project files, Git, LSP/DAP):** not exposed. The client presents its own chat/tools UI rather than fake desktop controls.

## Not tested / blocked

- Physical Android device, touch ergonomics, real keyboard, rotation and process death.
- Release signing with a production keystore; no key is available or committed. The CI artifact is intentionally unsigned.
- Play Store/AAB signing and upload.
- Local Gradle build in this environment: `java`, `adb`, `gradle` and the Android SDK are not installed, so the APK was built and executed only on GitHub's runner.
- 51 lint warnings were not individually triaged in this phase; lint gate passes with no errors.
