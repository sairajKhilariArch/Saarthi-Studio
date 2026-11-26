# Sarthi Studio Android companion

Sarthi Studio for Android is the companion client for the Sarthi Studio desktop host. It supports Direct Router mode for an OpenAI-compatible endpoint and Desktop Host mode through the desktop Host API. Conversations and Android preferences are stored locally.

## Project status

> **Architecture:** the Rust desktop app is the primary host. Android supports both Direct Router and Desktop Host modes. Host mode uses the desktop's separate authenticated Host API over TLS; host-mode chat currently advertises non-streaming text and no attachment support. See [`PLATFORM-ARCHITECTURE.md`](../docs/PLATFORM-ARCHITECTURE.md).

**v1.52.0 (version code 62) — current Android build.** The `release` build type enables R8 minification and resource shrinking, `targetSdk` is 35, and plaintext router endpoints are restricted to loopback/private LAN ranges (public hosts must use HTTPS). Release APK/AAB artifacts are assembled unsigned and signed at distribution time. CI must pass and the physical-device checklist in `UPGRADE-GUIDE-1.11.1.md` must be completed before distribution.

## Included capabilities
- OpenAI-compatible FreeLLMAPI chat, model listing, streaming and Stop.
- Local SQLite chat history; rename/delete/search; edit/regenerate; prompt presets.
- Request diagnostics and reported token counts.
- JSON backup/restore and Markdown export.
- Retry of failed requests, bounded transient retry, and ordered model fallback.
- Favorite/default/fallback model preferences and FreeLLMAPI free-tier budget viewer.
- Folders, tags, pinned/favorite chats, and message-content search.
- Syntax highlighting for a basic common-token subset; code/text attachments.
- Android speech-recognition input and text-to-speech output.
- Limited image input using OpenAI-compatible multimodal message content when the model/router supports vision.

## Setup
1. Start FreeLLMAPI on the PC/server. For use over a trusted local hotspot, bind the service to the LAN interface and use the PC's LAN address from the phone.
2. Open Sarthi Studio Settings and enter the base URL ending in `/v1`, for example `http://192.168.1.10:3001/v1`, and the unified API key.
3. Load models if available, pick the default model, optionally configure favorite/fallback model IDs, and test the connection.

## Important limits
- Text attachments: 3 files maximum, 512 KiB each, 1 MiB combined.
- Images: 2 maximum, 900 KiB combined; model/router must support vision. Image bytes are held in the local message content and included in JSON backups.
- Backup files are limited to 10 MiB.
- The quota screen reports what `/api/free-tier` returns; it does not assert an exact remaining daily quota when the server does not provide one. PDF extraction is not included in this version.

## Build
See `BUILDING.md` for Gradle/Android SDK requirements and CI workflow. See `CHANGELOG.md` and `UPGRADE-GUIDE-1.11.1.md` for details.
