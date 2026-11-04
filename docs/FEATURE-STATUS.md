# Feature status report (audit of `main` @ 1.26.3)

Status vocabulary: **Working** (route + persistence + UI verified in code or
by run), **Partial** (exists but a stated requirement is unmet), **Broken**
(exists and fails), **Missing** (no implementation).

Evidence notes: `cargo test --locked` = 127 passed at 1.26.2/1.26.3 sources;
`desktop-ci`, `rust-host-api-ci`, `android-ci` green on the 1.26.3 push;
release run `41861357127` published 8 non-macOS assets.

## Phase 1 — CI/CD and release reliability

| Item | Status | Evidence / gap |
|---|---|---|
| Rust compiles, tests, clippy gate | Working | `desktop-ci` job green, `-D warnings` |
| Version extraction across OSes | **Broken → fixed in 1.26.4** | CRLF wan. CR leaked into artifact names (run `41693462775`) |
| Artifact name validation before upload | **Missing → added in 1.26.4** | no validation step existed |
| Checksum verification of uploads | Partial → verified in 1.26.4 | `SHA256SUMS.txt` produced, never checked by CI |
| AppStream metadata for AppImage | **Missing → added in 1.26.4** | `app/packaging/` had only `.desktop` + icons |
| macOS release job | **Broken (environment)** | `queued` >30 min in `41861357127`; `cancelled` in earlier runs; no macOS asset in any release |
| Release tags v1.26.0/1.26.1/v1.26.2 | Broken (historical) | no assets published; v1.26.2 never triggered a run (invalid YAML) |
| Regression test for version parsing | **Missing → added in 1.26.4** | `scripts/ci/test-version.sh` |
| Duplicate release runs | Missing → `concurrency` added | two tag runs could race the same release asset name |

## Phase 2 — Router manager

| Item | Status | Notes |
|---|---|---|
| Detect/start/stop FreeLLMAPI compose project | Working | `router.rs`, `/api/router*`, ownership file, stop-only-if-owned |
| OmniRoute router | Partial | `main.rs` `/api/omniroute*`; same compose mechanism, no registry abstraction |
| Router registry (multiple routers, install methods, min versions) | Working | `router_registry.rs` + `/api/routers`; only routers with a real lifecycle in the crate are listed |
| OS/arch detection, dependency checks, package-manager awareness | Partial | registry gates OS/arch and lists methods per platform; no dependency probing or package-manager detection |
| Install flow with approval, download, checksum, configure | Missing | installer script is out-of-app |
| Health check distinguishing "process alive" from "healthy" | Working | `healthy` means "the health probe answered"; a container that is up but silent is `running: true, healthy: false` (unit-tested) |
| Auto-start on launch | Partial | start on demand + CLI; no startup sweep with per-router preferences |
| Router UI (list, logs, update, uninstall, connection tester) | Partial | list, health, start/stop, restart and the connection tester; logs, update, uninstall and a config editor still missing |

## Phase 3 — Model and provider manager

| Item | Status | Notes |
|---|---|---|
| Provider presets and model discovery | Working | presets in `app.js`/`index.html` (38 refs), `/models` with 1-minute cache keyed by base+key |
| Local (Ollama) models | Working | `local.rs`: `/api/tags`, `/api/version`, hardware, pull with progress |
| Capability metadata (context, modalities, tools, reasoning) | Missing | no per-model capability table; capabilities are inferred nowhere |
| Per-agent / per-project / per-task model selection | Partial | global model + per-message override; agent uses the active model |
| Fallback, circuit breaker, retry/backoff policies | Partial | router-side fallback reported back as `fallback_attempts`; no client-side policy |
| Provider health monitoring | Missing | only on-demand `/api/test` |

## Phase 4 — Thinking and OpenAI Platform

| Item | Status | Notes |
|---|---|---|
| Native OpenAI Responses API provider | **Missing** | 0 references to `/v1/responses`, `api.openai.com`; OpenAI only via compatible base URL |
| Reasoning/Thinking control (`off/low/medium/high`, model default) | **Missing** | 8 incidental `thinking|reasoning` matches (prose, CSS), no request parameter |
| `@Thinking` composer shortcut | **Missing** | no parser, no escape path |
| Reasoning summaries / progress surfacing | Missing | — |
| Scoped precedence (message > agent > project > global) | Missing | no scope model exists |

