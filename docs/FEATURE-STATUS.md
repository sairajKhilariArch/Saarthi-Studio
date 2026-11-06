# Feature status report — audit of `main` @ 1.28.0

Read against the master specification phase by phase. Status vocabulary:
**Working** (route + persistence + UI verified in code or by a recorded run),
**Partial** (exists but a stated requirement is unmet), **Missing** (no
implementation), **Broken** (exists and fails).

## Evidence for this audit

| Check | Command | Result |
|---|---|---|
| Rust suite | `cd app && cargo test --locked` | **183 passed, 0 failed, 1 ignored** (2026-11-05) |
| Route registrations | `grep -c '\.route(' app/src/{main,agent,host_api}.rs` | 37 / 7 / 8 |
| Test functions in tree | `grep -rc '#\[tokio::test\]\|#\[test\]' app/src` | 182 |
| Last green release | run `41884439627` (`v1.26.6`) | all six jobs success, 9 assets |
| Release line | `8ff7c5e` … `1.27.0` | 1.26.4 → 1.26.23 all shipped, LF manifests |

The counts above are what the tree contains today, not a projection.

## Verdict in one paragraph

Phases 1 and 2 are effectively delivered (Phase 1 needs only install smokes and
the dead-tag decision; Phase 2 needs only a configuration editor and
dependency/package-manager probing). Phases 5 (run engine + team/crew
orchestration), 9 (workspace + PTY), 10 (guards), 11 (context/usage), 14
(clients) and 16 (backend/client tests) are largely there. **Phase 4 (Thinking +
OpenAI Platform) and Phase 7 (MCP) have no implementation at all**, Phase 13 is a
flat saved-prompt list rather than a workflow graph, and Phases 3, 6, 8, 12, 15,
17, 18 are partial in the specific ways listed below.

## Phase 1 — CI/CD and error diagnosis

| Item | Status | Evidence / gap |
|---|---|---|
| One version parser for all OSes | Working | `scripts/ci/version.sh` strips `[:space:][:cntrl:]`, validates semver |
| Version regression suite | Working | `scripts/ci/test-version.sh`, wired into `desktop-ci.yml` |
| Artifact-name + checksum gate | Working | `scripts/ci/verify-artifacts.sh` runs before every upload |
| Release concurrency | Working | `concurrency: desktop-release-${{ github.ref }}` |
| AppStream metadata | Working | `appstreamcli validate-tree` clean; AppImage attaches |
| macOS job | Working | `macos-15-intel`, both Darwin targets, universal tarball |
| All six release jobs green | Working | run `41884439627`, 9 assets |
| Install smoke on deb/AppImage/vsix/apk | **Missing** | only the Windows zip was launched locally |
| Dead tags `v1.26.0/1/2` | Open | documented as failed; no decision to delete |

Issue A (agent SQL syntax) is **not reproducible**: `agent.rs` compiles and its
query paths are covered by tests — left unchanged rather than rewritten blindly.
Issue F (macOS cancellation) was diagnosed as runner acquisition, not code.

## Phase 2 — Router manager

| Item | Status | Notes |
|---|---|---|
| Central registry with §5.1 fields | Working | `router_registry.rs`; only routers with a real lifecycle are listed |
| OS/arch gating, Android never claims a local install | Working | `capability()` + tests |
| Detection: installed/version/runtime/healthy/ownership | Working | `/api/routers`, probes run concurrently (1.26.14) |
| `healthy` means the probe answered | Working | unit-tested both ways |
| Approval-gated install with plan digest | Working | 1.26.18 |
| Pinned images, verified downloads | Working | 1.26.19/1.26.20/1.26.21 |
| Update + uninstall (data kept unless `purge`) | Working | 1.26.23 |
| Auto-start sweep, external routers never stopped | Working | 1.26.17, rule tested both ways |
| UI: list, health, start/stop, restart, tester, logs | Working | 1.26.13–1.26.16 |
| **Configuration editor** | **Missing** | no route, no UI |
| Dependency/package-manager probing (Docker, npm, brew) | Missing | registry gates OS/arch only |
| Remote service routers | Missing | registry is local-process only |

