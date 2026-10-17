# Free AI Suite for Ubuntu — Rust edition

A Rust-powered local AI coding/chat workspace that works with FreeLLMAPI. The application backend, local HTTP server, router client, settings persistence, usage tracking, and SQLite history are implemented in Rust (`axum`, `reqwest`, `rusqlite`). The existing browser interface uses HTML, CSS, and JavaScript, served locally by the Rust app.

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
- Code editor and code-review context
- Usage/error log with latency and token counts when the provider reports them
- Router connection test and saved settings
- Token-conscious context: stores full local history but sends only the latest four messages plus a short system prompt with each request
- Binds to `127.0.0.1` only; key is stored on the server side in a `0600` configuration file
- One-click launcher for the Rust app and FreeLLMAPI Docker service

The code editor edits text; it does not execute code. Automatic fallback is controlled by FreeLLMAPI and depends on available provider quota. Free tiers are not unlimited.

## Addresses and local data

- AI Studio: `http://127.0.0.1:8765`
- FreeLLMAPI dashboard: `http://127.0.0.1:3001` by default
- Settings: `~/.config/free-ai-studio/config.json`
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
