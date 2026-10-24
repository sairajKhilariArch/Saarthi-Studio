# Free AI Studio — desktop, Windows, Android and VS Code

A Rust-powered local AI coding/chat workspace that works with FreeLLMAPI. The backend, local HTTP server, router client, settings persistence, usage tracking, folder workspace API, and SQLite history are implemented in Rust (`axum`, `reqwest`, `rusqlite`). The browser interface uses HTML, CSS, and JavaScript, served locally by the Rust app: a compact agent-style shell with threads, a file explorer, tabbed editing, and a router-backed chat.

## One-command install

1. Extract `free-ai-suite-rust-ubuntu.zip`.
2. Open a terminal inside the extracted `free-ai-suite-rust-ubuntu` folder.
3. Run:

   ```bash
   ./install-and-run.sh
   ```

The installer installs missing Ubuntu prerequisites, installs the stable Rust toolchain if needed, builds the Rust app in release mode, starts FreeLLMAPI via Docker Compose, and creates an Applications-menu shortcut named **Free AI Suite (Rust)**. The first Rust build downloads crates from the Cargo registry, so internet is required during installation.

The installer reuses `~/freellmapi` if it exists. It preserves FreeLLMAPI's existing `.env`, database, provider configuration, and the AI Studio's existing local settings/history outside the application folder. It does not remove or reset provider settings.

After that, launch **Free AI Suite (Rust)** from the Ubuntu Applications menu, or run `~/.local/bin/free-ai-suite`.

## First-time setup

When the app opens, choose **Provider setup** to open FreeLLMAPI. Configure the upstream provider credentials and any fallback chain there. Then create/copy the FreeLLMAPI unified API key and save it in **Router settings** in the app. You only need to do this once unless you change keys.

Never share the unified API key, provider keys, or `.env` file in chat or public logs.

## Features

- Rust HTTP backend and OpenAI-compatible router client
- Persistent conversations and message history (SQLite)
- Workspace explorer: open any local folder, browse the tree, and edit files in tabs
  - Atomic saves (temp file + rename), `Ctrl+S`, `Tab` indentation, line numbers
  - Create, rename, and delete files/folders, with unsaved-change protection
  - Scratch pad for pasting one-off code, and “Attach to chat” for file context
- Markdown replies with code blocks and copy buttons; threads with automatic titles
- Usage/error log with latency and token counts when the provider reports them
- Router connection test and saved settings
- **Token-budgeted context** sized like a coding agent: a hard per-request ceiling (6k tokens by default), a names-only project outline, old attachments replaced by one-line markers, and head/tail cropping so a long thread never resends itself. The composer shows a live `~used / budget` meter
- **Router console**: the FreeLLMAPI dashboard's providers, model catalog, routing strategy, analytics, logs and free-tier budget, served from inside the app over loopback
- **Docker lifecycle**: Studio starts the router only when it is stopped, and stops it on exit only if Studio started it
- **Native window**: the UI opens in an application window (Tauri 2 + the system webview), not a browser tab; `--browser` keeps the browser flow if you prefer it
- Binds to `127.0.0.1` only; key is stored on the server side in a `0600` configuration file
- One-click launcher for the Rust app and FreeLLMAPI Docker service

The editor edits text; it does not execute code or open a terminal. Workspace access is confined to the folder you explicitly open: paths are canonicalised, `..`/absolute escapes and symlinks leaving that folder are rejected, binary and files over 1 MB are refused, and workspace routes reject non-loopback hosts/foreign origins. Automatic fallback is controlled by FreeLLMAPI and depends on available provider quota. Free tiers are not unlimited.

## Runs as an application window, not a browser tab

The desktop binary links Tauri 2 and loads the same local UI in the system webview (WebKitGTK on Linux, WebView2 on Windows). That keeps the download small — no bundled Chromium — and closing the window shuts down the local server, then stops the router if this run started it.

```bash
free-ai-studio              # application window
free-ai-studio --browser    # open in your default browser instead
free-ai-studio --no-gui     # headless server (used by tests and the extension)
```

Windows users run `free-ai-studio.exe` from the release zip; WebView2 ships with Windows 10/11.

## Router console and Docker lifecycle

Studio reads where FreeLLMAPI is installed from `~/.config/free-ai-suite/freellmapi-dir`, starts it with `docker compose up -d` when it is not answering, and records ownership in `~/.config/free-ai-studio/router-owned.json`. On exit it runs `docker compose down` **only** when it started the router itself, so a router you launched by hand keeps running.

```bash
free-ai-studio router status|start|stop
```

