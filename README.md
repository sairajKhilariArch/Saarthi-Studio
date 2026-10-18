# Saarthi Studio for Android

**Version 1.10.0 (versionCode 12)** — native Android companion for Saarthi Studio.

Saarthi Studio connects to a FreeLLMAPI / OpenAI-compatible router running on your Ubuntu computer or another server. The Android app does not run AI models locally; requests are routed by the configured server.

## Features included in this combined 1.7.0–1.10.0 implementation

- **Streaming answers with Stop**: receive server-sent-event (SSE) chunks as they arrive; cancel an active request; save a clearly marked partial reply when stopped; fall back to non-stream mode only for explicit early stream-capability errors before any text was received.
- **Text-file attachments**: select `.rs`, `.java`, `.py`, `.json`, `.md`, and `.txt` via Android's document picker; max 3 files, 512 KiB each, 1 MiB total; UTF-8 validation; remove files before sending. Content is inlined into the user message, not uploaded as a binary object.
- **Edit and resend**: edit an earlier user message, then atomically remove later turns in that same conversation before resending the edited message.
- **Regenerate response**: regenerate only the latest assistant response without duplicating the preceding user message.
- **Prompt presets**: built-in presets for Rust, Java QA, Python, code review, minimal reproductions, and step-by-step explanations; create, edit, duplicate and delete custom presets; apply as visible editable composer text.
- **Preset-aware backups**: JSON backup schema 2 includes custom presets; restore still accepts legacy schema 1 backups and adds imported chats/presets without overwriting existing records.
- Multiple saved conversations, automatic titles, switch/open, rename and delete.
- Search conversation titles and message contents.
- Persistent local history using SQLite; schema v4 adds presets without deleting existing conversations/messages.
- Markdown-style answer formatting and copy buttons for answers/code blocks.
- Optional code context panel, model picker via `/v1/models`, local usage diagnostics, Markdown export, full chat JSON backup/restore.
- Unified API key encrypted at rest with Android Keystore and AES-GCM.
- No third-party Android runtime libraries; Android platform APIs are used.

## Important attachment/privacy behavior

Attached source files are read on the phone and inserted as text into the next user message. That means the file contents become part of local chat history and are included in JSON chat backups. Review contents before sending. This version intentionally does **not** claim to parse PDFs, DOCX/XLSX documents, images, or arbitrary binary formats.

Use plain HTTP only between your phone and a server on a trusted private LAN/hotspot. Do not expose the FreeLLMAPI port directly to the public internet. Use HTTPS for remote/public connections.

## Project layout

```text
Saarthi-Studio-Android-1.10.0/
├── .github/workflows/build-android.yml
├── app/
│   ├── build.gradle.kts  # versionName 1.10.0, versionCode 12
│   └── src/main/
│       ├── AndroidManifest.xml
│       ├── java/com/saarthi/studio/MainActivity.java  # all feature code in this one package/file
│       └── res/values/styles.xml
├── build.gradle.kts
├── gradle.properties
├── settings.gradle.kts
├── BUILD-APK-UBUNTU.sh
├── BUILDING.md
├── CHANGELOG.md
└── INSTALL-ON-ANDROID-BRANCH.md
```

## Build

Requirements: JDK 17, Gradle 8.9, Android SDK Platform 35, Build Tools 35.0.0.

```bash
export ANDROID_HOME="$HOME/Android/Sdk"  # adjust to your SDK location
export ANDROID_SDK_ROOT="$ANDROID_HOME"
./BUILD-APK-UBUNTU.sh
```

The APK output path is `app/build/outputs/apk/debug/app-debug.apk`. Open the project in Android Studio and use **Build APK(s)** as an alternative.

## Versioning

The combined feature package is version `1.10.0`, code `12`. Future Android releases must use a `versionCode` greater than 12. Android update installation also requires a compatible signing certificate; always export an important chat backup before updating.
