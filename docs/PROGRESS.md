# Progress checklist

Persistent record of what is done, what is in flight and what is next. Update on
every shipped commit. Status values: `[x]` verified with evidence, `[~]`
partial, `[ ]` not started.

## Release line

| Version | Commit | Content | Release evidence |
|---|---|---|---|
| 1.26.2 | `75c4192` | LF restore + CR-safe version reads | workflow YAML was invalid, no run |
| 1.26.3 | `58cbea1` | Valid release workflow; first release with assets | 8 assets, macOS missing |
| 1.26.4 | `92b6817` | Single version parser, artifact gate, regression tests, release concurrency, spec docs | run `41869072420`: deb/windows/vscode/android success, Linux AppImage failed, macOS job never acquired |
| 1.26.5 | `3692edc` | AppImage AppStream attempt + macOS on an Intel runner | run `41882152731`: macOS exit 101 (missing aarch64 std), Linux failed `validate-tree` |
| 1.26.6 | `76b9fd1` | validate-clean AppStream identity (RDNS id, OARS rating), both Darwin targets | run `41884439627`: **all six jobs success, 9 assets** |
| 1.26.7–1.26.23 | `8ff7c5e` | Router manager completed (UI, install plans, pinned images, verified downloads, update/uninstall), resume for runs, 175 tests | in-tree evidence only; no tag cut yet |
| 1.27.0 | `1b91b32` | Full spec audit in `FEATURE-STATUS.md`; every manifest on 1.27.0 | `cargo test --locked`: 175 passed, 1 ignored |
| 1.28.0 | `ed30132` | Phase 3 capability table from provider metadata only, Ollama `/api/show` probe, capability line in the model menu | `cargo test --locked`: 183 passed, 1 ignored |
| 1.29.0 | `a389789` | Phase 4 backend: Thinking levels + precedence + `@Thinking`, native OpenAI Platform (Responses API), reasoning summaries as SSE frames, per-project Thinking column | `cargo test --locked`: 204 passed, 1 ignored; clippy clean |
| 1.30.0 | `a0c76d8` | Phase 4 frontend: Think control + `@Thinking` hint + reasoning card + OpenAI Platform settings + `openai:` models in the menu; headless launch fixes | live smoke on :8799 — levels, 400-for-no-key, config round-trip, project column |
| 1.35.0 | this commit | Phase 7 agent half: an `mcp` action in the agent loop (run-scoped opt-in, once-per-run catalogue in `mcp-tools`, approval pause naming the tool and arguments, shared `mcp::invoke` call path, `mcp: N tools` on the run card) + the `list_runs` column/`row_to_run` drift fixed | `cargo test --locked`: 234 passed, 1 ignored; clippy `-D warnings` clean; live run on :8811 against a real stdio MCP server — 2 tools discovered, `think` then pause `call MCP tool echo.echo with {"text":"hello from saarthi"}`, approve → step `mcp` output `echo: hello from saarthi` → `done`; deny rule on `echo.echo` → step refused and run failed naming the rule; auto-approve run called the tool with no pause; runs list now returns rows (3/3) instead of 0 |
| 1.34.0 | `f7b668b` | Phase 7 UI: Settings → MCP panel (register/edit/test/remove, tool cards with required args, schema-shaped JSON template, explicit approve-then-call, raw result) + `env` preserved when a save omits it | live browser pass on :8795 — server saved and listed, Test discovered `echo`/`ping`, the template pre-filled `{"text": ""}`, first call answered `ask` and flipped the button to "Approve and call", second call returned `echo:` |
| 1.33.0 | `79cc9ab` | Phase 7 first slice: MCP registry (`mcp.json`), stdio JSON-RPC handshake per operation, tool discovery with real schemas, argument validation, every route policy-gated on `mcp_tool` | live end-to-end on :8796 with a real stdio server — test listed `echo` with its schema, an approved call returned `echo: hi`, a missing argument and a wrong type were refused 400, an unknown tool 404, the default `ask` 409, and both decisions landed in the audit log |
| 1.32.0 | `010566b` | Phase 10 UI: Settings → Permissions panel (add/revoke rules, audit trail), re-read on every visit | live browser pass on :8797: rule added through the form, badge + audit row appeared, revoke removed it |
| 1.31.0 | `7c949c3` | Phase 10 first slice: central permission policy store (rules + scopes + resolution), agent-step and router-install enforcement, `permission_audit` table and routes | `cargo test --locked`: 214 passed, 1 ignored; live grant/audit/revoke smoke |

## Phase 1 — CI/CD and release reliability

