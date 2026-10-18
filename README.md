# Saarthi Studio Android — Usage & Diagnostics Update

Version 1.4.0 adds an in-app **Usage & Diagnostics** panel to the existing Saarthi Studio Android project.

## Features

- Total recorded AI requests and successful/failed requests.
- Total tokens only when FreeLLMAPI reports a usage total.
- Average observed response duration.
- Recent request records with model ID, HTTP status (when known), provider header (when supplied), and concise failure details.
- A one-tap connection test against the configured FreeLLMAPI `/v1/models` endpoint.
- SQLite schema migration and backfill from existing saved assistant replies where metadata is available.
- Leaves conversation management, Markdown/code copy, model picker, encrypted settings, and the FreeLLMAPI connection in place.

## Apply update

See [`UPGRADE-INSTRUCTIONS-1.4.0.md`](UPGRADE-INSTRUCTIONS-1.4.0.md). The update script copies two files into your existing repository and preserves timestamped backups. Apply it only to the `android` branch.

## Limitations

This is local usage telemetry, not provider-side quota data. Unreported token totals remain unknown. A successful source edit is not a compiled APK; wait for the GitHub Actions workflow to succeed before installing the new build.
