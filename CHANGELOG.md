# Changelog

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