- [x] Diagnose the CR-in-artifact-name failure (`41693462775`) to the CRLF manifest rewrite
- [x] Restore LF manifests and normalise the 12 files the bump script flipped
- [x] Make the workflow valid again (block-scalar version reads, YAML parse check)
- [x] Publish a release with real assets (`v1.26.3`, 8 assets + checksums)
- [x] One version parser for Linux/macOS/Windows (`scripts/ci/version.sh`) — `Read package version` green on all three in `41884439627`
- [x] Version validation: non-empty + semver shape (9 manifest cases in the regression suite)
- [x] Artifact-name validation before upload (`scripts/ci/verify-artifacts.sh`) — ran green on ubuntu, macOS and Windows runners
- [x] Checksum verification inside the artifact gate
- [x] Regression suite for LF/CRLF/empty/malformed versions + bad names/paths/checksums (`20 passed, 0 failed`)
- [x] Wire the regression suite into `desktop-ci.yml` (runs on every push)
- [x] Release `concurrency` so two tags cannot race the same asset name
- [x] AppStream metadata that validates: `appstreamcli validate-tree` clean, AppImage built
- [x] macOS job actually runs and attaches its tarball (`free-ai-suite_1.26.6_macos-universal.tar.gz`, both arches verified by `lipo` in-job)
- [x] Install smoke on the produced Windows artifact (local: release build `EXIT=0`, headless HTTP 200, brand string present, checksum verified)
- [ ] Install smoke on deb / AppImage / vsix / apk (needs the respective platform; only the Windows zip could be launched on this machine)
- [ ] Decide fate of dead tags `v1.26.0`/`v1.26.1`/`v1.26.2`

### Phase 1 verdict per previously diagnosed issue

| Issue | Verdict | Evidence |
|---|---|---|
| A. Rust Agent syntax error | **Not reproducible** | `cargo test --locked`: 127 passed, 0 failed; release build clean at 1.26.4/1.26.6. `agent.rs` compiles and its SQL paths are covered by tests. Left unchanged rather than rewritten blindly. |
| B. CR in Linux version | **Fixed** | Single parser; `sed` output stripped of `[:space:][:cntrl:]`; CRLF manifest is a test case. |
| C. macOS version regex | **Fixed** | macOS uses the same `scripts/ci/version.sh`; step green in `41884439627`. |
| D. Artifact upload failure | **Fixed** | Gate rejects control characters/spaces and verifies checksums before every upload; upload steps green on all three OSes. |
| E. AppImage AppStream warning | **Fixed, and it was fatal** | appimagetool runs `appstreamcli validate-tree` and aborts on non-zero. `validate-tree` fails on *warnings*, so two iterations were needed: the non-RDNS id and the missing OARS rating. Now clean; AppImage attached. |
| F. macOS job cancellation | **Diagnosed and fixed** | GitHub's own annotation: "The job was not acquired by Runner of type hosted even after multiple attempts", plus macOS-arm64 capacity notices. `macos-latest` is arm64; the job moved to `macos-15-intel` and then failed for a real reason (only the x86_64 std installed, exit 101), which was fixed by installing both Darwin targets. |

## Foundations running alongside Phase 1

- [~] Permission engine — per-run flags + route guards exist, no central policy store (Phase 10)
- [x] Specification audit at 1.27.0 — per-phase delivered/remaining with the evidence read from the tree (`FEATURE-STATUS.md`)
- [~] Observability — `request_log` + SSE, no structured logs/correlation IDs
- [x] Spec documents (`ARCHITECTURE`, `IMPLEMENTATION-SPECIFICATION`, `MASTER-ROADMAP`, `TESTING-STRATEGY`, `KNOWN-ISSUES`, `FEATURE-STATUS`, this file)
- [ ] Frontend test runner
- [ ] End-to-end harness (headless launch + mock router)

## Phase 2 — Router Manager

Slices 1–3: `app/src/router_registry.rs` holds the registry and platform
capability layer (slice 1), the live probes behind `/api/routers` (slice 2) and
the per-router connection tester and restart (slice 3). It reuses `router.rs` for
lifecycle and points at its facts rather than re-deriving them.

