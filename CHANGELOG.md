# Changelog

## 1.10.0 — combined feature release (versionCode 12)

This project bundles the planned 1.7.0–1.10.0 feature set into the single `com.saarthi.studio.MainActivity.java` source file.

### Streaming responses (1.7.0)
- Parse OpenAI-compatible SSE `data:` frames and append answer deltas live.
- Add a Stop button that disconnects the active HTTP connection.
- Keep partial text with a “Stopped by user” marker when generation is cancelled after text arrives.
- Retry non-streaming only for selected explicit early stream-capability rejections (400/404/405/422), before any content was displayed.
- Store token usage only if it was reported by the router/provider.

### Text-file attachments (1.8.0)
- Add Android Storage Access Framework document selection for `.rs`, `.java`, `.py`, `.json`, `.md`, and `.txt`.
- Enforce max 3 files, 512 KiB per file, and 1 MiB total; reject invalid UTF-8 and unsupported extensions.
- Inline selected contents into the outgoing user message as text; no separate binary upload or broad storage permission.
- Indicate that contents become chat history and backup data; clear unsent attachments on conversation switch.

### Edit/resend/regenerate (1.9.0)
- Give persisted messages stable database IDs in the UI model.
- Edit a user message and atomically delete subsequent turns in the same conversation before resending.
- Regenerate only the latest assistant reply using its preceding user message without inserting a duplicate user turn.

### Prompt presets (1.10.0)
- Upgrade SQLite schema from v3 to v4 by adding `prompt_presets`; retain existing chat and request log rows.
- Seed six built-in programming/debugging presets.
- Add create, edit, duplicate, delete and composer insertion flows; preset text remains visible/editable before sending.
- Bump JSON backup schema to v2 to include custom prompts; continue accepting v1 backups.

## 1.6.0
- Added live search for saved conversation titles and message content.
- Search result cards show the matching message excerpt where available.
- Kept search on the existing SQLite schema.
- Bundled the complete standalone Android Studio/Gradle project source.

## 1.5.0
- Added chat export to Markdown, full JSON backup, and restore.

## 1.4.0
- Added Usage & Diagnostics: local request outcomes, reported token totals, latency, recent failures, and connection testing.

## 1.3.0
- Added a model picker that queries FreeLLMAPI's `/v1/models` endpoint while retaining `auto` and manual model IDs.

## 1.2.0
- Added Markdown-style formatting and copy controls for responses/code blocks.

## 1.1.0
- Added separate conversations, conversation history, rename/delete, and automatic titles.
