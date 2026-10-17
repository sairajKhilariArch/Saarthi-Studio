# Changelog

## 1.1.0 — Conversation management

- Added separate conversations instead of one global message list.
- Added Chats screen with conversation previews and open/switch actions.
- Added new chat, rename, and delete controls.
- Automatically titles a new conversation from the first prompt.
- Upgraded SQLite database from version 1 to 2 and migrates existing messages into `Previous conversation`.
- Incremented Android `versionCode` to 2 and `versionName` to 1.1.0.
- Fixed Android SDK setup in GitHub Actions by avoiding the obsolete `tools` SDK package.
- The APK workflow now watches pushes to the `android` branch.

## 1.0.0

- Initial native Android chat UI.
- FreeLLMAPI-compatible chat requests and connection test.
- Locally stored chat history and Android Keystore encrypted settings.
