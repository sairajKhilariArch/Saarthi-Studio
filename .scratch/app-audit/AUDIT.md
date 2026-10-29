# Saarthi Studio — Feature Inventory (audited 2026-10-29)

Statuses: Working / Partial / Missing / Broken / Unverified / Blocked.
Evidence = files, tests, or live checks. Updated every milestone close.

## Desktop (Rust server + static web UI)

| ID | Feature | Status | Evidence | Notes |
|---|---|---|---|---|
| D1 | Chat + provider streaming (`/api/chat`, `/api/chat/stream`) | Working | app/src/main.rs routes; live health/stream checks across sessions | SSE to web UI |
| D2 | Provider router lifecycle (`/api/router*`, `/omniroute*`) | Working | router.rs, main.rs:2237-2242 | |
| D3 | Router console proxy + login | Working | router_console.rs:433-435 | |
| D4 | Workspace ops (open/pick/list/read/write/create/rename/delete) | Working | workspace.rs:987-996; path-confinement + `.env` handling tests | guarded mutators |
| D5 | Terminal (open/input/resize/close/stream) | Working | terminal.rs; origin-guarded | stream is the origin-guard exception |
| D6 | Attachments + embeddings | Working | media.rs:39-40 | |
| D7 | Skills registry API | Working | skills.rs:326-328 | |
| D8 | SSE live mirroring `/api/events` | Working | events.rs; multi-tab refresh | covers conversations + agent runs |
| D9 | Agent runs: create/approve/cancel/list/detail | Working | agent.rs:869-872; live API flow verified this session (run → approval → detail) | |
| D10 | Agent bounded loop: think/list/read/write/command/done/fail | Working | agent.rs; HARD_MAX_STEPS 30, defaults 12, repeat-action guard, 90s command timeout | |
| D11 | Agent write diffs + per-step checkpoints + restore | Working | agent.rs (checkpoint column, unified_diff, /restore); live round-trip verified (write → diff in log → restore → disk reverted) | 200k-char checkpoint cap; restore is per-file only (documented) |
| D12 | Agent crash recovery → honest `interrupted` records | Working | agent.rs; verified on restart in an earlier session | |
| D13 | Deep search | Working | deepsearch.rs (682 lines) | |
| D14 | Host API (Android bridge): health/capabilities/models/chat | Working | host_api.rs:41-52; token auth + rate-limit tests | |
| D15 | Media flows in provider payloads | Working | media.rs | |
| D16 | **Privacy: secret scan + exclusions before context send** | Missing | recon grep: no secret/redact/exclusion logic in app context path | → M2 |
| D17 | **Adaptive model routing** | Working | main.rs `choose_tier`/`adaptive_tier` + agent.rs `step_once` override + failover step; settings UI (adaptiveRouting/cheapModel/strongModel); rule test in main.rs tests | M6 done; opt-in both-tiers-configured; off = no override (old behavior) |
| D18 | **Reproducible run records (model/config per run)** | Working | agent.rs: ADD model/router_config columns (idempotent migration), INSERT snapshot at create (base_url+adaptive+both tiers+base model), per-step `record_model`; run JSON exposes `model`/`routerConfig`; run card shows model tag | failover falls back to base model, logged as step |
| D19 | Git branch/worktree isolation | Missing | decision: not planned (in-place + per-file restore chosen) | parked |

## VS Code extension (Node server, lib.ts)

| ID | Feature | Status | Evidence | Notes |
|---|---|---|---|---|
| V1 | Chat + streaming | Working | lib.ts:1718-1736 | subset by design |
| V2 | SSE consumer /api/events | Working | lib.ts:1338 | |
| V3 | Workspace ops | Working | lib.ts:1811-1883 | |
| V4 | **Agent runs (API + panel)** | Missing | recon: no /api/runs handling in lib.ts | → M3 |
| V5 | **Privacy scan** | Missing | none | → M2 |
| V6 | Adaptive routing (VS Code) | Not targeted | extension proxies desktop run JSON; desktop-only UI this round | desktop feature; extension silently unaffected |

## Android client

| ID | Feature | Status | Evidence | Notes |
|---|---|---|---|---|
| A1 | Chat via desktop host | Partial | DesktopHostTransport.java header: "non-streaming until the later streaming milestone"; HTTPS + token enforced | → M4 |
| A2 | Direct router transport | Working | DirectRouterTransport.java | |
| A3 | Local DB, migrations, search, attachments, speech, policies | Working | ChatDatabase, DatabaseMigrations, ConversationSearch, AttachmentPolicy, BackupPolicy, RoutingPolicies | |
| A4 | **Agent runs** | Missing | no agent transport support | → M4 |
| A5 | **Streaming host chat** | Missing | same transport | → M4 |

## Packages / release

| ID | Feature | Status | Evidence | Notes |
|---|---|---|---|---|
| R1 | Linux desktop release | Working | linux-release.yml | |
| R2 | Windows desktop release | Working | windows-release.yml, desktop-release.yml | |
| R3 | macOS build target | Working (unsigned) | desktop-release.yml macos job (universal lipo, both-slice verify, headless HTTP smoke) + desktop-ci.yml macOS build/test | unsigned tarball; signing blocked (R7) |
| R4 | VS Code release | Working | vscode-release.yml | |
| R5 | Android release | Working | android-release.yml + android-ci.yml | |
| R6 | Rust host API CI | Working | rust-host-api-ci.yml | |
| R7 | macOS signing/notarization | Blocked | no credentials available in this environment | document, do not fake |

## Security posture (audited)

- File/terminal access path-confined + local-origin guard on mutators (workspace.rs, terminal.rs). Working (tests).
- Host API auth brainstormed: token + rate limits incl. failed auth (host_security tests). Working.
- No secrets logged: host tests assert Bearer secret absence from public body. Working.
- `.env` treated read-only in workspace listing. Working.
- Gaps: no capability isolation tiers beyond write/command perms on runs (same design), **no privacy/secret filtering on outbound context** (D16) → M2.

## Test baseline
`cargo test`: 98 passed / 0 failed (2026-10-30, after M6). Extension: 22/22. Android unit tests: green. JS syntax check clean.
