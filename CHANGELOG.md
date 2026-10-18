# Changelog

## 1.3.0 — FreeLLMAPI model picker

- Adds a **Load available models** control in Router settings.
- Fetches model IDs from the router's OpenAI-compatible `GET /v1/models` endpoint.
- Lets users select `auto` or any returned model ID, while keeping manual model-ID entry available.
- Gives clearer feedback when the model endpoint is unavailable, returns no usable model IDs, or rejects the request.
- Leaves local conversation storage, database schema, encrypted settings format, and chat request format unchanged.
- Increments Android `versionCode` to 4 and `versionName` to 1.3.0.

## 1.2.0 — Markdown and code-copy actions

- Renders Markdown headings, bullets, inline code, bold and italic spans using native Android widgets.
- Shows fenced code blocks in monospaced, horizontally scrollable panels with language labels.
- Adds one-tap **Copy code** and **Copy response** actions.
- Keeps original Markdown response text in SQLite; formatting is applied when displaying it.
- Does not change the SQLite schema, server API, router settings, or conversation history.
- Increments Android `versionCode` to 3 and `versionName` to 1.2.0.


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