## Phase 5 — Agent execution engine

| Item | Status | Notes |
|---|---|---|
| Real task execution loop with tools | Working | `agent.rs`: read/write/command/list/think, 12 default / 30 hard steps, 90 s command cap |
| Approval + checkpoint + rollback | Working | `awaiting_approval`, per-step checkpoints (200 kB cap), unified diffs |
| Crash recovery (`interrupted`) | Working | terminal statuses persisted; `markInterrupted()` on startup (also in VS Code `RunStore`) |
| Cancellation | Working | `ACTIVE` flag map, cancel route |
| Task lifecycle beyond run states (Queued/Planning/Verifying) | Partial | run states only; no queue, priority, dependencies |
| Parallel/multi-agent execution, worktrees, file locks | **Missing** | single run at a time per conversation; no orchestration layer |
| Specialised agent roles (planner/researcher/reviewer…) | Missing | two prompt modes only (`plan` mode exists) |
| Ruflo integration | Missing | not referenced |

## Phase 6 — Skills

| Item | Status | Notes |
|---|---|---|
| Discovery from bundled/project/user/`~/.agents` roots | Working | `skills.rs`, 5 roots in precedence order |
| Enable/disable with persistence | Working | `/api/skills/:id/state` → `config.json` |
| Instruction injection with token cap | Working | picked skill rides the next message |
| Install from git / local dir, update, remove, versioning | **Missing** | discovery only |
| Permission/model constraints per skill | Missing | skills are prompt text, never executed (by design) |

## Phase 7 — MCP

**Missing entirely.** `\bmcp\b` has zero matches in the repository. No server
registry, no tool discovery, no schema validation, no permission binding.

## Phase 8 — Project knowledge and memory

| Item | Status | Notes |
|---|---|---|
| Conversation history persistence | Working | `conversations`/`messages` + Android mirror |
| Workspace file search | Working | `/api/workspace/search` (`search_tree`/`search_workspace`) |
| Codebase indexing / symbols / incremental index | **Missing** | — |
| Semantic retrieval (RAG) over project | Missing | `deepsearch.rs` is web research, not codebase retrieval |
| Memory management UI (inspect/edit stored memory) | Missing | — |

## Phase 9 — VS Code-like developer environment

| Item | Status | Notes |
|---|---|---|
| File tree, tabs, editor with save | Working | `workspace.rs` + UI (`ide.js` shell) |
| Real PTY terminal | Working | `portable-pty`, SSE, resize |
| Search (workspace) | Working | `/api/workspace/search` |
| Git status display | Partial | `vcs_status`/`parse_git_status` shows status only — no stage/commit/diff/branch ops |
| Syntax highlighting, diagnostics, completion, formatting | **Missing** | no editor engine (deliberate: no Monaco/CodeMirror) |
| Unsaved-change guards, diff viewer for pending writes | Partial | agent pending-write diff exists; editor-level diff absent |
| Build/test runner, debugger, problems panel | Missing | agent can run commands; no runner UI |

## Phase 10 — Safety and permissions

| Item | Status | Notes |
|---|---|---|
| Path confinement, symlink escape prevention | Working | canonicalised, `MAX_FILE_BYTES` 1 MB, body cap 2 MB |
| Cross-site / DNS-rebinding write guard | Working | loopback `Host` + `Origin` checks on mutating routes |
| Outbound secret detection | Working | `privacy.rs` blocks credential-shaped attachments |
| Host API auth + transport guardrails | Working | token store, body caps, separate listener |
| Central permission engine (tools/network/git/secrets, per-scope grants) | Partial | per-run flags + endpoint guards, no unified policy store or audit log |
| Secrets at rest via OS keychain | Missing | config file holds keys (mode-restricted, not keychain) |
| Emergency stop / privileged-operation audit | Partial | run cancel exists; no global kill switch or audit trail |

## Phase 11 — Token, context, cost

