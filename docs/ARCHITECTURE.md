# Architecture (as built, verified against the tree)

This document records what the repository actually contains. It is the map that
`docs/MASTER-ROADMAP.md` builds on; anything not verified here is marked as
planned, not real.

## 1. Runtime shape

One Rust binary (`app/`, crate `free-ai-studio`, binary `free-ai-studio`)
serves everything:

| Listener | Default | Mounted routes | Auth |
|---|---|---|---|
| Loopback UI/API | `127.0.0.1:8765` | `main.rs` router + `workspace`, `skills`, `terminal`, `local`, `media`, `router_console`, `agent` routers | None (loopback + `Host`/`Origin` guards on writes) |
| Host API (remote) | opt-in, separate listener | `host_api.rs` (`/api/host/v1/*`) | Bearer host token (`host_auth.rs`), TLS via `axum-server` |

`gui` feature (default) adds a Tauri v2 window (`gui.rs`, `tauri = "=2.12.5"`).
`--no-default-features` builds the headless server used by the `.deb` job.
The UI is static files from `app/static/` (`index.html`, `app.js`,
`highlight.js` — the editor's pure tokenizer, `styles.css`, `ide.js`,
`ide.css`, `skills/`) with no build step. `ide.js` owns the workbench and the editor's layer: it wraps the textarea in `.code-wrap`, paints `#codeLayer` from `highlight.js`, and keeps that layer's metrics, wrapping, find count and scroll sync.

## 2. Modules and responsibilities