The console signs in with FreeLLMAPI's loopback desktop session (`POST /api/auth/desktop-session`); a server install can sign in once with its dashboard email/password and the session token is cached in a `0600` file. Everything the console does goes through Studio's local bridge — provider keys are typed into the router, never stored by Studio.

## VS Code extension

`vscode-extension/` packages the same interface as a `.vsix`. It probes `http://127.0.0.1:8765/api/health`; if the desktop app answers it embeds it, and if it does not the extension starts its **own** built-in server (same context budget, same conversation store, same router console bridge) inside VS Code. Editor commands attach the active file or the current selection, and the built-in server's folder API is limited to the workspace VS Code has open.

## Addresses and local data

- AI Studio: `http://127.0.0.1:8765`
- FreeLLMAPI dashboard: `http://127.0.0.1:3001` by default
- Settings: `~/.config/free-ai-studio/config.json`
- Last opened folder: `~/.config/free-ai-studio/workspace.json`
- Chat history: `~/.local/share/free-ai-studio/history.sqlite3`
- App log: `~/.local/state/free-ai-suite/studio.log`

## Troubleshooting

```bash
cd ~/freellmapi && sudo docker compose ps
cd ~/freellmapi && sudo docker compose logs --tail=100 freellmapi
cat ~/.local/state/free-ai-suite/studio.log
```

If the Rust build fails, run `cargo build --release` inside `~/.local/share/free-ai-suite/app` and review the first compiler error. Rust dependencies are version-pinned in `Cargo.toml`; Cargo may create `Cargo.lock` during the first build.

FreeLLMAPI is a separate project: https://github.com/tashfeenahmed/freellmapi

## Monorepo layout and application roles

The Rust desktop app remains the **primary product and intended host**. Its existing Rust service and web UI remain at `app/`; the desktop README, build scripts, and data locations remain the root-level source of truth.

- **Desktop + Windows host:** `app/` — Rust service, application window and web UI. Do not move or replace these files as part of Android work.
- **VS Code extension:** `vscode-extension/` — TypeScript extension, built-in server, and tests.
- **Android companion:** `android/` — independent Gradle project, Android source/tests, Android-specific changelog, and Android CI. It shares the desktop UI's dark, accent-led visual language.
- **Build toolchain:** `.cargo/config.toml` + `rust-toolchain.toml` pin nightly with the cranelift backend and mold linking for fast development builds; both are required (see the file comments). Android requires JDK 17 — newer JDKs can break Gradle 8.9's embedded Kotlin.
- **Platform architecture:** see [`PLATFORM-ARCHITECTURE.md`](PLATFORM-ARCHITECTURE.md) for the desktop-host/Android-client plan and the security requirements for remote access.
- **Android build:** `cd android && gradle testDebugUnitTest lintDebug assembleDebug assembleDebugAndroidTest assembleRelease bundleRelease` (requires JDK 17, Gradle 8.9, and Android SDK Platform 35).

**Important:** this directory restructure keeps desktop and Android source side by side; it does not itself implement desktop-host connectivity. The current Android app still configures a router endpoint directly. Do not advertise desktop-host pairing as available until the authenticated host API and Android client transport are implemented and tested.

## Production Ready - Branch: main

### Android (companion client)
- `release` build type uses R8 minification and resource shrinking; `targetSdk` 35. Version `1.19.0` (version code `16`).
- The "Android CI" workflow gates every `android/**` change on unit tests, lint, debug + release assembly, and emulator instrumentation, and uploads the **unsigned** release APK/AAB (`saarthi-studio-android-release-unsigned`). Sign artifacts with your own keystore at distribution time.
- Plaintext router endpoints are restricted to loopback/private LAN ranges via `network_security_config.xml`; public hosts must use HTTPS.
- Local helper: `android/BUILD-APK-UBUNTU.sh` runs unit tests, lint, debug and release assembly.

### Desktop (Rust host)
- The "Desktop CI" workflow builds, tests, and clippy-gates every `app/**` change against the committed `Cargo.lock`.
- Pushing a `v*` tag runs "Desktop Release", which builds and attaches every platform: Linux x86_64 tarball, `.deb` (cargo-deb) and AppImage with SHA-256 checksums; a Windows x86_64 portable zip built on `windows-latest`; the unsigned Android release APK/AAB; and the VS Code `.vsix`.
- Release profile: LTO, single codegen unit, symbol stripping. The binary resolves its static UI directory at runtime (`FREE_AI_STUDIO_STATIC_DIR` override → installed `../share/free-ai-suite/static` → adjacent `static/` → source tree for `cargo run`), so one binary works from every packaged layout.