## Phase 3 — Model and provider manager

| Item | Status | Notes |
|---|---|---|
| Provider presets | Working | `app.js` / `index.html` |
| Model discovery with cache | Working | `/models`, 1-minute cache keyed by base + key |
| Local Ollama models | Working | `local.rs` (status, tags, hardware, pull with progress) |
| Context window where a provider reports it | Partial | `main.rs:1181` reads `context_window`; `app.js:2704` displays it — nothing is inferred when absent |
| Capability table (modalities, tools, reasoning, structured output) | Working (1.28.0) | `capabilities.rs` from `/models` rows and Ollama `/api/show`; unknown stays `null` |
| Price table | Partial (1.28.0) | carried when the provider publishes `pricing.prompt`/`completion`; never inferred |
| Model scope (task > agent > project > global) | Working (1.46.0) | `model_scope::resolve` with a reported origin; `projects.model`, `agent_model`, and a per-run `model`; the composer pick stays the task scope |
| Fallback / retry | Working (1.50.0) | ordered, capped, deduplicated chain that retries only an unavailable provider and never replays a half-streamed answer; every attempt lands in meta. A circuit breaker (`breaker.rs`) opens after three consecutive unavailable answers, refuses the model for 45 s, then allows one probe; a skipped candidate is recorded as `circuit open: skipped` and the whole-chain refusal names each model's remaining seconds |
| Provider health monitoring | Working (1.50.0) | `GET /api/health/providers` merges the breaker's live state with 24 h of `request_log` (requests, successes, average latency, the provider's own last error) and states the opening rule; shown as a Provider health card on the usage page. `/api/health` remains the cheap liveness answer |

## Phase 4 — Thinking and OpenAI Platform

Backend in 1.29.0, composer and settings in 1.30.0.

| Item | Status |
|---|---|
| Native OpenAI Platform provider (Responses API) | Working (1.29.0) — `openai.rs`, own key, `openai:` model ids, buffered + streaming + agent calls |
| Thinking control (off/low/medium/high/model default) | Working (1.30.0) — composer Think control, levels from `/api/thinking/levels` |
| `@Thinking` shortcut + literal escape | Working (1.29.0/1.30.0) — stripped server-side before storage, hinted in the composer, `\@Thinking` stays literal |
| Scope precedence (message > agent > project > global) | Working (1.29.0) — `projects.thinking` column migrated in place, agent scope from config |
| Reasoning summaries / progress surfacing | Working (1.30.0) — collapsible card fed by the provider's summary frames |
| Platform key in Settings (write-only) + key test | Working (1.30.0) — `/api/openai/test`, no tokens spent |
| Tests | Working — 21 new cases, 204 total; live smoke on :8799 for the new routes |
| Tool calling / structured output through the platform path | Missing — the request shape allows it, no agent tool loop uses `openai:` yet |

## Phase 5 — Agent execution engine

