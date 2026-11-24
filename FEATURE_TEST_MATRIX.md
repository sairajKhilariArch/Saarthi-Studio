# Sarthi Studio — Feature Test Matrix

**Date:** 2026-11-24  
**Branch:** `audit/full-application-2026-11-24`  
**Version:** `3.88.1`  
**Legend:** **Pass** = exercised against the real binary or real persistence; **Pass (test)** = covered by the Rust/frontend/extension suite, not re-driven through the UI; **Partial** = present but a stated behavior is unverified or limited; **Missing** = no implementation found; **Blocked** = environment or external service unavailable; **Fixed** = broken at baseline, repaired and re-tested.

All real-binary cases below use `scripts/ci/test-e2e.sh`, a scratch `FREE_AI_STUDIO_HOME`, the real debug/release binary, a real SQLite database and `scripts/mock-router.py` as the provider fixture. No real user data was touched.

## A. AI chat

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Create conversation | `POST /api/conversations`; UI "New thread" | Pass | Pass | e2e 201 + UUID; live UI created `hello from the audit UI` |
| Send message / non-stream reply | `POST /api/chat` through mock router | Pass | Pass | e2e answer `Mock router reply…`; UI stream turn |
| Streaming response | `POST /api/chat/stream`; inspect SSE frames | Pass (baseline not in e2e) | Pass | e2e delta + done; reply stored exactly once |
| Stop generation | UI `AbortController`; recorder saves partial reply | Partial | Partial | code path inspected; fast mock gave no timed window to observe an abort |
| Regenerate reply | UI/API search | Missing | Missing | no regenerate route or action; "Resend" resends a user message |
| Edit previous message | UI/API search | Missing | Missing | no message `PUT` route or edit action |
| Delete individual message | UI/API search | Missing | Missing | conversation delete only |
| Delete conversation + cascade | `POST /api/conversations/:id/delete`; query messages | Pass | Pass | e2e 200, then messages 404; DB cascade |
| Conversation history | `GET /api/conversations/:id/messages` | Pass | Pass | e2e 2 then 4 messages; restart retains them |
| Persistence across restart | kill/start same home | Pass | Pass | e2e restart finds conversation and messages |
| Multiple conversations | create, list, delete second | Pass | Pass | e2e create/delete; Rust project-filtered thread list test |
| Context retention | `prepare_chat` history + SQLite | Pass (test) | Pass (test) | Rust context/history tests; e2e mock reports packed messages |
| Long conversation / token budget | context packer, per-thread budget | Pass (test) | Pass (test) | Rust tests; e2e budget update returns 5000 |
| Project memory in stream | recall marker + stream `meta.memory_used` | **Broken** (dependent on skill) | **Fixed** | new e2e checks `"memory_used"` in done frame |
| Network interruption / provider down | start run with provider down; restart | Pass | Pass | e2e failed run records provider error; server still answers |
| Error display | failure path in UI and API | Pass (test) | Pass (test) | Rust error tests; live UI no console errors |

## B. Providers and models

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Config read | `GET /api/config` | Pass | Pass | e2e `has_api_key=true`, raw key absent |
| Config update | `POST /api/config` | Pass | Pass | e2e and live UI config set |
| Provider list / add / edit / remove | `providers.rs` routes; Rust tests | Pass (test) | Pass (test) | CRUD/redaction tests, URL validation |
| API key redaction | e2e config scan | Pass | Pass | no `e2e-not-a-real-key` in response |
| Key storage | keychain/fallback path | Pass (test) | Pass (test) | Rust keychain tests; no live Secret Service assertion |
| Model discovery | mock `/v1/models` | Pass | Pass | e2e and live UI show `mock-model` |
| Model switching | composer/model scope | Pass (test) | Pass (test) | model scope tests; live UI model button present |
| Fallback providers | model chain | Pass (test) | Pass (test) | Rust chain tests |
| Authentication failure | mock/error paths | Pass (test) | Pass (test) | Rust error/status tests |
| Real provider connectivity | requires owner key/network | Blocked | Blocked | no credentials supplied; no live third-party call |
| Local model integration | Ollama-compatible path | Pass (test) | Pass (test) | Rust local-mode tests; no Ollama runtime here |
| Rate-limit handling | router error mapping | Pass (test) | Pass (test) | Rust tests |

