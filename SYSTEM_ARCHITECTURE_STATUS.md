# Sarthi Studio — System Architecture Status

**Date:** 2026-11-25  
**Branch:** `remediation/security-2026-11-24`  
**Version:** `3.88.4`  
**Scope:** Cross-module integration of the existing architecture. No module was rewritten and no dependency was added.

## Runtime shape

Sarthi Studio is one Rust binary with two entry modes (desktop window and headless server) serving a hand-written static UI. SQLite is the system of record; JSON files hold configuration, workflow definitions and installed-skill metadata. The frontend has no build step.

```text
Browser / desktop webview / VS Code host
                │  HTTP + SSE (loopback, same-origin guard)
                ▼
        Axum routes in app/src/main.rs
   ┌──────────┬──────────┬───────────┬──────────────┐
   │ Chat     │ Agents   │ Workflows │ Skills / MCP │
   │ prepare  │ queue +  │ graph +   │ discovery +  │
   │ + stream │ steps    │ scheduler │ install      │
   └────┬─────┴────┬─────┴─────┬─────┴──────┬───────┘
        │          │           │            │
        ▼          ▼           ▼            ▼
   Provider     Tool gate   Child agent   Prompt text
   router /     (permissions, runs via the  (bounded,
   local /      approvals)   same agent     validated)
   platform                   engine
        │          │           │
        └──────────┴───────────┴──► SQLite (conversations, messages,
                                     projects, agent_runs/steps,
                                     workflow_runs/steps, request_log,
                                     memory, permissions, …)
```

### Module responsibilities and interfaces

| Module | Responsibility | Interface used by other modules |
|---|---|---|
| `main.rs` | HTTP routes, chat preparation, context packing, persistence of replies and settings, static assets | `/api/chat`, `/api/chat/stream`, `/api/conversations`, `/api/config` |
| `router.rs` / `openai.rs` / `local.rs` | Provider selection, fallback chain, streaming, usage/cost | `call_router_with`, `open_stream`, local chat |
| `agent.rs` | Queue admission, permission gate, step loop, verification, cancellation, run history | `/api/runs`, `create_run`, `start_queued`, `stop_run`, `dispatch` |
| `workflow.rs` | Graph validation, dependency scheduling, approvals, retries, run history, child agent creation | `/api/workflows`, `/api/workflow-runs`, `begin_run`, `spawn` |
| `skills.rs` | Skill discovery, manifest/version validation, instruction bounding, install/update/remove | `/api/skills`, `select_attached`, `resolve_attached`, `with_instructions` |
| `memory.rs` / `index.rs` | Project/global recall and code retrieval for chat and agent backgrounds | `recall_quiet`, `prompt_block`, `retrieve` |
| `mcp.rs` | Opt-in MCP server lifecycle and tool catalogue | Agent run `allowMcp`, tool discovery |
| `extensions.rs` | wasmi sandbox, manifest validation, host functions | Extension routes and host calls |
| `permissions.rs` | Saved allow/ask/deny policy plus per-run capability gate | Agent tool execution |
| `workspace.rs` / `terminal.rs` / `git.rs` | Root-scoped file access and PTYs | Agent tools, workspace routes |
| `installer.rs` / `update.rs` | Verified staged updates and packaging | Desktop startup |

The dependency direction is one-way: route modules call domain modules; domain modules do not import the route layer. Shared types (`AppState`, skill catalogue, run rows) live in the owning module.

## Integration paths after Phase 4

| Path | Status | Notes |
|---|---|---|
| Chat → provider → model | **Connected** | The answering model is now written on stream and non-stream paths and returned by the thread list. Streaming, stop, fallback and error mapping were already present. |
| Chat → agent → tools | **Connected** | The queue freezes permissions per run; the gate denies, asks or allows; denied writes fail the run visibly. Tool output is appended to the step log. |
| Agent → skills | **Connected** | Direct runs accept an optional skill; it is validated before a row exists and stored at the front of the goal. Skill text is guidance, not a tool grant. |
| Agent → memory | **Connected (existing)** | `run_background` injects recall and retrieval; chat uses the same functions. Project filters remain the isolation boundary. |
| Workflow → agents → tools | **Connected** | Every agent node creates a normal `agent_runs` row through the shared queue, so permissions, retries, cancellation and token ceilings apply. Workflow skills reach every agent node. |
| Desktop → backend | **Connected** | Loopback HTTP/SSE with Host/Origin checks; invalid requests return structured errors; restart marks interrupted runs rather than replaying them. |
| Extensions → application | **Connected (existing)** | Discovery/install/remove are manifest-gated; execution is sandboxed; host capabilities are explicit. |