| Item | Status | Notes |
|---|---|---|
| Real execution loop with tools | Working | `agent.rs`: think/list/read/write/command/done/fail |
| Approval pauses, checkpoints, diffs, restore | Working | `awaiting_approval`, per-step checkpoint (200 kB cap) |
| Cancellation, crash recovery (`interrupted`) | Working | `ACTIVE` map; startup sweep |
| Pause / resume between steps | Working (1.52.0) | `POST /api/runs/:id/pause` parks a live run at the next step boundary (never mid-action), writes a `pause` step, sets `paused` and releases its control entry; `resume` continues from the same step log. A pause is durable across a restart and is only refused when there is nothing to stop (a run awaiting approval) |
| Resume from the step log | Working | `POST /api/runs/:id/resume` + 2 tests |
| Team (executor → reviewer → repair) | Working | `review` step with verdict/findings |
| Crew pipeline + per-stage budgets/policies | Working | `crew` roster 1–6, `stage_max_steps`, fail/skip |
| Hybrid cloud-plan / local-execute | Working | plan step, bounded escalation |
| Multi-agent parity in VS Code | Working | `agent.ts` port |
| Task queue, priority, dependencies | Working (1.53.0) | every run is created `queued` and admitted by one dispatcher pass ordered by `priority` (-10…10) then age; `dependsOn` is validated at creation (unknown/self/loop/empty/>8 all 400) and a dependency is only satisfied by `succeeded`, while a dependent of a failed/cancelled/interrupted run is failed with the dependency's name. A queued run survives a restart and is admitted at boot; pause names it as queued, cancel and the emergency stop include it |
| Retry from scratch (`POST /api/runs/:id/retry`) | Working (1.55.0) | starts a clean run from a finished run's settings with `retry_of` linking back; the step log, tokens, duration, error and dependencies are not carried over, and a `retry` step names the original. Refused for a run that has not finished; a paused run is told to resume instead |
| Concurrency limit (`max_concurrent_runs`) | Working (1.53.0) | clamped 1–4, default 1; a run waiting for approval still holds its slot, a paused or queued run does not. Above 1, writes to a claimed or changed file are refused per run (1.54.0) |
| Conflict detection before a write | Working (1.54.0) | `app/src/filelock.rs`: a `write` is refused as a non-fatal `write-conflict` step when another live run has claimed the path or the file on disk is not what this run last read; the run re-reads and continues. Claims are dropped for any run no longer `running`/`awaiting_approval` and released by `finish` |
| Parallel execution + worktree isolation / file locks | Partial (1.54.0) | leases guard the agent's own `write` only — a command step, an editor save or another program is caught after the fact by the fingerprint check; there is no `worktree` per run and the ledger is in-process, so `max_concurrent_runs` still defaults to 1 |
| Verification step before `succeeded` | Working (1.56.0) | a run that wrote, ran a command or called a tool goes to the verifier, which must answer `APPROVE`; ISSUES hands the findings back for one repair round and a second ISSUES ends the run `failed` with the findings kept. Read-only runs and `team` mode are not verified twice. No human sign-off step: the verifier is a model on the same router |
| Ruflo | Not integrated (spec says optional) | Saarthi Core keeps ownership |

## Phase 6 — Skills

| Item | Status | Notes |
|---|---|---|
| Discovery across 5 roots | Working | `skills.rs` |
| Enable/disable persisted | Working | `POST /api/skills/:id/state` |
| Instruction injection with token cap | Working | rides the next message |
| Install / update / remove / version tracking | Working (1.49.0) | install writes `<user skills>/<slug>/SKILL.md` after validating it with the list parser (temp file + rename, refuses overwrite unless `replace`); update rewrites in place and keeps the id; remove moves the folder to `<root>/.removed/` instead of deleting; `skills.json` records version, install/update times and a content fingerprint, so “edited since install” is reported; only the user root is writable |
| Skill permissions / usage history | Partial (1.49.0) | usage history is real (`skill_usage`: id, time, conversation, tokens; `uses`/`last_used_at` per skill and `/api/skills/:id/usage`); writing routes are gated on `fs_write`. There are no per-skill permissions because a skill is prompt text and is never executed |
| Automatic skill selection | Working (1.57.0) | `skills::select_for`: id/name words score 4, description words 2, noise words and numbers dropped; needs 2 points, at most two skills, id tie-break, 1,200-token budget, disabled and broken skills skipped. A manual pick always wins, and `auto_select_skills` (default on) turns it off. The reply meta names what was attached (`auto_skills`) |

## Phase 7 — MCP

