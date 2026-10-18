# Saarthi Studio Android 1.11.1

Saarthi Studio is an Android client for a self-hosted FreeLLMAPI endpoint. This project preserves locally stored chats and encrypted endpoint settings while adding conversation organization, model preferences/fallback, budget inspection, developer attachments, voice, and optional vision input.

## Project status

**v1.11.1 release candidate — build/device validation required.** The source package should not be treated as a verified APK until CI succeeds and the physical-device checklist in `UPGRADE-GUIDE-1.11.1.md` is completed.

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
2. Open Saarthi Studio Settings and enter the base URL ending in `/v1`, for example `http://192.168.1.10:3001/v1`, and the unified API key.
3. Load models if available, pick the default model, optionally configure favorite/fallback model IDs, and test the connection.

## Important limits
- Text attachments: 3 files maximum, 512 KiB each, 1 MiB combined.
- Images: 2 maximum, 900 KiB combined; model/router must support vision. Image bytes are held in the local message content and included in JSON backups.
- Backup files are limited to 10 MiB.
- The quota screen reports what `/api/free-tier` returns; it does not assert an exact remaining daily quota when the server does not provide one. PDF extraction is not included in this version.

## Build
See `BUILDING.md` for Gradle/Android SDK requirements and CI workflow. See `CHANGELOG.md` and `UPGRADE-GUIDE-1.11.1.md` for details.