## C. Agents

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Create/run agent | `POST /api/runs` with mock scripted provider | Pass | Pass | e2e run created |
| Approval gate | run stops before write; approve | Pass | Pass | e2e `waiting` then `succeeded` |
| File write tool | approve write; read file from disk | Pass | Pass | e2e `e2e-agent.txt` |
| Verification loop | verifier approves | Pass | Pass | e2e succeeds after verification |
| Step log | `GET /api/runs/:id` | Pass | Pass | e2e `write, verify, done` |
| Persistence across restart | kill/start | Pass | Pass | e2e run + steps survive |
| Cancellation/interruption | kill mid-run; restart | Pass | Pass | e2e interrupted state and log |
| Queue/concurrency | admission tests | Pass (test) | Pass (test) | 704-test suite |
| Multi-agent/crew | crew templates/runtime tests | Pass (test) | Pass (test) | Rust tests; no UI crew run in this pass |
| Permissions | allow/ask/deny rules | Pass (test) | Pass (test) | Rust permission/approval tests |
| Execution monitoring | `/api/events`, runs panel | Pass | Pass | EventSource 200; runs routes covered by e2e/Rust tests |

## D. Skills and extensions

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Skill discovery | `GET /api/skills` | Pass | Pass | e2e catalogue readable; live UI loaded skills |
| Install/remove/update | `skills.rs` routes | Pass (test) | Pass (test) | Rust/frontend tests |
| Git import/refresh | import form + refresh | Pass (test) | Pass (test) | frontend + Rust tests |
| Skill permissions | manifest validation | Pass (test) | Pass (test) | Rust tests |
| Invalid skill handling | validation errors | Pass (test) | Pass (test) | Rust tests |
| Extension discovery/validation | `extensions.rs` | Pass (test) | Pass (test) | Rust suite |
| Extension isolation | wasmi sandbox, fuel/memory/table caps | Pass (test) | Pass (test) | ADR + Rust tests |
| Extension install/remove | manifest-gated routes | Pass (test) | Pass (test) | Rust tests |
| Host functions read/write/data/network/terminal/MCP | sandbox tests | Pass (test) | Pass (test) | Rust tests; real local fixture for MCP |
| Panel sandbox | CSP + opaque iframe | Pass (test) | Pass (test) | code/Rust/frontend tests; no malicious panel installation |
| Real third-party extension | unavailable | Blocked | Blocked | no external package/peer |

## E. Developer environment

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Open workspace | `POST /api/workspace/open` | Pass | Pass | e2e 200 |
| List directory | workspace entries route | Pass (test) | Pass (test) | Rust tests |
| Create file | `POST /api/workspace/create` | Pass | Pass | e2e |
| Save/read file | `PUT/GET /api/workspace/file` | Pass | Pass | e2e content `first line` |
| Rename | `POST /api/workspace/rename` | Pass | Pass | e2e old 404/new 200 |
| Delete | `POST /api/workspace/delete` | Pass | Pass | e2e 200 then 404 |
| Path traversal refusal | `../outside.txt` | Pass | Pass | e2e 400, no path leaked |
| Symlink escape refusal | canonicalization tests | Pass (test) | Pass (test) | Rust tests |
| Multiple workspaces/recent folders | workspace store | Pass (test) | Pass (test) | Rust/frontend tests; HTML-injection regression added |
| Git operations | `git.rs` tests | Pass (test) | Pass (test) | extensive Rust tests; not live-driven in this pass |
| Runner/build/test | `runner.rs` tests | Pass (test) | Pass (test) | Rust tests; no real build command in e2e |
| Watch mode | `watch.rs` tests | Pass (test) | Pass (test) | Rust tests |
| Terminal open/input/output/close | real PTY, command wrote file | **Untested** | **Pass** | new e2e `terminal-proof.txt` |
| Terminal persistence | one PTY per folder | Pass (test) | Pass (test) | code + tests; not manually held across project switches |
| LSP diagnostics | `lsp.rs` | Pass (test) | Pass (test) | Rust tests; no real language server installed |
| DAP debugger | `dap.rs` | Pass (test, debugpy documented) | Pass (test) | Rust tests; no lldb/dlv |

## F. Memory

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Create memory | `POST /api/memory` | Pass | Pass | e2e record id |
| List memory | `GET /api/memory` | Pass | Pass | e2e marker present |
| Recall into chat | stream with matching message | **Broken reporting** | **Fixed** | e2e `memory_used` in stream meta |
| Update memory | same id + new fields | Pass (test) | Pass (test) | Rust tests |
| Delete memory | `POST /api/memory/:id/delete` | Pass | Pass | e2e list no longer contains it |
| Project isolation | `project_id` filters | Pass (test) | Pass (test) | Rust tests |
| Persistent storage | SQLite | Pass | Pass | e2e restart/DB |
| Permissions | memory routes behind local guard | Pass | Pass | global guard + e2e foreign Host |

