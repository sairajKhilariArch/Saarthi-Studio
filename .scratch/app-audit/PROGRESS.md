# Progress

## Done
- M1 audit + persistent state (this directory).
- M2 privacy pass shared core (Rust `privacy.rs` + VS Code ported scanner), context packing integration.
- M3 VS Code agent parity: `agent.ts` runtime, `/api/runs` server, Runs webview panel; cancel flag now actually read mid-step (desktop + extension); all tests green (94/94 desktop, 22/22 extension).
- M4a Android streaming host chat: `DesktopHostTransport` SSE bridge, dual-format stream parsing in MainActivity, host_api `chat_stream` route + strict-shape tests + mock-router happy-path (delta/done frames, persisted messages).
- M4b Android agent runs: agent routes now also mounted at `/api/host/v1/runs*` on the host listener behind the host bearer wall; `agent_runs` advertised in `/api/host/v1/capabilities`; MainActivity Tools menu → "Agent runs" with capability-gated Runs screen (list, detail, pending diff monospace preview, Approve/Reject, Cancel-run, per-step Restore), New-run composer with permission checkboxes, graceful degradation for older hosts. Host boundary tests: unauth 401, list shape, masked 400s, shared 404s.

- M5 macOS packaging: desktop-release.yml macos job now builds BOTH arm64 and x86_64, merges a universal binary with `lipo`, verifies both slices, runs a headless HTTP smoke (`FREE_AI_STUDIO_PORT`), and packages one `macos-universal` unsigned tarball (no dmg: unsigned dmg is punished harder by Gatekeeper than a tarball — deviation from the roadmap wording, same goal). desktop-ci.yml gained a macOS build+test job so macOS-only breaks surface in CI. Signing/notarization stays a documented blocker (no credentials).

## Current
- M6 done: adaptive routing (choose_tier/adaptive_tier, step-level override + failover-to-base step, recorded per run) + reproducible run records (model/router_config columns + create-time snapshot + per-step record; run card shows model). Settings: Adaptive routing checkbox + cheap/strong fields in Chat context section.
- M7 done: Local AI. Backend: `local.rs` — Ollama-only adapter (127.0.0.1:11434 default, configurable, remote URLs refused), real runtime detection/API-version probe, `/api/tags` model discovery, select/pull endpoints (pull progress via the event bus, one pull at a time), best-effort hardware probe (CPU/RAM/GPU/disk, Unknown when undetectable), `chat_through_local` gating. Routes: `/api/local/{status,models,hardware,select,pull}`; config gains local_base_url/local_model/local_chat. Frontend: Local AI view wired to `localBtn` (handler was missing — added, plus `runsBtn` which had the same defect), status panel + setup onboarding when the runtime is absent, endpoint/selection form, explicit pull form, model cards, hardware panel with exact-machine recommendation. Chat/runs stay on remote unless the local toggle is on; no silent fallback either way.

## Next
- Release-pipeline repair (v1.25.0 re-run) and Step 2 of the master plan (see trace below).

## Step 1 close-out (release pipeline repair)
- Release failure diagnosis from the v1.25.0 workflow run 41007439650: deb job failed in the headless build (`build.rs` ran `tauri_build::build()` unconditionally — panic "missing cargo:dev" without the `gui` feature); Linux `release` job failed at `cargo deb`; vscode job failed at `npx tsc --noEmit` (2 real type errors). Also: every version in tree still said 1.22.0 while the tag said v1.25.0 (Windows zip asset was literally `free-ai-suite_1.22.0_windows-x86_64.zip`).
- Fixes: `build.rs` gates tauri-build on `CARGO_FEATURE_GUI`; `agent.ts`/`lib.ts` type errors fixed (tsc clean, 22/22 tests, vsix packages); versions bumped to 1.25.0 in app Cargo.toml+lock, extension package.json, android versionName 1.21.1→1.25.0 (versionCode 18).

## Step 2 (master plan) — message-flow trace (B1)
- Chat: `POST /api/chat` → `deepsearch_phase` (optional native-provider pre-phase) → `prepare_chat` (saves user message, enforces privacy on code attachments, resolves media) → thread history via `history_limit` recent rows → workspace outline (optional) → `context::pack` (token-budgeted: budget 6k default, 2–24 history messages, old code attachments replaced by markers, head/tail cropping) → local mode routes to Ollama (`local::chat_once`) else `call_router` → POST `{router_url}/chat/completions` (bearer, `stream:false`) → assistant message persisted, `request_log` row written (model, tokens, latency, fallback attempts), event bus notified.
- Streaming: same prep, `POST /api/chat/stream` → `chat_stream` folds provider SSE into UI frames.
- Agent runs: `POST /api/runs` → persisted RunRow + steps → `agent::run_driver` loop → `step_once`: territory/gate check → adaptive tier pick → `build_messages` (goal + capped step history) → `call_model` (tier override or base) → reply parsed as one JSON action: think/done/fail → terminal; else `execute_action_step` (permission gate → approval gate → tool with checkpointer → log) — checkpoint/diff/restore per edit.
- Simple-message token path already capped by `context::pack` (B2: verify live request sizes; B3: multi-turn window behavior).

## Blockers
- No Ollama runtime on this VM: runtime detection, model listing, pull and local chat verified as honest failure paths + unit tests; a live local inference round-trip needs a machine with Ollama installed.
- Live Android device/emulator: none provisioned (gradle build + unit tests + lint only).
- Later-turn environment: `gradle` not on PATH — binary found at `~/.local/opt/gradle/gradle-8.9/bin/gradle`; JDK used: `/usr/lib/jvm/java-17-openjdk-amd64`.
- No macOS runner locally (M5 CI jobs execute only on GitHub's `macos-latest`); signing credentials absent.
- No provider credentials here: adaptive tiering verified by unit tests, not a live routed run.
