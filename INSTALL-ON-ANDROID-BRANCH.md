# Install Saarthi Studio Android 1.11.1 onto the `android` branch

This archive is a complete Android source project for version **1.11.1 / versionCode 14**. It keeps `minSdk = 26`, uses Java 17, and includes the release stabilization refactor and regression tests. It is a source package, not a prebuilt or device-verified APK.

## Before replacing files

1. Run `git switch android` and `git pull --ff-only origin android` in your existing repository.
2. Back up the installed app's chats as JSON. Keep a second copy outside the phone.
3. Back up the repository working tree with Git, or use a separate fresh clone.
4. Extract this archive and review the complete files before copying them into the Android project root.

## Apply and verify

Copy the extracted project contents to the root of your repository checkout. Do not copy the extracted directory as a nested subfolder. Then run:

```bash
git diff --check
./BUILD-APK-UBUNTU.sh
```

If any command fails, fix the reported issue instead of committing an unverified build. Once JDK 17, Gradle 8.9, Android SDK Platform 35, and Build Tools 35.0.0 are available, the expected debug APK is `app/build/outputs/apk/debug/app-debug.apk`.

## Commit and push

```bash
git status --short
git add -A
git commit -m "fix(android): stabilize v1.11.1 and add regression coverage"
git push origin android
```

GitHub Actions should run JVM regression tests, lint, debug APK assembly, and emulator instrumentation tests. It uploads the APK after a successful assembly and uploads the lint report for inspection. The workflow remains failed if any required gate fails.

## Manual acceptance still required

Install the APK **over** the existing v1.10.0/v1.11.0 app without uninstalling first. Confirm chat history and custom prompt presets remain intact. Then test FreeLLMAPI connect/stream/Stop/Retry/fallback, voice services, image input on a vision-capable model, and backup/restore using schema v1/v2/v3 files. This physical-device validation cannot be inferred from a successful compilation alone.

Keep FreeLLMAPI limited to your trusted hotspot/LAN; do not expose the server port or unified key publicly.
