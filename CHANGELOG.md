# Changelog

## 1.5.0 — Chat Backup & Export

- Export the current conversation to a Markdown file using Android's document picker.
- Export all conversations as a JSON backup that excludes API keys and provider settings.
- Restore a JSON backup by appending imported conversations without deleting existing chats.
- Validate backup format and size before importing; import is transactional.
- Keeps the existing FreeLLMAPI connection, encrypted settings, Markdown rendering, model picker, and diagnostics.
