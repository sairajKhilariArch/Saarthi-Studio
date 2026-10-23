# Building Saarthi Studio Android

## Toolchain
- JDK 17
- Android Gradle Plugin 8.7.3
- Gradle 8.9
- Android SDK Platform 35 and Build Tools 35.0.0

## Local build and checks
From the project root:

```bash
./BUILD-APK-UBUNTU.sh
```

The helper runs unit tests, lint, and APK assembly as separate phases, so a lint failure does not prevent an otherwise buildable APK from being produced for diagnosis. Its exit status remains nonzero if any required phase fails. The debug APK should be created at `app/build/outputs/apk/debug/app-debug.apk`; all checks must pass before the build can be called verified.

## GitHub Actions
Push to the `android` branch, or run the `Build Saarthi Studio Android APK` workflow manually in the GitHub Actions tab. It runs the unit-test task, Android lint, and debug APK build, then publishes the APK as a 30-day Actions artifact. A green build does not replace physical-device tests for network, speech, image input and SQLite migration.


## Release build (production artifacts)

```bash
cd android
gradle assembleRelease bundleRelease
```

- The release build enables R8 minification and resource shrinking. Rules live in `app/proguard-rules.pro` (currently none are needed; add one only with a regression test proving it).
- No signing configuration is committed, so both artifacts are unsigned: `app/build/outputs/apk/release/app-release-unsigned.apk` and `app/build/outputs/bundle/release/app-release.aab`. Sign them with your own keystore at distribution time (or use Play App Signing for the AAB).
- Plaintext HTTP is restricted to loopback and private/link-local ranges by `app/src/main/res/xml/network_security_config.xml`; router endpoints on any other host must use HTTPS.
- CI (`android-ci.yml`) runs unit tests, lint, debug and release assembly on every push/PR touching `android/**`, runs instrumentation tests on an API 35 emulator, and uploads the debug APK plus the unsigned release APK/AAB. The workflow fails if any required gate fails.

## Regression checks

Run `./BUILD-APK-UBUNTU.sh` locally; it runs unit tests, lint, debug APK assembly, and release APK/AAB assembly as separate phases and fails if any phase fails. The GitHub Actions workflow runs database migration, backup-restore, search, bounded-history, and activity-launch instrumentation tests on an API 35 emulator and uploads the lint report for review. A green build does not replace physical-device tests for network, speech, image input and SQLite migration.
