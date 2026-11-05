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
| Per-agent / per-project / per-task model scope | Partial | global model + per-message override; runs snapshot the model they used |
| Fallback / retry / circuit breaker | Partial | router-side `fallback_attempts` is recorded; no client-side policy |
| Provider health monitoring | Missing | on-demand `/api/test` only |

## Phase 4 — Thinking and OpenAI Platform

The backend shipped in 1.29.0 (`thinking.rs`, `openai.rs`); the composer control
is the remaining half, tracked below rather than counted as done.

| Item | Status |
|---|---|
| Native OpenAI Platform provider (Responses API) | Working (1.29.0) — `openai.rs`, own key, `openai:` model ids, buffered + streaming + agent calls |
| Thinking control (off/low/medium/high/model default) | Partial — backend levels and resolution work; the composer control lands next |
| `@Thinking` shortcut + literal escape | Working backend (1.29.0) — parsed and stripped before the message is stored; the chip lands next |
| Scope precedence (message > agent > project > global) | Working (1.29.0) — `projects.thinking` column migrated in place, agent scope from config |
| Reasoning summaries / progress surfacing | Partial — reasoning frames are emitted server-side; the panel lands next |
| Tests for any of the above | Working — 21 new cases, 204 total |
| Composer control, Settings fields, platform models in the picker | Missing (next commit) |

## Phase 5 — Agent execution engine

| Item | Status | Notes |
|---|---|---|
| Real execution loop with tools | Working | `agent.rs`: think/list/read/write/command/done/fail |
| Approval pauses, checkpoints, diffs, restore | Working | `awaiting_approval`, per-step checkpoint (200 kB cap) |
| Cancellation, crash recovery (`interrupted`) | Working | `ACTIVE` map; startup sweep |
| Resume from the step log | Working | `POST /api/runs/:id/resume` + 2 tests |
| Team (executor → reviewer → repair) | Working | `review` step with verdict/findings |
| Crew pipeline + per-stage budgets/policies | Working | `crew` roster 1–6, `stage_max_steps`, fail/skip |
| Hybrid cloud-plan / local-execute | Working | plan step, bounded escalation |
| Multi-agent parity in VS Code | Working | `agent.ts` port |
| Task queue, priority, dependencies | **Missing** | states are run states only |
| Parallel execution + worktree isolation / file locks | **Missing** | no `worktree` use; the only match is git-status parsing |
| Verification step before `succeeded` | Partial | reviewer stage is opt-in, not enforced |
| Ruflo | Not integrated (spec says optional) | Saarthi Core keeps ownership |

## Phase 6 — Skills

| Item | Status | Notes |
|---|---|---|
| Discovery across 5 roots | Working | `skills.rs` |
| Enable/disable persisted | Working | `POST /api/skills/:id/state` |
| Instruction injection with token cap | Working | rides the next message |
| Install / update / remove / version tracking | **Missing** | routes are list, get, state only |
| Skill permissions / usage history | Missing | skills are prompt text and never executed |
| Automatic skill selection | Partial | manual picker only |

## Phase 7 — MCP

**Missing entirely.** `\bmcp\b` has **zero** matches in `app/src`. No registry,
no connection lifecycle, no tool discovery, no schema validation, no permission
binding, no health or reconnect. The Phase 10 policy store this phase depends on
is also not built.

## Phase 8 — Project knowledge and memory

| Item | Status | Notes |
|---|---|---|
| Conversation history persistence | Working | `conversations`/`messages`, Android mirror |
| Workspace text search | Working | `/api/workspace/search` |
| Embeddings *probe* | Partial | `media.rs` proxies one `/api/embeddings` call; nothing is stored or indexed |
| Codebase indexing / symbols / incremental updates | **Missing** | — |
| RAG over the project | Missing | `deepsearch.rs` is web research, not codebase retrieval |
| Memory records + management UI | Missing | — |

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
| Central policy store (once/session/project/deny) | **Missing** | per-run flags + route guards only |
| OS keychain for secrets | Missing | keys live in mode-restricted `config.json` |
| Privileged-operation audit log, global kill switch | Partial | run cancel exists; no global stop, no audit trail |

## Phase 11 — Token, context and cost

| Item | Status | Notes |
|---|---|---|
| Token-budgeted packing | Working | `context.rs`, 1 k–32 k clamp, VS Code parity |
| Usage log + aggregates/series/by-model | Working | `request_log`, `/api/usage` |
| Pricing, cost estimates | **Missing** | no price table |
| Budget enforcement that stops a run | Missing | budget trims context only |
| Compression beyond head/tail cropping | Missing | attachment markers + crop |
| Context-utilisation display | Partial | not surfaced in the UI |

## Phase 12 — Live agent monitoring

Partial. `agent_runs`/`agent_steps`, SSE events and a runs panel with diffs,
approve/reject, cancel and restore are real backend state. There is **no**
cross-agent dashboard, no per-agent duration/token totals, no pause, and no
queue view — consistent with Phase 5 having no queue.

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
3. **Phase 10 policy store** — unblocks MCP (Phase 7) and makes the existing
   per-run flags uniform.
4. **Phase 2 configuration editor** + dependency probing — closes a phase that
   is already 80 % delivered.
