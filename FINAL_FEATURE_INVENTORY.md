# Sarthi Studio — Final Feature Inventory

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Branch:** `acceptance/phase-10-final-2026-11-26`  
**Code verified:** 3.88.17 (`1d6e458`)  
**Verification host:** Linux x86_64, AMD Ryzen 9 8940HX, 14 GiB RAM, Arch Linux kernel 7.3.5

## State definitions

| State | Meaning |
|---|---|
| **PASS** | Implemented, exercised on the real release binary and/or the live UI in this phase, with recorded evidence |
| **PARTIAL** | Implemented and partly exercised; a named part is untested or unavailable |
| **FAIL** | Defect reproduced and not repaired |
| **BLOCKED** | Could not be tested here because of missing hardware, credentials, OS or authorization |
| **NOT IMPLEMENTED** | No implementation exists |
| **NOT APPLICABLE** | The capability does not apply to this product |

A passing compilation is not treated as a PASS for runtime behaviour. Mock providers are identified as mock; no mock result is reported as a live vendor integration.

## Core application

| Feature | Implementation | Backend | Frontend | Integration | Test coverage | Known limitations | Acceptance |
|---|---|---|---|---|---|---|---|
| Application startup (`--headless`, `--no-gui`, `--browser`) | Rust + Tauri 2 | Present | Served UI | Local loopback HTTP | Release binary cold 83 ms, warm 67–84 ms to `/api/ready`; E2E startup/restart | Native window not launched on a desktop session in this phase | PASS (headless). GUI window BLOCKED |
| Graceful shutdown | Rust signal handling | Present | Window close path | Router ownership cleanup | SIGTERM exit in 12 ms; E2E restart reuses port | GUI-close path untested here | PASS (headless) |
| Configuration (`config.json`, 0600) | Rust | Present | Settings forms | Keychain migration | E2E save/restart; corrupt-config backup added and tested 3.88.17 | Real OS keychain not exercised | PASS |
| User settings | Rust + UI | Present | Present | Config + DB | E2E; live Settings panel; help-text defect fixed | — | PASS |
| Theme management | CSS/JS | N/A | Light + dark, persisted | Local storage/config | Live round-trip dark `rgb(6,17,17)` → light `rgb(244,240,232)` | No screenshot captured | PASS |
| Navigation / command shell | HTML/JS | N/A | Sidebar + activity bar + panels | 180 API routes | Live snapshot: 10 primary navigation buttons, 281 buttons, no console errors; Skills and Settings exercised | Not every one of 281 controls was clicked | PARTIAL |
| Project management | Rust + SQLite | Present | Switcher, rename, archive/delete | Conversation scoping | E2E create/switch/rename/delete; project restore | — | PASS |
| Workspace management | Rust | Present | Explorer/tabs | Filesystem | E2E folder open, create/edit/rename/delete, restart; live 1,000-file tree | — | PASS |
| Conversation management | Rust + SQLite | Present | Threads, history paging | Chat + provider | E2E create/list/delete/restart; 1,000-message conversation opened in 2.2 ms | Per-message edit/delete/regenerate not implemented (documented debt) | PASS |
| Model management | Rust | Present | Model field/catalog | Router `/models` | E2E model restoration; mock catalog only | Live vendor catalogs not tested | PARTIAL |
| Provider management | Rust | Present | 13 presets + named providers | OpenAI-compatible HTTP | E2E provider add/switch/key masking; mock provider | Real vendor APIs not tested | PARTIAL |

## AI functionality