| Item | Status | Notes |
|---|---|---|
| Server registry | Working (1.37.0) | `mcp.json` beside the config; `transport` (stdio/http/sse), `url` for remote servers, `env` for stdio and write-only `headers` for remote — values stored, only names returned; an entry without `transport` reads as stdio |
| Connection lifecycle | Working (1.39.0) | a live session per server is reused across calls (handshake skipped, tool list re-read), warm for five idle minutes, evicted on save/remove and on a failed request, with a fresh-handshake fallback; a dropped stdio child is killed and an SSE reader aborted |
| Tool discovery | Working (1.33.0) | `tools/list`, schemas kept |
| Schema validation | Working (1.33.0) | `required` presence + declared scalar types; not full JSON Schema |
| Permission binding | Working (1.33.0) | `mcp_tool` on `server.tool`; ask/deny/allow, audited |
| Health, logs, reconnect | Working (1.36.0) | health recorded from a real handshake (`serverInfo.version`, protocol, tool count, latency, time), `unknown` before any contact; stderr and client events in a 200-line in-memory ring; one handshake retry after 250 ms, never a retried `tools/call`; `/api/mcp/health`, `/api/mcp/health/check`, `/api/mcp/logs?name=`; an idle supervisor pings each pooled session every 60 s with a 5 s budget and evicts only silence or a dead transport (1.45.0) |
| SSE / HTTP transports | Working (1.37.0) | streamable HTTP (one POST per message, JSON or SSE answer, `Mcp-Session-Id` carried forward) and legacy HTTP+SSE (GET stream, `endpoint` event, id-matched answers); both share handshake, catalogue, validation, gating and health/log state with stdio |
| Agent use of MCP tools | Working (1.35.0) | `Action::Mcp` with a per-run `allowMcp` flag; catalogue discovered once into `agent_runs.mcp_tools` and logged as `mcp-tools`; approval pause names the tool and arguments; `mcp_tool` rules on `server.tool` veto; execution goes through the same `mcp::invoke` as the route |
| Per-step tool rediscovery | Working (1.38.0) | a failed `mcp` step re-reads the catalogue before the repair step; the snapshot's `refreshes` counter bounds it to three per run and survives a resume |
| MCP manager UI | Working (1.34.0) | Settings → MCP: register/edit/test/remove, tool cards, argument template, approve-then-call, raw result |

## Phase 8 — Project knowledge and memory

| Item | Status | Notes |
|---|---|---|
| Conversation history persistence | Working | `conversations`/`messages`, Android mirror |
| Workspace text search | Working | `/api/workspace/search` |
| Embeddings *probe* | Partial | `media.rs` proxies one `/api/embeddings` call; nothing is stored or indexed |
| Semantic search over the code index | Working (1.63.0) | `app/src/embeddings.rs`: indexed symbols are embedded through the configured router and cached in `embedding_cache` by model + symbol + file digest, ranked by cosine similarity, served by `GET /api/index/semantic?q=`. Off until `embedding_model` is set (the route then answers 400), a provider error is a 502 rather than a silent fallback to the word search, and one search embeds at most 128 new documents, reporting the rest as `pending`. The Index sidebar tab (1.64.0) shows what the index holds, refreshes it, searches by words or by meaning, opens a hit's file, and carries the embedding-model field. No browser check yet |
| Codebase indexing / symbols / incremental updates | Working (1.59.0) | `app/src/index.rs`: bounded walk skipping build/dependency folders, secret-shaped files never read, privacy exclusions applied before reading, 1 MiB and NUL-byte skips, 5,000-file ceiling reported as `truncated`; symbols per language family with path + line + kind + name; refresh by content digest (unchanged untouched, changed re-extracted, deleted dropped); ranked search over names and paths; `GET /api/index`, `POST /api/index/refresh`, `GET /api/index/search?q=`. No UI yet and symbols are line scans, not a parser |
| RAG over the project | Working (1.60.0) | chat turns carry the index's ranked matches: one snippet per file, the declaration plus its opening lines read from disk at request time, labelled `path:line — kind name`, 4 snippets / 700 tokens, reported as `retrieved` in the reply meta. Word-based, so no embeddings are involved; `deepsearch.rs` remains web research, a different thing. Agent runs open with the same two blocks through `agent::run_background` (1.61.0), with memory read quietly so a run's repeated asks leave `uses` alone, and the block is stored on the run at promotion (1.62.0): `context` on the run carries the memory and code labels, the card says `given 1 memory + 1 code`, the expanded run names them, and the steps read the stored block back instead of recalling again |
| Memory records + management UI | Working (1.58.0) | `app/src/memory.rs`: `memory_records` with kinds decision/preference/task/fact, a required title, optional details and a scope (one project or every project); `GET /api/memory`, `POST /api/memory`, `POST /api/memory/:id/delete`, local-origin guarded; recall by shared words (title 3, body 1, floor 2, 6 records / 900 tokens) with `uses`/`last_used_at` and `memory_used` in the reply meta; sidebar Memory pane with form, list and forget. The model never writes a record |

