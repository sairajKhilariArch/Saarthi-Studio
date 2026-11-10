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

## Step 3 (master plan) — hybrid cloud/local architecture (done)
- New per-run mode `hybrid` (`agent_runs.mode`, additive migration; `normalize_mode` rejects anything but ""/"hybrid"; `POST /api/runs` refuses a hybrid run without a selected local model).
- Phase 1 in `run_loop`: `plan_with_cloud` asks the cloud router (never the local runtime) for a compact numbered plan from the goal + capped workspace outline; the plan is logged as a `plan` step and reuses any existing plan on resume.
- Phase 2 in `step_once`: hybrid runs skip the adaptive tiers and go to `hybrid_execute` → `local::chat_once` against the configured Ollama endpoint. On a local failure, `ask_cloud_for_guidance` makes one bounded cloud call (goal + capped step log + error), logs it as an `escalate` step and appends the guidance to the retry prompt; a second failure fails the run.
- Context transfer is the existing bounded step log (`build_messages`/`build_history`, 300-char outputs, sanitized) — the cloud only ever sees the goal, outline, step log and error, never chat history.
- UI: run composer gained a "Model mode" picker (`runMode`), payload sends `mode`; `run_json`/lists expose `mode`.

## Step 4 (master plan) — multi-agent orchestration (done)
- Modes: `team` (executor → REVIEWER, one repair round on findings) and `crew` (sequential
  specialist pipeline). `normalize_mode` accepts ""/hybrid/team/crew. Crew line-up is a JSON
  list of known roles (1–6 stages, default executor → reviewer), validated at create time;
  an all-inspector crew is refused when file writes are allowed.
- Orchestrator state is the step log itself: stage k = number of recorded `handoff` steps
  (`crew_stage_index`, clamped to the last stage). Each stage's done becomes a handoff step;
  only the final stage's done ends the run. Team mode logs a `review` step with the reviewer
  verdict and findings on ISSUES.
- Desktop UI: run composer offers Default/Hybrid/Team/Crew with an optional comma-separated
  roster field (shown only for crew); run cards display the mode and, for crew, the roster
  chain. `/api/runs` runs expose `mode` + `crew`.
- Tests: parse_crew defaults/role validation/writer rule, crew_stage_index clamping,
  reviewer verdict interpretation, permission gates with the crew field (120 desktop tests).
- E2E orchestration verified in-test against a scripted mock router (real run loop, real /api/runs route, real SQLite step log): crew handoff (write -> handoff -> read -> done, handoff names the next reviewer stage), and the team review round (review ISSUES -> handoff repair -> second review APPROVE -> succeeded, workspace file actually fixed).
- Multi-client parity closed: team/crew/profile ported to the VS Code standalone server
  (agent.ts: normalizeMode/normalizeProfile/parseCrew/crewStageIndex/interpretVerdict,
  profile-aware buildMessages + allowed() gating, staged stepOnce, teamReviewDone;
  lib.ts /api/runs forwards profile/mode/crew; summaries expose mode+crew for the run
  cards; hybrid answers with an honest "needs the desktop app" 400). E2E crew handoff,
  team repair round and desktop-identical validation errors asserted in test/lib.test.mjs
  (25/25 extension tests, tsc clean).

## Step 5 (master plan) — DRAFT: resilient multi-agent runs
Goal: crews survive real workloads without lying about progress.
Status: budget item built end-to-end (2026-11-02); templates/resume/golden-run still open.
1. DONE - Per-stage budgets: agent_runs.stage_max_steps (0=off) + stage_policy
   (""/"fail" stops honestly, "skip" records the incomplete stage as a handoff and
   continues; the FINAL stage can never be skipped). Enforced in run_loop (desktop)
   and stepOnce (extension) with the step count restarting at each handoff. POST
   /api/runs accepts stagePolicy/stageMaxSteps (crew-only); run summaries expose both;
   composer sports a budget field + policy picker shown only for crew runs.
   Tests: Rust E2E budget skip + budget fail, policy validator unit; extension E2E
   budget skip + policy validation case (desktop 123/123, extension suite rerun pending).
2. Resume from stage k: reuse the approval-resume path - the step log is already the
   orchestrator state, so "run again" continues the current stage without a new run.
3. Crew templates: named rosters stored beside workflows; composer preset chips; POST
   /api/runs keeps accepting raw line-ups (same validation).
4. Cross-client golden run: one scripted mock-router scenario asserted byte-identical on
   the Rust suite and the extension suite (step sequences already match).
