# Sarthi Studio — Integration Test Report

**Date:** 2026-11-25  
**Branch:** `remediation/security-2026-11-24`  
**Candidate:** `3.88.4`  
**Baseline reviewed:** `23384e9` (3.88.3)  
**Host:** Linux x86_64, local desktop and headless binaries; no Windows/macOS/Android host, signing key, real provider key or real MCP peer.

## Scope and method

Phase 4 exercised the boundaries between existing modules rather than re-testing each module in isolation. The work followed the rule: inspect the integration path, reproduce the break against a real binary or database, fix the cause, add regression coverage, and rerun the affected suites.

The primary evidence is `scripts/ci/test-e2e.sh`: a real debug binary, a scratch `FREE_AI_STUDIO_HOME`, a real SQLite database, a real PTY, and `scripts/mock-router.py` as the provider fixture. Rust tests cover the internal contracts that the HTTP surface cannot reach (workflow scheduling, queue admission, permission gates, migrations). No real user data was touched.

## Integration defects found and fixed

| ID | Path | Defect | Fix | Regression evidence |
|---|---|---|---|---|
| INT-01 | A: Chat → Provider → Model | Non-stream `/api/chat` saved the reply but never wrote `conversations.model`; `/api/conversations` also omitted the column from both list queries. The UI's `openConversation` reads `match.model`, so model restore was dead after every non-stream turn. | All three non-stream save paths (deep search, local, router) now persist the answering model with `updated_at`; both conversation-list queries select `COALESCE(v.model,'')` and the JSON carries `model`. | Extended Rust list test; new e2e check `the conversation list returns the model the turn used`. |
| INT-02 | C/E: Workflow → Agent → Skills | A workflow stored and displayed `skill`, and the editor let users attach one, but execution never resolved or applied it. The attachment was silently ignored. | The workflow resolves its skill before a run row exists and again on resume/schedule. The instructions are placed in front of every agent node's task and stored in the child run's goal, so a resumed run keeps them. A missing, disabled, broken or incompatible skill refuses the run with a specific error. | 4 Rust checks in `skills`/`workflow`; e2e two-step workflow proves the skill reaches both child agent goals. |
| INT-03 | E: Workflow → Agents | Child agent runs inserted by the workflow left `token_budget` at the column default `0`, bypassing the configured run ceiling that manually started runs obey. | Workflow child rows now inherit `load_config.run_token_budget`, matching `create_run`, and are inserted through a testable helper. | Rust test `a_workflow_child_run_inherits_the_configured_token_ceiling`. |
| INT-04 | C: Agent → Skills | Direct `/api/runs` had no skill field at all, while chat and workflows supported skills. The Runs form could not attach one. | `POST /api/runs` accepts an optional `skill`, validates it with the same resolver, and stores the instructions at the front of the run goal. The Runs form has a populated skill picker. | Rust validation tests; e2e run succeeds and its stored goal contains the skill; missing-skill run returns 400. |

No hardcoded success response, permission bypass, swallowed error or new dependency was introduced. Skill text remains prompt guidance; tool access is still decided by the existing run permission gate.

## Path-by-path status

### A. Chat → Provider → Model

| Check | Result | Evidence |
|---|---|---|
| Selected model answers and is stored on the conversation | **Pass** | e2e non-stream turn, list returns `mock-model`; Rust list test asserts the column mapping |
| Streaming reply, delta/done frames, one stored reply | **Pass** | Existing e2e real-SSE and JSON-fallback checks |
| Stop generation safely | **Pass** | Existing e2e aborted stream stores one partial reply; next turn answers |
| Provider error is visible | **Pass** | Existing e2e provider-down run records the error; server stays up |
| Provider switching does not corrupt the thread | **Pass (test)** | Model scope resolution tests; the model column is a display/restore hint, not a history rewrite |
| Real third-party provider | **Blocked** | No owner key/network fixture supplied; mock router used |

### B. Chat → Agent → Tools