| Module | Lines | Responsibility |
|---|---|---|
| `main.rs` | 3628 | App state, config load/save, SQLite open/migrate, chat + streaming chat, Thinking resolution, conversations, usage, router/omniroute status, static serving, listener setup |
| `agent.rs` | 3591 | Bounded autonomous run loop: one JSON action per model step (`think`/`list`/`read`/`write`/`command`/`done`/`fail`), permissions, approval pauses, checkpoints, diffs, cancellation, `agent_runs`/`agent_steps` persistence |
| `host_api.rs` | 1807 | Authenticated router for the Android companion (health, capabilities, models, chat, conversations, messages) |
| `workspace.rs` | 1699 | Path-confined folder workspace: entries, read/write, create/rename/delete, search, and the read-only `git status` the Source Control pane loads |
| `git.rs` | 1785 | Real Git operations in the open folder: diff, stage, unstage, discard, commit, branches, log, fetch, fast-forward pull, push, per-hunk staging and a commit-detail diff. Argv-only invocations, validated workspace-relative paths and remote names, every mutation behind the `git` permission action. Remote commands have a 90 s deadline, no terminal prompt, and drained, bounded streams; hunk patches are built server-side and sent on stdin |
| `format.rs` | 406 | Format one editor buffer with a fixed formatter chosen by extension (rustfmt/gofmt/Prettier), returning text only — the normal save path writes it; failures name the tool and never replace the buffer |
| `watch.rs` | 416 | Watch mode: one preset reruns when the open folder's fingerprint moves (700 ms, the explorer's hash), behind one `command` approval; holds the runner's only slot, reports the last real run, and Stop kills the command and its shell's children on Windows |
| `runner.rs` | 817 | Detected build/test presets, bounded process capture (separate stdout/stderr threads, 300 s ceiling, 1 MiB per stream) and compiler/test output parsed into clickable `path:line:col` problems |
| `local.rs` | 1422 | Ollama adapter: detect, models, hardware, select, pull (progress over SSE), one model's real capabilities (`/api/show`), 600 s chat timeout, no silent remote fallback |
| `embeddings.rs` | 579 | Semantic search over the code index: symbols are embedded through the router, or by the local runtime when the model carries a `local:` prefix, cached in `embedding_cache` by model + symbol + file digest and ranked by cosine similarity; a provider failure is an error, never a silent word search |
| `index.rs` | 1145 | Code index over the open folder: bounded walk with build/dependency folders and privacy exclusions skipped, per-language symbol extraction, incremental refresh by content digest, ranked word search, the RAG snippets chat turns carry, and the symbol completion ranked behind `GET /api/index/complete` |
| `deepsearch.rs` | 714 | Research answers: provider-native grounding (Gemini, OpenAI `web_search`) or Exa/Tavily loop; bounded queries; SSE steps; citations |
| `host_auth.rs` | 595 | Host token lifecycle (issue/redeem/rotate/revoke), paired-device records |
| `router.rs` | 545 | FreeLLMAPI lifecycle: compose dir/port discovery, ownership file, start/stop/status, stop-only-if-owned on exit |
| `router_console.rs` | 505 | Admin proxy to the router console with session-token handling and path allowlist |
| `media.rs` | 461 | Attachment validation/storage/serving, `/api/embeddings` proxy |
| `skills.rs` | 454 | `SKILL.md` discovery across 5 roots, frontmatter parse, enable/disable, instruction caps |
| `host_security.rs` | 366 | Transport guardrails: origin checks, body caps, timeouts |
| `privacy.rs` | 341 | Outbound credential-shape detection and workspace exclusion rules |
| `terminal.rs` | 318 | Real PTYs (`portable-pty`) — ConPTY on Windows, unix pty elsewhere; SSE streams; resize; close |
| `host_transport.rs` | 245 | Listener binding, TLS material handling |
| `context.rs` | 213 | Token-budgeted request packing (4 chars/token, 6 k default budget, 1 k–32 k clamp) |
| `contract.rs` | 131 | The local capability contract: one server-side table of implemented feature ids, this build's version and platform, and limits read from the constants the routes enforce; served at `GET /api/capabilities` |
| `capabilities.rs` | 280 | Model capabilities from provider-published metadata only (`/models` rows, Ollama `/api/show`); every unknown flag is `null`, never guessed |
| `openai.rs` | 771 | Native OpenAI Platform provider: Responses API request/response/stream translation, `openai:` model ids, model catalog, key test, reasoning-rejection retry and its per-process memory |
| `thinking.rs` | 351 | Thinking levels, fixed precedence (message > agent > project > global > provider default), `@Thinking` directive with a literal escape |
| `permissions.rs` | 761 | Central permission policy: action × target × decision × scope rules, fixed resolution order, `permission_audit` trail, grant/revoke/audit routes |
| `mcp.rs` | 862 | MCP registry (`mcp.json`), stdio JSON-RPC handshake per operation, tool discovery, argument validation, policy-gated tool calls |
| `events.rs` | 77 | Broadcast hub for SSE conversation mirrors |
| `gui.rs`, `browser.rs` | 46, 30 | Tauri window bootstrap, system browser launch |

## 3. HTTP surface (verified route registrations)

Loopback UI/API (`main.rs`):

- `GET /`, `GET /static/*file`
- `GET|POST /api/config`
- `GET /api/conversations/:id/messages`, `POST /api/conversations/:id/delete`
- `GET /api/usage`, `POST /api/open`, `GET /api/health`, `GET /api/capabilities`, `POST /api/test`
- `GET /api/workspace/watch`, `POST /api/workspace/watch/start`, `POST /api/workspace/watch/stop`
- `POST /api/workspace/format` (buffer text in, formatted text out; no write)
- `GET /api/router`, `POST /api/router/start`, `POST /api/router/stop`
- `GET /api/omniroute`, `POST /api/omniroute/start`, `POST /api/omniroute/stop`

Mounted sub-routers:

