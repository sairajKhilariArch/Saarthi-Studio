# Changelog

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
