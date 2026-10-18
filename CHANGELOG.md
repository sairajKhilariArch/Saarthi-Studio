# Changelog

## 1.4.0
- Added Usage & Diagnostics dashboard.
- Records AI request successes, failures, status codes, latency, token counts when provided, and routed provider metadata when returned.
- Adds a one-tap FreeLLMAPI connection test from diagnostics.
- Adds database schema migration with best-effort backfill from existing saved assistant messages.
- Preserves existing conversation, Markdown/code copy, and model-picker features.
