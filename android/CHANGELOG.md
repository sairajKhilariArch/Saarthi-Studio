# Changelog

## 1.19.0 — release packaging and visual parity

### Changed
- Version: `1.19.0`, version code `16`.
- The Android client keeps its own FreeLLMAPI/router connection and shared dark
  palette; the release pipeline now attaches its unsigned APK and AAB to the
  same GitHub release as the Linux, Windows and VS Code artifacts.

### Notes
- Android remains a companion client: it connects to a router (or the desktop
  host API) directly and cannot run Docker or the desktop workspace editor.

## 1.12.0 — Production release build

### Added
- Production `release` build type with R8 code shrinking/obfuscation and resource shrinking (`proguard-android-optimize.txt` plus an intentionally empty `proguard-rules.pro`; the app uses no reflection, JNI, or runtime serialization, so no keep rules are required).
- Scoped network security configuration replacing app-wide `android:usesCleartextTraffic="true"`: plaintext HTTP is now allowed only for loopback and private/link-local ranges (`localhost`, `127.0.0.1`, `::1`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `169.254.0.0/16`, `fd00::/8`). Every other router host must use HTTPS, matching the transport rule in `PLATFORM-ARCHITECTURE.md`.
- Adaptive launcher icon (vector background/foreground with a monochrome layer for Android 13+ themed icons), replacing the default system icon. minSdk 26 covers every adaptive-icon-capable device, so no density PNG fallbacks are needed.
- CI now assembles the unsigned release APK and AAB alongside the existing debug gates and uploads them as the `saarthi-studio-android-release-unsigned` artifact; the release build is part of the required validation gate.

### Changed
- `targetSdk` raised from 34 to 35 to match `compileSdk` (Google Play has required API 35 since August 31, 2025).
- Removed the stale `build-android.yml` workflow, which triggered on a nonexistent `android` branch and ran Gradle from the repository root where no Gradle project exists. `android-ci.yml` (now named "Android CI") is the single Android workflow.
- Version: `1.12.0`, version code `15`.

### Release gate
- No signing configuration is committed: release artifacts are unsigned (`app-release-unsigned.apk`, `app-release.aab`) and are signed at distribution time.
- Physical-device upgrade, LAN-plaintext, and public-HTTPS router tests remain required before production distribution.

## CI follow-up — Android instrumentation reliability

### Fixed in CI configuration
- Remove the trailing whitespace reported in `VALIDATION-REPORT.md` by the committed-whitespace guard.
- Run the AndroidJUnitRunner directly after installing the app and test APKs, parsing per-test runner status markers rather than trusting the `am instrument` shell exit code.
- Capture instrumentation output, Android logcat, process state, and memory details into an absolute workspace path before the emulator is torn down.
- Preserve the final release-validation gate; an empty test run, process crash, assertion failure, or install failure still fails CI.

## 1.11.1 — CI regression and instrumentation-test safety correction

### Fixed
- Improve Android instrumentation failure diagnostics by collecting logcat and emulator process/memory state before emulator teardown.
- Ensure pull requests targeting `main` run Android validation as well as pull requests targeting `android`.
- Keep whitespace validation strict while allowing tests and build diagnostics to run before the final CI gate.
- Separate application APK assembly from instrumentation APK assembly so a test APK failure does not hide an otherwise buildable debug APK.
- Correct the app-launch smoke test to verify the visible message composer `EditText` hint without relying on `View.isShown()`.
- Add a package-private database-name constructor for isolated instrumentation fixtures while preserving `saarthi_history.sqlite3` as the production default.
- Move backup/restore and history-paging instrumentation tests to unique test-only databases. Their setup and cleanup no longer delete the user's production chat-history database.
- Keep the production database schema and installed app version unchanged; the patch corrects validation and test-data safety without changing user-facing features.

## 1.11.1 — Stabilization and regression coverage

### Fixed
- Moved API-27-only `windowLightNavigationBar` into API 27+ resources, preserving minSdk 26.
- Redact bearer tokens/API-key-like values from saved diagnostic details.
- Ensure APK assembly and report upload can run for diagnosis even when lint or unit tests fail; the workflow still ends in failure until each gate passes.

### Refactored
- Extract SQLite persistence and data records into `ChatDatabase.java`.
- Extract database migrations, network retry/fallback rules, attachment limits, backup validation/fingerprinting, search SQL/preview handling, and text-to-speech lifecycle into focused classes.
- Make backup chat/message/preset writes and backup fingerprint insertion part of one database transaction.
- Skip restoring the same byte-for-byte JSON backup file more than once.
- Advance SQLite from schema v5 to v6 to record imported backup fingerprints.

### Regression tests and CI
- JVM tests cover v1/v2/v3 backup versions, byte limits, attachment allowlists/budgets, retry/fallback handling including partial output, diagnostic redaction, and search parameter binding.
- Android instrumentation tests exercise v4→v5→v6 migrations and preservation of conversation/message IDs and custom presets, plus a main activity launch smoke test.
- CI uploads a built debug APK when assembly succeeds, uploads a lint report for review, and runs instrumentation tests on an Android emulator.

### Release gate
- Version: `1.11.1`, version code `14`.
- A successful CI run and physical-device upgrade, FreeLLMAPI, voice, vision, and backup/restore tests remain required before production distribution.