## Phase 9 — Developer environment

| Item | Status | Notes |
|---|---|---|
| File tree, editor, save, workspace search | Working | `workspace.rs` + `ide.js` |
| Real PTY terminal (ConPTY/unix pty, SSE, resize) | Working | `terminal.rs`, `portable-pty` |
| Git status | Partial | `vcs_status` / `parse_git_status` reads state only |
| Git stage/commit/diff/branch/worktree checkpoints | **Missing** | — |
| Syntax highlighting, diagnostics, completion, formatting | Missing | no editor engine, by design |
| Build/test runner, problems panel, debugger | Missing | agent can run commands; no runner UI |

## Phase 10 — Safety and permissions

| Item | Status | Notes |
|---|---|---|
| Path confinement, symlink escape prevention | Working | canonicalised, `MAX_FILE_BYTES` 1 MB |
| Loopback `Host`/`Origin` write guard | Working | `host_security.rs` |
| Outbound secret detection | Working | `privacy.rs` |
| Host API bearer auth + TLS + body caps | Working | `host_auth.rs`, `host_transport.rs` |
| Central policy store (once/session/project/deny) | Working (1.31.0) | `permissions.rs`; rules scoped once/session/project/always, resolved longest-target → deny → scope |
| Privileged UI routes gated | Working (1.40.0) | terminal open, editor saves, workspace create/rename/delete and settings/key saves consult saved rules; deny → 403, ask → 409 until `approve: true`, both audited. No rule means no prompt — the click is the consent. UI routes pass an empty project, so `project`-scoped rules still apply to agent steps only. Git has no mutating route; updates have no in-app route |
| Enforcement of that store | Partial (1.31.0) | agent steps and router installs consult it; terminal, editor writes, git, updates and settings do not yet |
| OS keychain for secrets | Missing | keys live in mode-restricted `config.json` |
| Privileged-operation audit log | Working (1.31.0) | `permission_audit` table + `/api/permissions/audit` |
| Global kill switch | Missing | per-run cancel exists; no stop-everything control |
| Permissions UI | Working (1.32.0) | Settings → Permissions: add a rule, list rules in force, revoke, read the audit trail |

## Phase 11 — Token, context and cost

| Item | Status | Notes |
|---|---|---|
| Token-budgeted packing | Working | `context.rs`, 1 k–32 k clamp, VS Code parity |
| Usage log + aggregates/series/by-model | Working | `request_log`, `/api/usage` |
| Pricing, cost estimates | Working | `cost.rs` × `capabilities::Price` from the live `/models` row × the provider's reported tokens; `request_log.cost_usd`/`cost_basis`, unpriced rows counted rather than zeroed. Native OpenAI Platform picks stay unpriced (no mirrored price table) |
| Budget enforcement that stops a run | Working | `daily_budget_usd`, rolling 24 h (timestamps are UTC), checked once before the fallback chain and before a stream; refusal is non-retryable and names spend, budget and unpriced count. Zero = no limit; local inference exempt |
| Compression beyond head/tail cropping | Working | `context.rs`: whitespace normalisation, repeat-once dedup, digest of labelled excerpts capped at a quarter of the budget, first message kept as a pointer; a fitting thread is untouched and every condensed/dropped message is reported in meta. No model call, so no invented summaries |
| Context-utilisation display | Working | context meter `~used / budget · %` with an over-budget state; `context_budget` in reply meta for other clients |

## Phase 12 — Live agent monitoring

