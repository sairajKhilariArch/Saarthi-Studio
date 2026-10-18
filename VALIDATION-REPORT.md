# Saarthi Studio Android 1.11.0 — Validation Report

- Package: `com.saarthi.studio`
- App version: `1.11.0`, `versionCode = 13`
- Baseline: Saarthi Studio Android v1.10.0 source package
- SQLite schema: v5; migration adds conversation folder, tags, pinned, and favorite metadata
- Backup writer: schema v3; importer accepts schemas v1, v2, and v3

## Checks completed in this environment

- `AndroidManifest.xml` parsed successfully as XML.
- `.github/workflows/build-android.yml` parsed successfully as YAML.
- `ReleaseRules.java` compiled with the locally installed `javac`.
- 12 direct Java smoke assertions passed for ordered model fallback, model-list normalization, fallback decisions, folder/tag sanitation and caps, attachment allowlists, and vision-model name heuristics.
- A SQLite v4→v5 migration simulation passed and retained the existing conversation/message rows while adding organization-column defaults.
- The conversation-search SQL was reviewed for parameter count and exercised against a SQLite schema simulation.
- `javac` reported no common Java syntax diagnostics for `MainActivity.java`, but produced errors for unavailable Android framework and `org.json` types. This is a source-level syntax scan only, not an Android compile.
- Git diff whitespace scan reported no whitespace diagnostics for the modified main Java file.

## Checks not completed

This environment does not contain Gradle or an Android SDK (`ANDROID_HOME` and `ANDROID_SDK_ROOT` are unset). Consequently, the following have **not** been run here:

- Android Gradle compilation, Android Lint, or the JUnit `testDebugUnitTest` task
- APK assembly
- Emulator or physical-device execution
- Live FreeLLMAPI streaming, fallback, quota endpoint, voice recognition/TTS, or vision request tests
- Actual installation-over-v1.10.0 database migration on an Android device

The project includes JUnit tests for the pure-Java rules and the GitHub Actions workflow invokes `testDebugUnitTest`, `lintDebug`, and `assembleDebug`. Treat this package as source code pending those checks, not as a verified APK.

## Before release

1. Back up chats from the installed v1.10.0 app.
2. Open the project in Android Studio or run `./BUILD-APK-UBUNTU.sh` with JDK 17, Android Gradle Plugin 8.7.3, Gradle 8.9, Android SDK Platform 35, and Build Tools 35.0.0.
3. Resolve any compilation/lint/test issues reported by the actual Android toolchain.
4. Test upgrade-in-place, saved chats, preset import, streaming/Stop/retry, folders/tags/search, quota endpoint unavailable handling, voice permissions, and image support against the user's FreeLLMAPI setup.
5. Push only after local review; confirm the `android` branch workflow succeeds before distributing its APK artifact.

This package has **not** been pushed to GitHub, and no APK is included.
