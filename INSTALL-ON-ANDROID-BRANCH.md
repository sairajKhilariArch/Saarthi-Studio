# Install this full project on the `android` branch

This archive contains a complete standalone Android project, not a patch-only archive. It includes source, manifest, resources, Gradle configuration, the APK build workflow, README, changelog, and build instructions.

## Before copying

Use a local clone at `~/Projects/Saarthi-Studio`. Make sure any local work you need is committed or backed up.

```bash
cd ~/Projects/Saarthi-Studio
git switch android
git status --short
```

If `git status --short` lists work you still need, commit or stash it before replacing project files.

## Back up and copy the full project

Assuming you extracted this ZIP into `~/Downloads/Saarthi-Studio-Android-1.6.0`:

```bash
cd ~/Projects/Saarthi-Studio
tar -czf ~/Saarthi-Studio-android-before-1.6.0.tgz --exclude=.git --exclude=.gradle --exclude='*/build' .

# Install rsync if the command is missing: sudo apt install rsync
rsync -av --delete \
  --exclude='.git/' \
  --exclude='.gradle/' \
  --exclude='**/build/' \
  ~/Downloads/Saarthi-Studio-Android-1.6.0/ ./
```

The `--delete` option makes the Android branch match this standalone project snapshot and removes old duplicate or misplaced source files that could otherwise break compilation. It preserves the Git database and build/cache directories excluded above. Do not use this procedure on `main` or on a branch that contains desktop/Apple source code.

## Review, commit, and push

```bash
git status --short
git diff --check
git diff --stat
```

Review the changes, then:

```bash
git add -A
git commit -m "Add conversation search and full Android project"
git push origin android
```

The push should trigger `.github/workflows/build-android.yml`. When the workflow succeeds, download the `saarthi-studio-android-debug-apk` artifact from GitHub Actions.

An in-place app update should preserve phone data when the application ID and signing certificate are compatible. Back up important chats first. Do not uninstall the current app to bypass a signing-certificate mismatch, because uninstalling usually deletes its private local history and settings.

Do not commit API keys, FreeLLMAPI `.env` files, router databases, signing keys, or local settings.