## Data consistency

| Store | Invariant | Status |
|---|---|---|
| `conversations` | `model` records the model that answered so reopening restores it | **Fixed:** all non-stream paths write it; list returns it |
| `messages` | One assistant row per completed reply; partial stream saved once | Existing tests and e2e |
| `agent_runs` | Created `queued`; admitted by one dispatcher; permissions/budget frozen at creation | Existing tests; workflow children now inherit the configured budget |
| `agent_runs` ↔ `workflow_steps` | A child run id is recorded on its step; cancellation finds and stops live children | Existing tests; e2e reads both child ids |
| `workflow_runs` / `workflow_steps` | State transitions are terminal-safe; a resume reopens only unfinished steps | Existing tests |
| `workflow_schedule_claims` | One due window starts one run across processes | Existing SQLite claim tests; a missing skill no longer burns a window |
| `request_log` | Provider failures and successes are recorded with model/status | Existing tests and provider-outage e2e |
| `memory` | Project/global scope filters reads and writes | Existing tests |
| Skills | Catalogue is discovered from disk; disabled/broken/incompatible skills are refused | New shared resolver and tests |

Migrations are additive and idempotent. Phase 4 added no schema migration: the existing `conversations.model` and `agent_runs.token_budget` columns were already present, so the fixes close behavior gaps rather than changing storage shape.

## Security implications of the Phase 4 changes

- **No new capability.** A skill is prompt instructions. Attaching one does not enable writes, commands, MCP calls or network access; those remain gated by the run's frozen permissions.
- **Skill input is bounded.** `instructions_for` crops the body to 900 tokens; `with_instructions` trims the body again to the caller's character ceiling and keeps the user's task intact at the end.
- **Disabled and incompatible skills are refused.** The resolver checks load errors, minimum Studio version and the user's disabled list before any run is created. This prevents a stale attachment from silently executing with fewer constraints than the user configured.
- **Conversation model is display metadata, not authority.** The stored model restores the composer choice; it does not bypass provider validation or fallback rules.
- **Workflow child budget now matches manual runs.** A scheduled graph can no longer ignore the configured token ceiling through the column default.
- **Error messages name the skill or the missing permission but do not include secrets.** Provider keys remain redacted by the existing config layer.

## Remaining architectural gaps and risks

1. **Skills are guidance, not enforceable policy.** A model may ignore instructions. Any behavior that must be guaranteed belongs in the permission gate, not a skill.
2. **Workflows are global.** A child run uses the active project when it executes, so switching projects during a long workflow can split its runs across project scopes. A workflow-level project binding would be a schema/product change and was not added.
3. **Workflow cancellation with multiple live agents is Rust-tested, not e2e-tested.** The stop path cancels child runs through the same route as a manual cancel.
4. **Direct-run skill text is stored in the run goal.** It is visible in the run detail and survives restart. A separate skill column/system message would be cleaner but needs a schema change; the goal is the existing durable channel.
5. **Release-mode e2e did not run this phase.** The environment refused the command before execution. The debug suite passes 75/75 and the release binary builds.
6. **Platform and external services remain unverified.** No Windows/macOS/Android host, code signing, tagged release, real provider/search/speech service, or real third-party MCP/extension.
7. **The `.deb` dependency metadata limitation is unchanged** (no `dpkg-shlibdeps` on this host).

## Verification snapshot

```text
Rust:              715 passed (712 + 3 CLI), 0 failed, 1 ignored
Frontend:          115 passed
Real-binary e2e:   75 passed (debug, 3.88.4)
Release e2e:       not run (environment refusal)
VS Code:           45 passed
Clippy/fmt:        clean
Release build:     3.88.4, `--version` verified
Version/metadata:  26 + 9 passed
Release/artifacts: 27 + 12 + 13 passed; installer 0 + 1 skipped
Audit:             0 vulnerabilities, 5 allowed warnings
```

## Conclusion

The existing chat, agent, workflow, skill, memory, desktop and extension modules now share their intended contracts on the paths exercised here. The Phase 4 defects were wiring defects, not missing architecture: a stored column was not returned, a stored skill was not applied, and a child run bypassed a configured ceiling. All three are fixed with regression coverage.

The system should not be described as fully integrated across unsupported platforms or external providers. It is integrated and verified on the Linux host for the paths and fixtures listed above, with the stated gaps remaining.