- [x] Router registry with the §5.1 fields (id, name, description, OS, arch, install methods, config location, health probe, docs)
- [x] Architecture normalisation for real release-asset spellings (`aarch64`/`amd64`/`x64`)
- [x] Platform gating: Android never claims a local install; remote stays available
- [x] Only routers with an existing lifecycle in the crate are listed (no aspirational entries)
- [x] Tests: unique slug ids, required fields, health probe present, HOME-relative config, Android exclusion, unknown arch rejection, stable JSON keys
- [x] Live probes per router (installed / version / runtime / healthy / ownership) reusing `router::status`, `router::omniroute_status` and `local::fetch_version`
- [x] `healthy` is "the health probe answered", never "the process exists" (unit-tested both ways)
- [x] `/api/routers` route returning capabilities + measured state + host platform (smoked: HTTP 200, three routers, real Windows host values)
- [x] Probe latency: the three probes run concurrently (1.26.14), so the endpoint costs the slowest probe instead of their sum
- [x] UI: the Router view lists every registered router with its measured state (1.26.13)
- [x] Router UI: list, health, start/stop (1.26.13)
- [x] Router UI: connection tester and restart — real status code and latency per router, restart through the existing stop/start paths so ownership rules are unchanged (1.26.15; verified against `scripts/mock-router.py`, 200 in 1 ms, plus the no-answer and no-lifecycle paths)
- [x] Router UI: logs — `GET /api/routers/:id/logs` reads the real container output (both streams), clamps the tail to 1–1000 lines and caps the response at 64 KiB; the local runtime is refused with its reason instead of showing an empty box (1.26.16)
- [ ] Router UI: update, uninstall, configuration editor
- [x] Install plan preview per router — exact argv, writes, verification wording and measured prerequisites, with no side effects (1.26.18)
- [x] Approval gate — the install POST needs `approve` plus the plan digest, and refuses before spawning anything when unsupported, blocked or changed; failed installs are recorded too (1.26.18)
- [x] Install records (`router-installs.json`) with a read route (1.26.18)
- [x] Image installs pinned to the registry manifest digest, with the unresolved-digest refusal and Start no longer creating containers (1.26.19)
- [x] Checksum-verified downloads: an asset is fetched while hashing, discarded on mismatch, and unpacked only after it verifies; Ollama installs from its official release and the hash that release publishes (1.26.20)
- [x] FreeLLMAPI installs from its published `ghcr.io` image pinned by digest, and its lifecycle brings up whichever deployment exists — compose project or Studio's container (1.26.21)
- [x] Update check, approval-gated update that pulls before replacing, and uninstall that keeps the data volume unless `purge` is explicit (1.26.23)
- [ ] Router UI: update, uninstall, configuration editor
- [x] Per-router launch preferences stored atomically, with `auto_start` in `/api/routers` and a "Start with Studio" checkbox (1.26.17)
- [x] Auto-start on launch that never stops externally owned routers — the sweep starts only an enabled, installed, not-already-answering router, and the rule is unit-tested both ways (1.26.17). A boot with Docker down records the failure per router and the app still opens

## Phase 3 — Model and provider manager

- [x] Capability table sourced only from provider metadata (`capabilities.rs`) — modalities, context limit, streaming, tools, reasoning, structured output, price; unknown stays `null` (1.28.0)
- [x] Context limit read once, including OpenRouter's nested `top_provider.context_length` (1.28.0)
- [x] Local runtime description per model: `GET /api/local/model-capabilities?name=` → `/api/show` capabilities + real context length, 404 for a model that is not installed (1.28.0)
- [x] Model menu shows reported capability marks and one honest summary line (1.28.0)
- [ ] Per-agent / per-project / per-task model scope (only global + per-message override today)
- [ ] Client-side fallback policy, retry and provider health monitoring (only on-demand `/api/test`)

## Phase 4 — Thinking and the OpenAI Platform

Backend half shipped in 1.29.0; the composer half is the next commit.

- [x] `thinking.rs`: four levels, provider-default when unset, fixed precedence (message > agent > project > global) — unit-tested both ways (1.29.0)
- [x] `@Thinking` / `@Thinking:high` / `\@Thinking` directive parsing; unknown level, mid-sentence mention and empty prompt all stay literal (1.29.0)
- [x] A level maps to `reasoning: {effort, summary}` on Responses and `reasoning_effort` on Chat Completions; Off sends no parameter (1.29.0)
- [x] Router models gated on the catalog's `capabilities.reasoning`: true → send, false → refuse with a reason, unknown → send nothing (1.29.0)
- [x] Platform model rejecting the parameter: one retry without it, refusal remembered per process, `reasoning_dropped` in meta (1.29.0)
- [x] `openai:` ids route through chat, streaming chat and agent calls; usage incl. `reasoning_tokens` recorded (1.29.0)
- [x] Reasoning summaries as their own SSE frame, from the API's summary channel (1.29.0)
- [x] `projects.thinking` migration + create/update/list; resolution uses the active project (1.29.0)
- [x] Routes: `/api/openai/models`, `/api/openai/test`, `/api/thinking/levels`; config carries both Thinking defaults and `has_openai_key` (1.29.0)
- [x] Composer Think control with the four levels + model default, fed by `/api/thinking/levels` (1.30.0)
- [x] `@Thinking` hint that mirrors the server's rules, including the `\@Thinking` escape (1.30.0)
- [x] Reasoning-summary card in the streaming row, from the provider's summary frames (1.30.0)
- [x] Settings → OpenAI Platform: write-only key, base URL, global and agent-run Thinking defaults, key test (1.30.0)
- [x] Platform models in the composer's model menu under `openai:` ids (1.30.0)
- [x] Headless launch fixed: a `--no-default-features` build serves instead of shutting down at once (1.30.0)
- [ ] Tool calling and structured output through the platform path (the request shape supports them; no agent tool loop uses `openai:` yet)
- [ ] `@Thinking` mention in the composer's slash/prompt helpers so discoverability does not depend on docs

