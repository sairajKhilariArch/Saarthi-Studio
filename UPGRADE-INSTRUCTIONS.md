# Saarthi Studio 1.5.0 — Backup & Export Upgrade

## Features

- Export the current chat as Markdown (`.md`).
- Export all chats as a portable JSON backup (`.json`). The backup contains conversation titles, message roles, message contents, and display metadata. It deliberately excludes the FreeLLMAPI URL, API key, and other settings.
- Restore a Saarthi Studio JSON backup. Restored conversations are appended; existing chats are not deleted. Importing the same backup more than once creates duplicates.
- Backup restore validates the format, limits the selected file to 10 MB and keeps the import in a database transaction to avoid partial imports.

## Apply on Ubuntu

Extract this ZIP. In a terminal, update the local Android branch and run the script from this extracted directory:

```bash
cd ~/Projects/Saarthi-Studio
git switch android
git pull --ff-only origin android
# From the extracted saarthi-studio-android-backup-export folder:
./apply-backup-export-upgrade.sh ~/Projects/Saarthi-Studio
```

Then review and commit the changes:

```bash
cd ~/Projects/Saarthi-Studio
git diff --check
git diff --stat
git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
git commit -m "Add chat backup and export"
git push origin android
```

Run **Build Saarthi Studio Android APK** in GitHub Actions on the `android` branch. Install the new APK over the existing app only after the build succeeds and only if it is signed with the same key. Do not uninstall the app to upgrade; that can delete local chat history and settings.

## Use

Tap **Backup** in the chat header, then choose Export current conversation, Export all chats, or Restore chats from backup. Android will ask where to save or which file to open.
