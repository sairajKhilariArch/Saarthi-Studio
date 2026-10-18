# Saarthi Studio v1.11.1 — Full Project Package Notes

This archive contains the complete Android project source and the tracked top-level files from the `android` branch snapshot at commit `98609b38b3a6e08b7c258ac11d52d983d51bb2da` (v1.11.1). It does not include Git history (`.git`) or a built APK.

## Project contents

- Android application source, resources, manifest, Gradle configuration, unit tests, and instrumentation tests.
- GitHub Actions workflow for Android test/lint/build validation and APK artifact upload.
- Build and upgrade documentation, changelog, license, and historical migration notes/scripts that remain tracked in the repository.
- One whitespace-only correction in `app/src/main/java/com/saarthi/studio/ChatDatabase.java` removes the extra blank line at EOF that caused the latest remote CI run to fail its `git show --check` step.

## Important notes

- The top-level `install.sh` is a historical Linux installer for the separate Rust Free AI Suite; it is **not** the Android APK build script. Do not run it to build the Android app.
- For the Android project, use `BUILD-APK-UBUNTU.sh` or open this folder in Android Studio.
- Required toolchain: JDK 17, Gradle 8.9, Android SDK Platform 35, and Android Build Tools 35.0.0.
- A previous GitHub Actions run stopped at a trailing-blank-line check before it could run the tests/lint/build steps. This package fixes that reported issue but is not yet proof that the remaining checks pass.
- Back up chats before upgrading an installed app. Install over the existing app with a compatible signing key; uninstalling may delete local history and settings.

## Suggested first checks

```bash
git diff --check
bash -n ./*.sh
./BUILD-APK-UBUNTU.sh
```

The last command requires the Android SDK environment and Gradle to be configured. Resolve any new build or lint reports before treating v1.11.1 as release-ready.
