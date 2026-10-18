# Saarthi Studio Android 1.10.0 — Upgrade Notes

**Final combined feature release:** 1.10.0 (`versionCode = 12`)
**Package:** `com.saarthi.studio`
**All feature source:** `app/src/main/java/com/saarthi/studio/MainActivity.java`

## Included features

| Release plan | Implemented in 1.10.0 |
|---|---|
| 1.7.0 | Streaming via SSE, Stop/cancel, partial answer preservation, early capability fallback |
| 1.8.0 | Text files selected through Android document picker and inlined in messages |
| 1.9.0 | Edit/resend user messages and regenerate the latest assistant answer |
| 1.10.0 | Built-in/custom prompt presets, SQLite v4 migration, preset-aware JSON backups |

The releases are combined rather than separate APKs. Install only the final 1.10.0 package; do not try to install all four version numbers one after another.

## Data migration and backups

- The SQLite helper version changes from 3 to 4. The migration adds the `prompt_presets` table and inserts built-in prompts. It does not recreate the existing conversation/message tables.
- Backup writer emits schema version 2 with custom prompt presets. Import accepts both schema 1 and schema 2 and appends imported chats/presets without overwriting existing records.
- Text attachment contents are inserted into the user message; therefore they are retained in chat history and JSON backups. They are not stored as separate binary files.

## Limits and behavior

- Streaming expects OpenAI-compatible SSE frames with `data:` JSON payloads and `[DONE]` termination. Usage counts remain “not reported” unless present in the server response.
- Non-stream retry is limited to HTTP 400, 404, 405, or 422 received before any content is shown. Other failures are reported rather than blindly retried.
- Text attachment extensions: `.rs`, `.java`, `.py`, `.json`, `.md`, `.txt`; max 3 files, 512 KiB each, 1 MiB total; UTF-8 only. PDF, DOCX/XLSX, images, and binary parsing are intentionally unsupported.
- Editing a user message deletes later turns in that same conversation in one SQLite transaction, then resends the edited text. Regeneration deletes/replaces only the latest assistant response and does not duplicate the user turn.
- Presets insert visible text into the composer; no preset silently changes future requests.

## Validation status

Source-level syntax checks found no Java parser/structural diagnostics. This working environment does not have the Android SDK Platform 35 or Gradle installed, so a complete Android APK build/device regression test has **not** been run here. Build with Android Studio or the included Gradle script before installing, then verify streaming and cancellation against the exact FreeLLMAPI version you run.