| Check | Result | Evidence |
|---|---|---|
| Agent receives the goal and step budget | **Pass** | e2e run creation and step log |
| Tool discovery and read/write execution | **Pass** | e2e workspace file write after approval |
| Write permission gate | **Pass** | e2e pauses at `awaiting_approval`, then writes after approval |
| Denied permission blocks the action | **Pass** | New e2e: reject approval → run `failed`, `e2e-denied.txt` absent |
| Failed tool does not crash the agent | **Pass (test)** | Existing gate/error tests; provider-down run fails cleanly |
| MCP tool access is opt-in and refused when unavailable | **Pass (test)** | Existing MCP permission tests |

### C. Agent → Skills

| Check | Result | Evidence |
|---|---|---|
| Skill discovery | **Pass** | e2e `/api/skills`; live catalogue |
| Attached skill resolved safely and bounded | **Pass** | `skills::resolve_attached` / `with_instructions`; 900-token body crop |
| Missing skill produces a useful error | **Pass** | e2e direct run and workflow run both return 400 naming the missing skill |
| Disabled/broken/incompatible skill refused | **Pass (test)** | `a_workflow_skill_is_checked_before_an_agent_sees_it` |
| Skill instructions reach the agent | **Pass** | e2e direct and workflow child goals contain the skill header and body |
| Whether a model obeys the instructions | **Not asserted** | Provider behavior; the mock proves delivery, not compliance |
| Skill permissions grant tools | **Not applicable** | Skills are prompt text; run permissions remain authoritative |

### D. Agent → Memory

| Check | Result | Evidence |
|---|---|---|
| Recall injected into a run | **Pass (test)** | `run_background` uses `memory::recall_quiet`, `prompt_block`, `index::retrieve`; existing tests |
| Memory writes persist | **Pass** | Existing e2e create/list/delete and restart checks |
| Project scoping | **Pass (test)** | Existing project-filter tests |
| Private memory leakage | **Pass (test)** | Project filters and redaction tests; no cross-project assertion was added this phase |

### E. Workflow → Agents → Tools

| Check | Result | Evidence |
|---|---|---|
| Multiple agents with a dependency | **Pass** | New e2e two-node workflow (`debug-it` → `report-it`) ends with 2/2 succeeded steps and 2 child run ids |
| Skill applies to every agent node | **Pass** | e2e asserts both steps ran; first child goal carries `debug` |
| Run state and step history persist | **Pass** | Existing workflow/run persistence tests; e2e reads the run detail |
| Cancellation stops child agents | **Pass (test)** | Existing Rust `stop_child_agent_runs` coverage |
| Retry policy | **Pass (test)** | Existing workflow retry tests |
| Scheduled start | **Pass (test)** | SQLite window-claim tests; scheduler now refuses a missing skill before claiming |
| Failure branches | **Pass (test)** | Existing condition/skip/failure tests |

### F. Desktop → Backend

| Check | Result | Evidence |
|---|---|---|
| HTTP/IPC routes answer on loopback | **Pass** | e2e readiness and route checks |
| Foreign `Host`/`Origin` rejected | **Pass** | Existing e2e 403 checks |
| Invalid requests rejected | **Pass** | e2e invalid skill, invalid workspace path, invalid approvals |
| Shutdown/restart cleanup | **Pass** | e2e kill/restart leaves an interrupted run and readable log |
| Errors are not swallowed | **Pass** | Structured JSON errors and process log checks |

### G. Extensions → Application

| Check | Result | Evidence |
|---|---|---|
| Discovery, install, remove | **Pass (test)** | Existing extension route tests |
| Sandbox and host-function permissions | **Pass (test)** | wasmi fuel/memory/table caps and host tests |
| Incompatible extension fails safely | **Pass (test)** | Manifest validation tests |
| Cannot bypass application security | **Pass (test)** | Sandbox/host gates; no real third-party package installed |

## Required test categories

| Category | Coverage |
|---|---|
| Successful execution | Chat turn, direct agent run, two-agent workflow, approved write |
| Invalid input | Unknown skill, unknown workflow/run ids, path traversal, invalid graph/profile |
| Missing dependencies | Provider down, missing skill, no workspace, no enabled MCP server |
| Permission denial | New e2e rejection test; run fails and file is absent |
| Network/provider failure | Existing e2e provider-outage and failed-run checks |
| Tool failure | Existing gate/error tests and failed-step paths |
| Cancellation | Existing stream abort, workflow cancel/child-stop tests, kill/restart |
| Retry | Existing workflow retry tests and queue re-admission tests |
| Restart | Existing e2e conversation/settings/run survival |
| Persistence and recovery | Existing restart, corrupt-database refusal, backup-before-migration tests |