| Feature | Implementation | Backend | Frontend | Integration | Test coverage | Known limitations | Acceptance |
|---|---|---|---|---|---|---|---|
| Chat | Rust | Present | Composer/thread | `/v1/chat/completions` | E2E real binary against mock router, 76/76; 800 measured turns | Live provider not tested | PASS (mock), PARTIAL (vendor) |
| Streaming | Rust SSE | Present | Incremental render + Stop | Provider SSE | E2E slow stream, partial saved once, next turn succeeds | — | PASS |
| Model routing | Router-side | Present | Model selection | FreeLLMAPI/OmniRoute | Mock catalog/routing; E2E model restore | Actual fallback is router-owned, not app-owned | PARTIAL |
| Provider fallback | Router-side | Delegated | Status only | Router | Mock routing metadata; no forced live fallback | Depends on upstream quota | PARTIAL |
| Tool execution | Rust agent tools | Present | Approval cards | Workspace/terminal | E2E write approval, denial, path containment | Mock model script; real model tool choice untested | PASS |
| Context management | Rust | Present | `~used / budget` meter | Prompt assembly | Unit tests for budget/cropping; E2E long thread | — | PASS |
| Memory | Rust + SQLite | Present | Memory panel | Recall into prompt | Unit/integration tests; panel visible | Not separately load-tested | PASS |
| Retrieval / Deep search | Rust | Present | Toggle + cited sources | Exa/Tavily/Gemini/OpenAI | Unit/E2E configuration paths; no live vendor key | Live search vendors untested | PARTIAL |
| Attachments | Rust media store | Present | Inline images/audio/video | Provider data URLs | E2E attachment store and serving; frontend tests | Video depends on provider support | PASS |
| Code generation | Model behaviour | Present through chat | Markdown/code blocks + copy | Provider | Covered by chat/markdown tests | No separate code-action feature | PASS |

## Agent system

| Feature | Implementation | Backend | Frontend | Integration | Test coverage | Known limitations | Acceptance |
|---|---|---|---|---|---|---|---|
| Agent creation/config | Rust | Present | Agents panel/profiles | Provider + workspace | E2E direct run; measured create 4.5–6.6 ms | — | PASS |
| Agent execution | Rust runner | Present | Live run view | Provider tools | E2E 76/76; workflow child runs | Real model plans untested | PASS (mock) |
| Agent cancellation | Rust | Present | Cancel control | Process cleanup | E2E cancellation and killed-run recovery | — | PASS |
| Token budgets | Rust | Present | Budget fields | DB request log | Unit tests for budget refusal; E2E maxSteps | — | PASS |
| Agent permissions | Rust | Present | Approval dialog | Workspace/terminal | E2E allow/deny write and command; wrong write under `allowWrites:false` failed safely | — | PASS |
| Execution history | Rust + SQLite | Present | Run history | DB | E2E inspect run, restart retains log | — | PASS |
| Error recovery | Rust | Present | Error state | Provider/workspace | E2E provider outage, kill mid-run, restart marks `interrupted` | — | PASS |
| Concurrency limits | Rust | Present | — | — | `max_concurrent_runs` clamp; measured queued starts behind 2 active slots | Not a throughput claim | PASS |

## Skills system

| Feature | Implementation | Backend | Frontend | Integration | Test coverage | Known limitations | Acceptance |
|---|---|---|---|---|---|---|---|
| Skill discovery | Rust | Present | Skills panel | Project/user/bundled dirs | Live panel listed code-review, debug, tests; E2E discovery | — | PASS |
| Skill loading | Rust | Present | Enable/disable | Prompt injection | E2E skill run; live panel | — | PASS |
| Skill validation | Rust | Present | Error display | Manifest rules | E2E missing skill returns clear 4xx; unit tests | — | PASS |
| Skill compatibility | Rust | Present | Error display | `requires:` version | Unit tests compare app/skill versions | — | PASS |
| Skill execution | Rust | Present | Composer chip | Agent/workflow | E2E skill agent and two-node workflow | — | PASS |
| Skill security | Rust | Present | Trust notice | Local Markdown only | E2E hostile/missing paths; content is instructions, not executable code | Prompt-injection risk remains model-level | PASS (bounded) |
| Skill version management | Rust | Present | Version shown | `skills.json` | Unit tests install/version metadata | No remote registry | PARTIAL |

## Workflow system