## G. Workflows

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Create/save workflow | `POST /api/workflows` | Pass | Pass | e2e id |
| Export document | `GET /api/workflows/:id/export` | Pass | Pass | e2e `sarthi.workflow` |
| Import | `workflow.rs` tests | Pass (test) | Pass (test) | Rust validation/import tests |
| Delete | `POST /api/workflows/:id/delete` | Pass | Pass | e2e removed from list |
| Run graph / conditions / retries | workflow engine tests | Pass (test) | Pass (test) | Rust tests; not live-run against provider in this pass |
| Sequential/parallel dependencies | graph scheduler tests | Pass (test) | Pass (test) | Rust tests |
| Cancellation/child runs | workflow tests | Pass (test) | Pass (test) | Rust tests |
| Persistence/history | workflow_runs/steps | Pass (test) | Pass (test) | Rust tests |
| Scheduling | 30-second tick, SQLite claims | Pass (test) | Pass (test) | Rust tests |

## H. Application management

| Feature | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Settings load/navigation | live UI click | Pass | Pass | Settings panel reached, no console errors |
| Config redaction | e2e | Pass | Pass | raw key absent |
| Themes | live theme button | Pass | Pass | `data-theme=dark` after click |
| Keyboard shortcuts | frontend suite | Pass (test) | Pass (test) | 115 frontend tests |
| Notifications/events | EventSource + UI | Pass | Pass | `/api/events` 200, no errors |
| Update check/stage/prepare/swap/rollback | installer/update Rust tests | Pass (test) | Pass (test) | checksum tests; no network release |
| Import/export | workflow export + skill import | Pass | Pass / Pass (test) | e2e + Rust/frontend tests |
| Logging/diagnostics | process log after turns | Pass | Pass | e2e log entries |
| Error reporting | structured JSON errors | Pass | Pass | e2e error cases |
| Backup/recovery | schema backup test | Pass (test) | Pass (test) | Rust `an_existing_database_is_copied…` |
| Uninstall | Rust tests | Pass (test) | Pass (test) | no destructive live run |
| Live browser layout | load/theme/nav/chat only | Partial | Partial | no pixel/responsive assertion |

## I. Security and platform

| Area | Test performed | Initial status | Final status | Evidence |
|---|---|---|---|---|
| Foreign `Host` on core routes | live before/after; e2e | **Broken (P1)** | **Fixed** | before 200 config, after 403; e2e |
| Foreign `Origin` | e2e capabilities | Pass | Pass | 403 |
| Workspace/terminal/agent guards | code + e2e traversal | Pass | Pass | 403 / 400 |
| Host API bearer | Rust tests | Pass (test) | Pass (test) | constant-time hash verifier |
| Host API TLS/bind policy | Rust tests | Pass (test) | Pass (test) | rejects 0.0.0.0/public |
| CSP/headers | live response | Pass | Pass | CSP, XFO DENY, nosniff, no-store |
| Secret redaction | config/provider tests | Pass | Pass | no raw key |
| Update checksum | Rust tests | Pass (test) | Pass (test) | tamper refused |
| Archive traversal | installer tests | Pass (test) | Pass (test) | zip/tar guards |
| Dependency audit | npm prod/full | Pass / partial | Partial | 0 prod; 3 moderate dev-only VSCode advisories |
| Rust advisory scan | `cargo-audit` | Blocked | Blocked | tool not installed |
| Windows | CI/source only | Blocked | Blocked | no host |
| macOS | CI/source only | Blocked | Blocked | no host |
| Android | no SDK/device | Blocked | Blocked | no JDK/SDK/Gradle |
| Website | build | Pass | Pass | 37 pages |

## Command evidence

```text
cargo test --locked (with lld):        704 passed, 0 failed, 1 ignored
node --test frontend:                  115 passed
bash scripts/ci/test-e2e.sh (debug):    51 passed, 0 failed
bash scripts/ci/test-e2e.sh (release):  51 passed, 0 failed
vscode-extension npm test:              45 passed
website npm run build:                  37 pages
test-version.sh:                        26 passed
test-linux-metadata.sh:                 9 passed
test-release.sh:                        27 passed
test-artifacts.sh:                      12 passed
test-bundle.sh:                         13 passed
test-installer.sh:                      0 passed, 1 skipped (no WiX)
```

## Not tested, explicitly

- A real provider, real API key, real search key, real speech service, real third-party MCP server, real language server or real debug adapter.
- A signed/installed Windows, macOS or Android build; a tagged 11-asset GitHub release; update from a previous published version.
- A browser layout, responsive, accessibility or cross-browser audit; a timed stop-generation abort; long-session memory/leak soak.