## End-to-end scenarios

| # | Scenario | Result | Evidence |
|---|---|---|---|
| 1 | Message → provider → conversation saved | **Pass** | e2e 2 messages, model returned by list, survives restart |
| 2 | Task → authorized tool → result in chat | **Pass** | e2e approval → write → verify → done; file on disk |
| 3 | Skill invoked → loaded → agent follows it | **Pass for delivery** | Direct and workflow runs store the skill instructions; mock verifies they arrived, not that a model obeyed |
| 4 | Workflow → multiple dependent agents → final result | **Pass** | New two-node dependency run, both child runs succeeded |
| 5 | Restart → conversations/settings/data remain | **Pass** | Existing e2e restart checks |
| 6 | Cancel → safe stop and resource release | **Pass for covered paths** | Stream abort and kill/restart; workflow child-stop is Rust-tested, not a live two-agent cancel |
| 7 | Deny permission → operation blocked | **Pass** | New e2e rejection: run failed, target file never created |

## Commands and results

```text
cargo test (with lld)                         712 passed, 0 failed, 1 ignored (main suite)
cargo test --test cli                           3 passed, 0 failed
cargo fmt --check                               pass
cargo clippy --all-targets -- -D warnings       pass
scripts/ci/test-e2e.sh (debug, 3.88.4)         75 passed, 0 failed
scripts/ci/test-e2e.sh (release, 3.88.4)       not run: environment refused the command twice before execution
cargo build --release                           pass; `Sarthi Studio 3.88.4`
scripts/ci/test-frontend.sh                     115 passed, 0 failed
vscode-extension npm test                       45 passed, 0 failed
website npm run build                           37 pages
scripts/ci/test-version.sh                      26 passed, 0 failed
scripts/ci/test-linux-metadata.sh               9 passed, 0 failed
scripts/ci/test-release.sh                      27 passed, 0 failed
scripts/ci/test-artifacts.sh                    12 passed, 0 failed
scripts/ci/test-bundle.sh                       13 passed, 0 failed
scripts/ci/test-installer.sh                    0 passed, 1 skipped (no WiX)
cargo audit                                     exit 0; 5 allowed unmaintained/unsound warnings
```

The debug e2e suite grew from 61 to 75 checks. New checks cover: conversation model round-trip, direct agent skill creation, stored direct-run skill, missing-skill refusal, denied write, workflow skill save, workflow run start, both dependent steps, skill delivery to the child goal, and missing workflow skill refusal.

## Remaining incompatibilities and limitations

- **Release-mode e2e was not run this phase.** The command was refused by the execution environment before it started, twice; per instruction it was not retried again. The 3.88.4 release binary builds and reports its version, and the same script passes 75/75 in debug.
- **No Windows, macOS or Android execution host.** Platform-specific packaging and runtime behavior remain unverified here.
- **No signing/notarization or tagged release.** The release pipeline was not run.
- **No real provider, search, speech, MCP peer or language server.** The mock router and local fixtures are the only external stand-ins.
- **Skill compliance is not asserted.** The tests prove the skill text is resolved, bounded and stored with the agent goal. They cannot prove a third-party model follows it.
- **Workflow cancellation with two live agents is not an e2e case.** It is covered by Rust tests that stop child runs.
- **Workflows remain global.** Their child runs use the active project at execution time; this phase did not add a workflow-level project binding.
- **The assembled `.deb` still has no resolved dependency metadata on this host.**

## Changed files

```text
app/Cargo.toml
app/Cargo.lock
app/packaging/io.github.sairajkhilariarch.SarthiStudio.appdata.xml
app/src/main.rs
app/src/skills.rs
app/src/workflow.rs
app/src/agent.rs
app/static/app.js
app/static/index.html
scripts/ci/test-e2e.sh
scripts/mock-router.py
FEATURE_TEST_MATRIX.md
BUG_FIX_LOG.md
RELEASE_READINESS_REPORT.md
INTEGRATION_TEST_REPORT.md
SYSTEM_ARCHITECTURE_STATUS.md
```

No `main` commit, merge, branch deletion, push or deployment was performed.
