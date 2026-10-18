# Changelog

## 1.6.0
- Added live search for saved conversation titles and message content.
- Search result cards show the matching message excerpt where available.
- Kept the search implementation on the existing SQLite schema; no database migration is needed.
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