## Phase 10 — Safety and permissions

- [x] Central policy store: action × target × decision × scope, `permissions.json` written atomically with private permissions (1.31.0)
- [x] Fixed resolution order, tested: longest target → deny/ask/allow → narrower scope; reads default allow, destructive actions default ask (1.31.0)
- [x] `once` rules are consumed by use; `session` rules die with the process; `project`/`always` persist (1.31.0)
- [x] Agent steps and router installs are actually gated by the store, and a veto names the rule in the run's log (1.31.0)
- [x] `permission_audit` table + `/api/permissions/audit`; grants, revokes, refusals and approved installs are recorded (1.31.0)
- [x] Routes: list, grant, revoke (local-origin guarded) (1.31.0)
- [x] Settings → Permissions UI: add a rule, list rules in force, revoke, read the audit trail (1.32.0)
- [ ] Wire the remaining privileged paths: terminal open, workspace writes from the editor, git, updates, settings/key changes
- [ ] OS keychain for `api_key` / `search_api_key` / `openai_api_key` (still mode-restricted `config.json`)
- [ ] Global emergency stop covering every agent run and terminal at once

## Phase 7 — MCP

- [x] Registry in `mcp.json`: name, command, args, environment, enabled; env values stored but only names returned (1.33.0)
- [x] Real stdio JSON-RPC session: `initialize` → `notifications/initialized` → `tools/list` → `tools/call`, one message per line (1.33.0)
- [x] 20-second ceiling per operation, child killed on every exit path, stray stdout lines ignored rather than treated as answers (1.33.0)
- [x] Tool discovery keeps each tool's `inputSchema`; calls are validated against `required` and declared scalar types before being sent (1.33.0)
- [x] Every process-starting route gated on `mcp_tool` (`server.tool`, server name for a test): deny → 403, ask → 409 `needs_approval`, approved retry proceeds, all audited (1.33.0)
- [x] Routes: list, save, remove, test, call (mutations behind the local-origin guard) (1.33.0)
- [ ] Persistent connection pool / health state / captured server logs / reconnect backoff
- [ ] SSE and streamable-HTTP transports (stdio only today)
- [x] Manager UI in Settings: register/edit/test/remove servers, tool cards, schema-shaped argument template, approve-then-call, raw result (1.34.0)
- [x] Agent integration: an `mcp` action in the agent loop, gated on a per-run `allowMcp` flag, refused when no server is enabled, paused for approval, vetoed by `mcp_tool` deny rules on `server.tool`, and executed through the same `mcp::invoke` the HTTP route uses (1.35.0)
- [x] Catalogue discovered once per run into `agent_runs.mcp_tools` and logged as an `mcp-tools` step, so step prompts never spawn a server (1.35.0)
- [x] Run surface: `allowMcp` on create/list/get, "May call MCP tools" on the run form, `mcp: N tools` on the run card (1.35.0)
- [ ] Per-step rediscovery / tool list refresh mid-run (the snapshot is fixed at run start)
- [x] Runs list defect found while building this: the `list_runs` SELECT omitted `model`, `row_to_run` read one column short, every row failed to map and the panel read "No runs yet"; `RUN_LIST_SQL` is now a constant with a mapping test (1.35.0)

## Next up

1. Phase 7 close-out — MCP health/log state, reconnect, then SSE/HTTP transports.
2. Phase 10 close-out — remaining privileged call sites (terminal, editor
   writes, git, updates, settings), keychain, global stop.
3. Phase 2 close-out — configuration editor plus dependency/package-manager
   probing, so the router manager stops being "80 % delivered".
