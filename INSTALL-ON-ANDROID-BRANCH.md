# Install this project on the `android` branch

This is a complete standalone Android project at version **1.11.0 / versionCode 13**, not a patch-only archive. The combined source preserves the v1.10.0 features and adds v1.11.0 reliability/model preferences, quota inspection, chat organization, expanded developer attachments, voice, and limited image input. The source is integrated in `app/src/main/java/com/saarthi/studio/MainActivity.java` plus pure-Java release rules. SQLite schema v5 preserves existing chats while adding organization metadata.

## Back up first

On your Ubuntu/Linux machine, make sure the local repository is checked out to the Android branch. Review uncommitted work before replacing files.

```bash
cd ~/Projects/Saarthi-Studio
git switch android
git status --short
```

If you have local changes you need, commit or stash them before proceeding. Make a backup of the repo files and export chats from the installed app before installing a newly built APK.

## Copy the project files

If you extracted this ZIP into `~/Downloads/Saarthi-Studio-Android-1.11.0`:

```bash
cd ~/Projects/Saarthi-Studio
tar -czf ~/Saarthi-Studio-android-before-1.11.0.tgz --exclude=.git --exclude=.gradle --exclude='*/build' .
rsync -av \
  --exclude='.git/' \
  --exclude='.gradle/' \
  --exclude='**/build/' \
  ~/Downloads/Saarthi-Studio-Android-1.11.0/ ./
```

This copies the supplied project files without deleting extra files in your checkout. Keep this on the `android` branch; do not overwrite `main` or a branch containing desktop/Apple source code.

## Review, build, and push

```bash
git diff --check
git diff --stat
./BUILD-APK-UBUNTU.sh
```

The build script requires JDK 17, Gradle 8.9, Android SDK Platform 35, and Build Tools 35.0.0. If the local build succeeds, commit and push:

```bash
git add -A
git commit -m "Add Saarthi Studio 1.11.0 features"
git push origin android
```

The repository's workflow runs unit tests, Android lint, and a debug APK build on a push to `android` touching app, tests, Gradle, documentation, or workflow files. It uploads `saarthi-studio-android-debug-apk`. Confirm the Actions run succeeds before installing the APK.

## Update warning

An in-place app update preserves private app data only when the application ID and signing certificate are compatible. Debug APKs built on different machines can use different debug certificates. If Android refuses the update because of a signature mismatch, do not uninstall the current app just to force installation; export chats first because uninstalling usually deletes private app data.

Do not commit API keys, FreeLLMAPI `.env` files, router databases, signing keys, or local settings. Use plain HTTP only over a trusted private LAN/hotspot; use HTTPS for remote/public connections.