| Item | Status | Notes |
|---|---|---|
| Token-budgeted context packing | Working | `context.rs` (1 k–32 k clamp), VS Code parity in `lib.ts` |
| Usage log with tokens/latency/status | Working | `request_log`, `/api/usage` with aggregates, series, by-model |
| Pricing, cost estimates, budget enforcement | Missing | no price table; budget trims context but never stops a run |
| Context compression beyond cropping | Missing | head/tail crop + attachment markers only |

## Phase 12 — Live monitoring

Partial. Agent run state, steps, diffs and SSE change events exist
(`agent.rs`, `events.rs`) and the UI renders a runs panel; there is no
cross-agent dashboard, no duration/token totals per agent, no queue view.

## Phase 13 — Workflow builder

**Missing** on desktop (no workflow routes/tables). The VS Code store keeps a
flat `{id,name,prompt,skill}` macro list — that is a saved prompt, not a node
graph, and it has no scheduler.

## Phase 14 — Cross-platform integration

| Item | Status | Notes |
|---|---|---|
| Desktop Linux/Windows/macOS build | Working (macOS packaging broken) | Windows + Linux artifacts published; macOS never attached |
| Android client | Working (subset) | chat + history + backup; remote-only agent, no router management |
| VS Code extension | Working (subset) | embedded Node server + shared UI; agent port exists |
| Capability negotiation between clients | Partial | Host API `/capabilities` exists; clients do not gate features on it |
| TLS/pairing/session for remote | Working | host token + TLS transport module |

## Phase 15 — Extension ecosystem

Missing beyond skills: no plugin manifest, no custom tools, no provider
adapters, no UI extensions.

## Phase 16 — Testing and quality

| Layer | Status | Notes |
|---|---|---|
| Rust unit/integration | Working | 133 `#[test]`/`#[tokio::test]` functions |
| VS Code extension | Working | `node --test` suite in `vscode-extension/test` |
| Android | Working | JVM tests + emulator instrumentation + validation gate |
| Frontend | **Missing** | no test runner (no build step, no bundler) |
| End-to-end (launch → chat → agent → restart) | Missing | only ad-hoc local smokes |
| Failure injection (offline, rate limit, disk full, crash) | Missing | — |
| Release-pipeline regression tests | **Added in 1.26.4** | `scripts/ci/test-version.sh` |

## Phase 17 — Observability

Partial. `request_log` + SSE events + per-step agent records; no structured
logging, no correlation IDs, no log export/support bundle, no health dashboard.

## Phase 18 — Installation and updates

| Item | Status | Notes |
|---|---|---|
| Linux `.deb` | Working | headless build on 22.04, GUI-dependency assertion |
| Linux AppImage / tarball | Working | AppStream metadata added in 1.26.4 |
| Windows portable zip | Working | published |
| Windows installer (MSI/EXE) | **Missing** | zip only |
| macOS package | Broken | job never ran; unsigned tarball only by design |
| Android APK/AAB | Working | debug-signed release build |
| VS Code VSIX | Working | published |
| Auto-update, migration, rollback | Missing | manual reinstall; data paths preserved by design |
| `install.sh` (Linux, source build) | Working | Ubuntu/apt oriented |

## Cross-cutting: previously diagnosed errors

| Issue | State |
|---|---|
| A. `agent.rs` SQL syntax error | Resolved — compiles, 133 tests, clippy `-D warnings` green; `SELECT status FROM agent_runs` queries are correct |
| B. CR in Linux version → bad artifact name | Root cause CRLF rewrite; **fixed at the source** (LF manifests + `scripts/ci/version.sh` strips whitespace/control chars) |
| C. Over-escaped macOS `sed` | Fixed; macOS now uses the same helper as Linux |
| D. Artifact upload failure | Fixed for 8 assets; now guarded by `verify-artifacts.sh` before upload |
| E. AppImage AppStream warning | Fixed in 1.26.6: `appstreamcli validate-tree` (fatal for appimagetool, fails on warnings) needs an RDNS component id, matching file names and an OARS rating — `io.github.sairajkhilariarch.SaarthiStudio.{desktop,appdata.xml}` |
| F. macOS cancellation/queue | **Unresolved**: no cancellation cause found in API data; the 1.26.3 job was never scheduled (queued ~35 min). Needs a runner-availability decision, not a code patch |