| Feature | Implementation | Backend | Frontend | Integration | Test coverage | Known limitations | Acceptance |
|---|---|---|---|---|---|---|---|
| Workflow creation/editing | Rust + UI | Present | Workflow panel | Agents/skills | E2E create/export/delete; measured create 0.5 ms | — | PASS |
| Validation | Rust | Present | Error display | Graph rules | E2E missing skill rejected before run; unit tests | — | PASS |
| Execution | Rust | Present | Live nodes | Agent runner | E2E dependent two-agent workflow; measured run accept 0.9 ms | Real model output untested | PASS (mock) |
| Scheduling | Rust | Present | Interval field | Background loop | Unit tests due-window and restart claim | No live multi-hour schedule observation | PARTIAL |
| Resumption | Rust | Present | Run view | SQLite step log | E2E restart/resume of interrupted run | — | PASS |
| Cancellation | Rust | Present | Cancel | Runner | E2E cancellation | — | PASS |
| Logging | Rust + SQLite | Present | Step log | DB | E2E inspect child run/log | — | PASS |
| Retry | Rust | Present | Retry action | Runner | Covered by run tests/e2e retry path | — | PASS |

## Developer environment

| Feature | Implementation | Backend | Frontend | Integration | Test coverage | Known limitations | Acceptance |
|---|---|---|---|---|---|---|---|
| Project explorer | Rust | Present | Tree/tabs | Filesystem | E2E; live 1,000 files, entries 0.6 ms | Very large trees beyond 1,000 not measured | PASS |
| File access/editing | Rust | Present | Editor, atomic save | Filesystem | E2E create/edit/rename/delete, path escapes refused | 1 MB/binary limits by design | PASS |
| Terminal | Rust PTY | Present | Terminal panel | OS shell | E2E PTY round trip; live panel present | Windows/macOS runtime blocked | PASS (Linux) |
| Git integration | Rust | Present | Source control panel | `git` CLI | E2E status/add/commit flow | No conflict-resolution UI | PASS |
| Extensions | Rust | Present | Extensions settings | Local commands | Unit tests + extension run tests | Third-party extension ecosystem not present | PARTIAL |
| Developer tools | Rust | Present | Diagnostics | Local APIs | Unit tests; live panel | — | PASS |
| Language servers | Rust LSP client | Present | Status only | External LSP | Unit/protocol tests | No real language server installed/run here | PARTIAL |
| Debug adapter | Rust DAP client | Present | Minimal | External DAP | Unit/protocol tests | No real debugger peer run here | PARTIAL |

## Distribution

| Capability | Linux | Windows | macOS | Android | Web |
|---|---|---|---|---|---|
| Build | PASS local release | CI-tested | CI-tested | CI-tested | N/A |
| Installer/package | `.deb` payload-tested; system install BLOCKED (no dpkg/root) | MSI/zip built in CI; install BLOCKED | `.app`/tarball built in CI; install BLOCKED | Debug APK emulator PASS; release unsigned | N/A |
| Launch | Headless release PASS; native window BLOCKED | BLOCKED | BLOCKED | Emulator PASS | Loopback UI PASS |
| Upgrade | BLOCKED (no system install; payload replacement tested) | BLOCKED | BLOCKED | BLOCKED | N/A |
| Uninstall | BLOCKED | BLOCKED | BLOCKED | Not tested | N/A |
| Signing | unsigned | unsigned, Authenticode BLOCKED | unsigned, not notarized, BLOCKED | unsigned by design | N/A |
| Update check/integrity | Unit tests PASS; live update BLOCKED (no published release) | BLOCKED | BLOCKED | No self-update | N/A |

## Inventory conclusion

The implemented product surface is broad and the main desktop journeys work on the tested Linux host. The inventory does **not** support a claim of complete cross-platform production support: Windows/macOS runtime, signing, system package installation and live updates remain blocked, and every provider/search/MCP/LSP peer test used a local fixture rather than a live third-party service.
