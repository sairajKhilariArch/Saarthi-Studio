# Upgrade Saarthi Studio Android to 1.2.0

This update adds basic Markdown formatting, fenced code cards, and one-tap copy actions. It does not change SQLite or FreeLLMAPI settings.

## Apply changes to your local Android branch

1. Extract this archive.
2. Ensure your local repository clone is on `android`:

   ```bash
   git -C ~/Projects/Saarthi-Studio switch android
   ```

3. From this extracted project directory, run:

   ```bash
   ./apply-markdown-code-upgrade.sh ~/Projects/Saarthi-Studio
   ```

4. Review and push:

   ```bash
   cd ~/Projects/Saarthi-Studio
   git diff --check
   git diff -- app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
   git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
   git commit -m "Improve Markdown and code copying in Android chat"
   git push origin android
   ```

5. Go to the repository Actions tab and run **Build Saarthi Studio Android APK** on `android`, or wait for the push trigger. Download `saarthi-studio-android-debug-apk` after it succeeds.

## What's changed

- Basic formatting for Markdown headings, bullet lists, bold, italics, and inline code.
- Fenced code blocks use a monospaced horizontally scrollable card.
- **Copy code** copies only the code inside a fenced block.
- **Copy response** copies the full assistant reply, including original Markdown.
- Bumped Android version to 1.2.0 / versionCode 3.

The Android source was reviewed for syntax consistency, but a full Gradle compile still needs to run on GitHub Actions or an Android SDK-equipped machine. Do not uninstall the current app while updating; an in-place update retains app-private chat history and settings when the signature is compatible.
