# Sarthi Studio — Phase 8 Web Validation

**Date:** 2026-11-25
**Build:** 3.88.12
**Surface:** the application's own browser UI served from the local Rust server (not the Astro marketing website).

## Status summary

| State | Result |
|---|---|
| Implemented | Yes — same static UI as the desktop shell |
| Built | Served directly; no frontend build step |
| Installed | N/A — no install |
| Executed | Yes, loopback server and Chromium |
| Tested | Load, assets, headers, origin/host rejection, persistence, restart, responsive layout (Phase 7 evidence), 125 frontend tests |
| Blocked / N/A | Accounts and sessions (none exist); Safari/Firefox; real network-loss testing; multi-user hosting |

## What was tested

- Server serves `index.html` and every runtime asset with HTTP 200 from the packaged tree: `app.js`, `ide.js`, `ide.css`, `highlight.js`, `replace.js`, `styles.css`, brand SVGs, skills.
- Browser reload returns the same workbench; the Phase 7 UI is hydrated from the local API, not a server-side template.
- Restart with the same `FREE_AI_STUDIO_HOME` reuses the SQLite database; E2E proves conversations, settings, runs and steps survive a restart, and the database is at the documented path.
- The browser UI reports request failures in-place and supports stopping a stream; the real-binary E2E suite exercises streaming, retries and error paths (76 passing).
- Responsive widths 1920/1600/1366/1024/768/390 were validated in Phase 7 with no page-level horizontal overflow; Phase 8 re-used those layouts and did not change them.

## Security behaviour

| Control | Evidence |
|---|---|
| Content-Security-Policy | `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'` |
| Framing | `X-Frame-Options: DENY` for normal launches; `frame-ancestors` allow-list only for the embedded VS Code webview mode |
| MIME sniffing | `X-Content-Type-Options: nosniff` |
| Referrer leakage | `Referrer-Policy: no-referrer` |
| Foreign origin | POST with `Origin: https://evil.example` → **403** |
| DNS rebinding | Request with `Host: evil.example` → **403** |
| Provider secrets | Keys live in the OS keychain / server config, never in the served bundle; the browser calls the local backend |
| Remote access | Optional Desktop Host API is HTTPS-only with a bearer token; it is not the local UI |
| Authentication | **None for the local UI by design** — it binds loopback only. There are no user accounts, cookies or sessions to expire. Exposing it on a network is not a supported authentication boundary. |

The frontend has no inline `<script>` and no inline event handlers; `script-src 'self'` is therefore compatible with the current UI.

## Functionality that requires the desktop/local server

The web UI is served *by* the desktop backend. Without that process it has no API. Features that inherently require the host machine and are unavailable in a pure remote/browser deployment:

- Opening folders and reading/writing project files.
- PTY terminals.
- Git operations, LSP/DAP, code indexing and local model/router management.
- Agent runs and workflows that execute tools on the machine.

The Android client is separate and intentionally does not expose these.

## Not tested / limitations

- No Safari, Firefox or mobile-browser matrix; Chromium (packaged) only.
- No automated session-expiry test because there is no session system.
- No throttled/offline browser run; streaming failure handling is covered by unit/E2E paths, not by a real radio/network cut.
- No public deployment, reverse proxy or multi-user load test. The app is a loopback-first local tool; remote Desktop Host access is a separate TLS/token API.
- The Astro marketing website under `website/` builds (37 pages) but is not this application and was not part of runtime validation.