- Workspace: `/api/workspace`, `/open`, `/pick`, `/close`, `/entries`, `/files`, `/file` (GET+PUT), `/create`, `/rename`, `/delete`, `/search`, `/vcs`
- Git (same router, so the same origin guard and body cap): `GET /api/workspace/git/diff`, `POST /git/stage`, `/git/unstage`, `/git/discard`, `/git/commit`, `GET /git/branches`, `POST /git/branch`, `/git/checkout`, `/git/fetch`, `/git/pull`, `/git/push`, `/git/stage-hunk`, `/git/unstage-hunk`, `GET /git/show`, `GET /git/log`, `GET /git/reflog`, `POST /git/undo`
- Runner (same router): `GET /api/workspace/runner` (detected presets, then commands saved in Settings), `POST /api/workspace/run` (one preset id, never a command line)
- Skills: `GET /api/skills`, `GET /api/skills/:id`, `POST /api/skills/:id/state`
- Terminal: `POST /api/terminal/open`, `/api/terminal/:id/input`, `/resize`, `/close`, `GET /api/terminal/:id/stream` (SSE)
- Local inference: `GET /api/local/status`, `/models`, `/hardware`, `POST /api/local/select`, `/api/local/pull`
- Media: `POST /api/attachments`, `POST /api/embeddings`, `GET /attachments/:file`
- Router console: `GET /api/router/console/status`, `POST /login`, `ANY /api/router/console/*path`
- Agent: run/step routes implemented in `agent.rs` (runs, approve, cancel, steps, diff)

Host API (`host_api.rs`, version `1`): `GET /api/host/v1/health`, `/capabilities`,
`/models`, `POST /api/host/v1/chat`, plus conversation/message reads. Body cap
512 KiB, page caps 100/150.

## 4. Persistence

SQLite at `~/.local/share/free-ai-studio/studio.db` (`rusqlite`, bundled).
Tables created by `main.rs`/`agent.rs`:

- `projects(id, name, folder, created_at, updated_at, archived)`
- `conversations(id, title, created_at, updated_at, project_id)`
- `messages(id, conversation_id, role, content, meta, created_at)`
- `request_log(model, provider, status, status_code, tokens, duration_ms, detail, created_at)`
- `agent_runs(id, conversation_id, status, …)` with statuses
  `running|awaiting_approval|succeeded|failed|cancelled|interrupted`
- `agent_steps(run_id, seq, action, summary, output, at, checkpoint)`

Config lives in `~/.config/free-ai-studio/config.json` (never moved — brand
string is "Sarthi Studio", internal `free-ai-studio` paths stay).

## 5. Platform clients

- **Android** (`android/`, Java + Gradle): chat client over the Host API and a
  direct-router transport. Own SQLite (`ChatDatabase`, `DatabaseMigrations`),
  policies for network/host discovery/host connection/attachments/backup,
  JVM + instrumentation tests, CSS-less native UI. No on-device agent or
  router management.
- **VS Code** (`vscode-extension/`, TypeScript): `extension.ts` (commands/webview),
  `lib.ts` (context packing, conversation store, workspace API, embedded Node
  HTTP server implementing a subset of the desktop routes), `agent.ts` (a port
  of the Rust run loop). Node `--test` suite under `test/`.

## 6. Delivery

`desktop-release.yml` builds on `v*` tags: `.deb` (ubuntu-22.04, headless),
Linux tarball + AppImage + `SHA256SUMS.txt`, Windows zip, macOS universal
tarball (unsigned), Android APK/AAB, VS Code VSIX — each job attaches its own
artifact with `softprops/action-gh-release`. CI gates: `desktop-ci.yml`
(clippy `-D warnings`, tests, ubuntu + macOS), `rust-host-api-ci.yml`
(a whole-tree `cargo fmt --check`, tests, clippy), `android-ci.yml` (unit, lint,
assemble, emulator instrumentation, hard validation gate).

## 7. Architectural boundaries that must not erode

1. The loopback router and the Host API router are separate; never merge them.
2. Workspace paths are canonicalised and confined to the opened folder; every
   new route reuses those helpers instead of joining paths by hand.
3. API keys never reach the browser; the UI only ever holds opaque state.
4. Ownership of an external process (router) is tracked in a state file —
   do not stop something the app did not start.
5. `--no-default-features` must stay buildable and dependency-light (it is the
   `.deb` path and the headless smoke path).
