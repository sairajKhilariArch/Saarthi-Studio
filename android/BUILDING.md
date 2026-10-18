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


## v1.11.1 regression checks

Run `./BUILD-APK-UBUNTU.sh` locally. The GitHub Actions workflow runs database migration, backup-restore, search, bounded-history, and activity-launch instrumentation tests on an API 35 emulator, uploads the lint report for review, and uploads the APK if APK assembly succeeds. The workflow still fails if any required validation gate fails.
