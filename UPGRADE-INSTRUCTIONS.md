# Upgrade the existing Android branch to Saarthi Studio 1.1.0

This update adds separate conversations, history browsing, new chat, rename, delete, and a database migration from the existing single-chat history.

## Apply the update to an existing local clone

1. Extract this archive.
2. Open a terminal in the extracted `saarthi-studio-android` folder.
3. Make sure your local repository is cloned and the `android` branch is checked out. Example:

   ```bash
   git -C ~/Projects/Saarthi-Studio switch android
   ```

4. Run the included updater (replace the path if your clone is elsewhere):

   ```bash
   ./apply-chat-management-upgrade.sh ~/Projects/Saarthi-Studio
   ```

   It updates only these files in the clone:
   - `app/src/main/java/com/saarthi/studio/MainActivity.java`
   - `app/build.gradle.kts`
   - `.github/workflows/build-android.yml`

5. Review and push the changes:

   ```bash
   cd ~/Projects/Saarthi-Studio
   git diff --check
   git diff
   git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts .github/workflows/build-android.yml
   git commit -m "Add conversation management to Android app"
   git push origin android
   ```

6. Open the repository's Actions tab and run **Build Saarthi Studio Android APK** on the `android` branch. When it succeeds, download the `saarthi-studio-android-debug-apk` artifact and extract `app-debug.apk`.

## What is included

- Each chat gets its own conversation ID and message history.
- **Chats** lists conversations and shows a message preview.
- **+ New chat** starts a separate conversation.
- Conversations are auto-titled from the first prompt and can be renamed or deleted.
- The existing v1.0.0 flat message history is migrated to a conversation named **Previous conversation** when the app is updated in place.
- The app version is bumped to 1.1.0 / versionCode 2.
- The Android Actions workflow avoids the obsolete `tools` SDK package and watches the `android` branch.

## Important data/signing note

Do not uninstall your existing app before attempting an in-place update; uninstalling removes the app-private chat database and settings. If Android reports that the new APK's signing certificate differs from the installed app, stop instead of uninstalling and decide how to retain the local data. GitHub-hosted debug builds may not use the same debug signing key across different runs.

The source was checked for Java syntax-pattern errors and the SQLite v1-to-v2 migration was simulated successfully. A full Android Gradle compilation has not been run in this environment because the Android SDK is unavailable here.