Working for monitoring (1.51.0) and controls (1.52.0), with one named gap.
`agent_runs`/`agent_steps`, SSE events and a runs panel with diffs,
approve/reject, cancel, restore and pause/resume are real backend state;
`GET /api/runs/overview` adds a cross-run dashboard (live, awaiting approval,
paused, failed, summed steps and tokens) built from the same rows as the panel,
and every run reports `tokens`, `elapsedSeconds` and `idleSeconds` with a live
run measured from its start. Pause parks a run at the next step boundary and a
paused run holds no live task, so start/pause/resume/cancel all act on real
state, and the emergency stop reaches parked runs too.

The queue is real as of 1.53.0: a run is created `queued`, takes a place in line
(`queue #N` on the card, a Queued count on the board, a waiting note with Cancel
only while it waits), can carry a `priority` and a `dependsOn` list, and starts
itself when a slot frees and its dependencies succeeded. Retry is a real control
as of 1.55.0 too, and it is distinct from resume: `POST /api/runs/:id/retry`
starts a clean run of the same goal and settings (linked by `retry_of`, logged as
a `retry` step), while resume continues an existing step log. There is no
drag-to-reorder — priority is a number on the form.

## Phase 13 — Workflow builder

Partial. `workflows.json` holds flat `{id, name, prompt, skill}` macros with CRUD
routes and a one-button run (Automation view). That is a saved prompt, **not** a
node graph: no task/agent/model/tool/conditional nodes, no dependency
resolution, no validation, no scheduler, no run history.

## Phase 14 — Cross-platform integration

| Item | Status | Notes |
|---|---|---|
| Desktop Linux/Windows/macOS | Working | deb, AppImage, tarball, zip, macOS universal tarball |
| Android client | Working (subset) | chat streaming, history, backup, remote agent runs via Host API; no router management, no local execution |
| VS Code extension | Working (subset) | embedded Node server, agent port, runs webview |
| Capability negotiation driving feature gates | Partial | `/api/host/v1/capabilities` exists; clients hand-gate a few screens |
| Pairing / TLS / sessions | Working | host token lifecycle + TLS transport |

## Phase 15 — Extension and plugin ecosystem

Missing beyond skills. No manifest (name/version/permissions/entry point),
no sandboxing, no custom tools, no provider adapters, no UI extensions.

## Phase 16 — Testing and quality

| Layer | Status | Notes |
|---|---|---|
| Rust unit/integration | Working | 183 passed / 1 ignored at 1.28.0 |
| VS Code extension | Working | `node --test` suites |
| Android | Working | JVM + emulator instrumentation + validation gate |
| Release-pipeline regressions | Working | `scripts/ci/test-version.sh` |
| Frontend | **Missing** | no runner; `app/static/` has no build step |
| End-to-end (launch → chat → agent → restart) | Missing | ad-hoc local smokes only |
| Failure-injection matrix | Missing | — |
| Install smokes in CI | Missing | — |
| Windows Rust CI job | Missing | Windows is exercised only by the release build |

## Phase 17 — Observability and diagnostics

Partial. `request_log`, SSE events and per-step run records exist. Structured
logs with correlation IDs, support-bundle export with redaction, and a health
dashboard do not.

## Phase 18 — Installation and updates

| Item | Status | Notes |
|---|---|---|
| Linux `.deb` / AppImage / tarball | Working | AppStream validates clean |
| Windows portable zip | Working | no MSI/EXE installer |
| macOS package | Working (unsigned) | Gatekeeper right-click → Open |
| Android APK/AAB | Working | debug-signed release build |
| VS Code VSIX | Working | published |
| Auto-update, DB migration rollback, uninstall | **Missing** | data paths preserved by design; manual reinstall |

## What to build next (spec order)

1. **Phase 3 capability table** — the only hard prerequisite for Phase 4, and
   the smallest honest unit of work: provider-reported metadata only, no
   invented capabilities.
2. **Phase 4 Thinking + OpenAI Platform** — the largest untouched block in the
   specification and the reason this audit exists.
3. ~~**Phase 10 policy store**~~ — shipped in 1.31.0; it gates agent steps,
   router installs and MCP calls. It unblocks MCP (Phase 7) and makes the existing
   per-run flags uniform.
4. **Phase 2 configuration editor** + dependency probing — closes a phase that
   is already 80 % delivered.
