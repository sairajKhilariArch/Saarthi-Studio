# Upgrade Saarthi Studio Android from 1.2.0 to 1.3.0

Version 1.3.0 adds a model chooser to Router settings. Tap **Load available models** to request IDs from FreeLLMAPI's OpenAI-compatible `GET /v1/models` endpoint. Choose `auto` for router-managed selection or pick an ID returned by the server. The text field remains editable if you prefer manual entry.

## Apply the update to your existing repository

1. Download and extract this archive.
2. Open Terminal and switch the local repository to the Android branch:

   ```bash
   cd ~/Projects/Saarthi-Studio
   git switch android
   git pull --ff-only origin android
   ```

3. From this archive's extracted `saarthi-studio-android` directory, run:

   ```bash
   ./apply-model-picker-upgrade.sh ~/Projects/Saarthi-Studio
   ```

4. Review and push:

   ```bash
   cd ~/Projects/Saarthi-Studio
   git diff --check
   git diff -- app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
   git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
   git commit -m "Add FreeLLMAPI model picker"
   git push origin android
   ```

5. Open the repository's **Actions** tab. Once the **Build Saarthi Studio Android APK** workflow succeeds, download the `saarthi-studio-android-debug-apk` artifact and extract `app-debug.apk`.

## Notes

- The update changes only `MainActivity.java` and `app/build.gradle.kts`.
- It does not alter the SQLite schema, saved conversations, or encrypted settings format.
- Keep the existing app installed while upgrading. If Android reports a signing-certificate conflict, do not uninstall the current app without backing up your chats and settings.
- The model picker relies on the router implementing `GET /v1/models`. If no model list is returned, the existing manual model entry still works.
- The source package has not been compiled with an Android SDK in this environment; the GitHub Actions build is needed to validate compilation.
