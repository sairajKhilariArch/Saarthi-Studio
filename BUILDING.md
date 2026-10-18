# Building Saarthi Studio Android

## Toolchain
- JDK 17
- Android Gradle Plugin 8.7.3
- Gradle 8.9
- Android SDK Platform 35 and Build Tools 35.0.0

## Local build and checks
From the project root:

```bash
gradle --no-daemon testDebugUnitTest lintDebug assembleDebug
```

The debug APK should be created at `app/build/outputs/apk/debug/app-debug.apk`. This command has to complete successfully in an Android SDK environment before the build can be called verified.

## GitHub Actions
Push to the `android` branch, or run the `Build Saarthi Studio Android APK` workflow manually in the GitHub Actions tab. It runs the unit-test task, Android lint, and debug APK build, then publishes the APK as a 30-day Actions artifact. A green build does not replace physical-device tests for network, speech, image input and SQLite migration.
