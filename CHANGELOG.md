# Changelog

Release **1.19.0** is built by five local commits. Each commit message starts
with its tag (`v1.19.0-1` … `v1.19.0-5`) and lists the functions it delivers.
Nothing here has been pushed or published.

## 1.19.0

### `v1.19.0-1` — window runtime alignment and platform folder pickers

- `tauri` pinned to `2.12.5` with `serde` `1.0.229`. `tauri` 2.11 compiled
  against the newer `tauri-runtime` monitor API and failed the build; the whole
  2.12 line now moves together.
- The cranelift dev backend is disabled. It aborts with “Rust cannot catch
  foreign exceptions” while compiling the Tauri/GTK window code, so debug and
  release builds both use LLVM; mold and nightly's parallel frontend stay on.
- Windows folder picker (PowerShell `FolderBrowserDialog`) and macOS picker
  (`osascript`), alongside the existing Linux Zenity/KDialog path. Linux-only
  linker flags remain scoped so the same `.cargo/config.toml` builds on Windows.

### `v1.19.0-2` — router console bridge, guarded embedding, test router

- `app/src/router_console.rs` bridges FreeLLMAPI's dashboard API over loopback:
  it requests a desktop session, falls back to a cached session or a one-time
  email/password sign-in, and forwards only an allow-listed set of admin
  families (`keys`, `models`, `fallback`, `analytics`, `logs`, `free-tier`, …).
  A stale session is dropped and re-minted exactly once.
- `/api/router/console/status`, `/api/router/console/login` and an
  `ANY /api/router/console/*path` passthrough, mounted behind the same loopback
  origin guard as the rest of the local UI. Paths outside the admin API,
  traversal, and encoded query strings are refused.
- `/api/health` reports `embeddable`; framing stays `DENY` unless the app was
  launched by the VS Code extension (`FREE_AI_STUDIO_EMBED=vscode`), in which
  case only a `vscode-webview:` frame may load the UI.
- `scripts/mock-router.py`: dependency-free FreeLLMAPI stand-in (proxy +
  dashboard halves) so the console can be exercised without Docker.

### `v1.19.0-3` — console UI, horizontal message actions, context meter

- New Router view: router status with start/stop, providers (add, enable,
  delete), model catalog with “use this model”, routing strategy with the raw
  settings, analytics summary and by-model table, live logs with clear, and the
  free-tier budget table.
- Every message now shows its actions as one horizontal row underneath it
  (user: Copy, Resend; assistant: Copy reply) instead of a stacked side rail.
- Composer token meter (`~used / budget`) driven by the new budget settings,
  and the real `context_tokens` reported by the router after each reply.
- Settings gained context budget, message window and workspace-outline controls.

### `v1.19.0-4` — VS Code extension with desktop detection and built-in server

- `vscode-extension/`: probes the desktop app's `/api/health`; when it answers
  and is embeddable the panel embeds it, otherwise the extension starts its own
  HTTP server on a random loopback port.
- The built-in server serves the same UI and API — chat with the same token
  budget rules, conversations, usage, the router console bridge, and a folder
  API confined to the workspace VS Code has open.
- Commands: open chat, ask about selection, attach active file, set router API
  key (secret storage), show status, restart server, open desktop app.
- The desktop UI accepts editor selections through a guarded `postMessage`
  prefill and shows them as a context chip.
- 13 unit/integration tests cover the packer, store, workspace rules, console
  path guard, desktop probe and both server surfaces; the packaged `.vsix` is
  about 57 KB.

### `v1.19.0-5` — release pipeline for Linux, Windows, Android and VS Code

- `Desktop Release` builds and attaches all four platforms to one GitHub
  release: Linux x86_64 (tarball, `.deb`, AppImage + SHA-256 sums), a Windows
  x86_64 portable zip from `windows-latest`, the unsigned Android release
  APK/AAB, and the VS Code `.vsix`.
- The extension job type-checks and tests before packaging, so a broken
  extension cannot reach a release.
- Android version `1.19.0` (code `16`); README documents the window shell,
  router console, Docker lifecycle, Windows zip and the extension.
- Version bumped to `1.19.0` in `Cargo.toml` and `tauri.conf.json`.
