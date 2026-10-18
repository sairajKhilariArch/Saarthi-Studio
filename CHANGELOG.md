# Changelog

## 1.11.0 — Power User Update (release candidate)

This release bundles reliability, model preferences/fallback, quota inspection, conversation organization, richer developer tools, voice input/output, and opt-in image input. This source package is a release candidate until CI and physical-device checks pass.

### Added
- Bounded same-model retry for selected transient transport/server failures, ordered fallback-model attempts before any text is returned, and a manual Retry action for failed latest responses.
- Favorite/default/fallback model preferences; favorite models are ordered first in the model picker.
- FreeLLMAPI `/api/free-tier` budget viewer. It displays the server response as a monthly budget overview when supported; it does not claim exact live daily quota or remaining messages.
- Conversation folders, tags, pinning, favorites, combined filters, and full-message/folder/tag search.
- SQLite schema v5 migration adds organization metadata without dropping conversations, messages, diagnostics, or prompt presets.
- Syntax coloring for common code tokens in code blocks and expanded text/code attachment extensions.
- Voice dictation through Android's speech-recognition intent and text-to-speech with a stop control.
- JPEG/PNG/WebP/GIF image attachment support using the OpenAI-compatible `image_url` content format, after a confirmation when vision support cannot be recognized from the model name.
- Backup schema v3 stores conversation organization and image-bearing message content while preserving import support for schema v1 and v2 backups.
- GitHub Actions runs `testDebugUnitTest`, `lintDebug`, and `assembleDebug` before uploading the APK artifact.

### Preserved
- FreeLLMAPI base URL and encrypted API-key storage, streamed responses, Stop, saved conversation history, rename/delete/search, edit/regenerate, prompt presets, diagnostics, Markdown export, and JSON backup/restore.

### Limits / caveats
- Text attachments: maximum 3 files, 512 KiB each and 1 MiB total. Supported file extensions are listed in the in-app guidance/source.
- Images: maximum 2 images per message and 900 KiB combined; the selected model and router must support vision. Image bytes are encoded inside the stored message for reliable local history and backup, so large backups can hit the 10 MiB backup cap.
- Quota display depends on FreeLLMAPI exposing `/api/free-tier` to the configured key. The returned overview is not guaranteed to be an exact live balance for that key.
- PDF text extraction is deferred because it requires a PDF parser dependency and its own file-size/security tests; PDFs are not silently treated as text.
- A successful source change or push is not the same as a successful build/device test.
