# Saarthi Studio 1.5.0 — Backup & Export Upgrade

## Features

- Export the current chat as Markdown (`.md`).
- Export all chats as a portable JSON backup (`.json`). The backup contains conversation titles, message roles, message contents, and display metadata. It deliberately excludes the FreeLLMAPI URL, API key, and other settings.
- Restore a Saarthi Studio JSON backup. Restored conversations are appended; existing chats are not deleted. Importing the same backup more than once creates duplicates in this historical version.
- Backup restore validates the format, limits the selected file to 10 MB, and keeps import in a database transaction to avoid partial imports.

## Historical upgrade instructions

These are retained as repository history. Do **not** run old patch scripts to install v1.11.1; they replace files from earlier releases. For the complete current project, use the current `android` branch or build this folder using `BUILD-APK-UBUNTU.sh`.

The current release should be installed over the existing app only after the build succeeds and only with a compatible signing key. Do not uninstall the app to upgrade, as that can delete local chat history and settings.