Acceptance: scripted tests cover budget exhaustion + resume + templates; desktop and
extension produce identical step sequences for the same script.

## Step 6 (master plan) - DRAFT: adaptive, accountable orchestration
Goal: reuse the M6 adaptive-routing machinery for crews and close the loop on quality.
1. Profile-aware tiers: inspector steps (review/planner thinking, reads) proposed on the
   cheap tier, builders + repairs on strong - extend choose_tier with the acting profile;
   opt-in adaptiveRouting unchanged.
2. Per-role accountability: tag agent model calls with run_id+profile on request_log so
   run cards can show who spent what (tokens, latency) - read-only SQL, no new engine.
3. Crew insights: success rate and findings-by-role from review steps surfaced in the
   existing usage/insights area.
4. Quality gate (opt-in per run): require the final crew stage to be review, and
   auto-append a debugger stage when review keeps reporting issues.
Acceptance: adaptive choice recorded per step like M6; insights are plain SQL; no silent
fallback beyond the existing tier failover.

## Next
- Android: run-composer mode picker + roster field, capability-gated (blocked on no device/emulator here).
- Release: push the round and tag (needs an explicit ask). Android/gradle unit tests still blocked (no gradle/wrapper on this machine).
- Release: push the round and tag (needs an explicit ask). Android/gradle unit tests can’t run on the current machine (no gradle/wrapper).

## Step 1 close-out (release pipeline repair)
- Release failure diagnosis from the v1.25.0 workflow run 41007439650: deb job failed in the headless build (`build.rs` ran `tauri_build::build()` unconditionally — panic "missing cargo:dev" without the `gui` feature); Linux `release` job failed at `cargo deb`; vscode job failed at `npx tsc --noEmit` (2 real type errors). Also: every version in tree still said 1.22.0 while the tag said v1.25.0 (Windows zip asset was literally `sarthi-studio_1.22.0_windows-x86_64.zip`).
- Fixes: `build.rs` gates tauri-build on `CARGO_FEATURE_GUI`; `agent.ts`/`lib.ts` type errors fixed (tsc clean, 22/22 tests, vsix packages); versions bumped to 1.25.0 in app Cargo.toml+lock, extension package.json, android versionName 1.21.1→1.25.0 (versionCode 18).

## Step 2 (master plan) — message-flow trace (B1)
- Chat: `POST /api/chat` → `deepsearch_phase` (optional native-provider pre-phase) → `prepare_chat` (saves user message, enforces privacy on code attachments, resolves media) → thread history via `history_limit` recent rows → workspace outline (optional) → `context::pack` (token-budgeted: budget 6k default, 2–24 history messages, old code attachments replaced by markers, head/tail cropping) → local mode routes to Ollama (`local::chat_once`) else `call_router` → POST `{router_url}/chat/completions` (bearer, `stream:false`) → assistant message persisted, `request_log` row written (model, tokens, latency, fallback attempts), event bus notified.
- Streaming: same prep, `POST /api/chat/stream` → `chat_stream` folds provider SSE into UI frames.
- Agent runs: `POST /api/runs` → persisted RunRow + steps → `agent::run_driver` loop → `step_once`: territory/gate check → adaptive tier pick → `build_messages` (goal + capped step history) → `call_model` (tier override or base) → reply parsed as one JSON action: think/done/fail → terminal; else `execute_action_step` (permission gate → approval gate → tool with checkpointer → log) — checkpoint/diff/restore per edit.
- Simple-message token path already capped by `context::pack` (B2: verify live request sizes; B3: multi-turn window behavior).

## Blockers
- No Ollama runtime on this machine: runtime detection, model listing, pull and local chat verified as honest failure paths + unit tests. Hybrid mode was verified end-to-end against a mock of Ollama's `/api/chat` contract (plan → local step → escalation → retry → write → done); a live local-inference run still needs Ollama installed (`ollama pull qwen2.5-coder:1.5b`).
- Live Android device/emulator: none provisioned (gradle build + unit tests + lint only).
- Later-turn environment: `gradle` not on PATH — binary found at `~/.local/opt/gradle/gradle-8.9/bin/gradle`; JDK used: `/usr/lib/jvm/java-17-openjdk-amd64`.
- No macOS runner locally (M5 CI jobs execute only on GitHub's `macos-latest`); signing credentials absent.
- No provider credentials here: adaptive tiering verified by unit tests, not a live routed run.
