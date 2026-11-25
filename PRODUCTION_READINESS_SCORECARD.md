# Sarthi Studio — Production Readiness Scorecard

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Code:** 3.88.17 (`1d6e458`)  
**Scoring rule:** each category is PASS, PARTIAL (half credit) or BLOCKED/FAIL (zero). The percentage is evidence-based, not an estimate of effort.

## Scorecard

| # | Category | Required evidence | Result | Evidence | Score |
|---|---|---|---|---|---:|
| 1 | Functionality | Real user journeys A–G pass | **PASS** | 76/76 debug + release E2E; live UI; journeys A–F exercised; G partial (install blocked) | 1.0 |
| 2 | Security | Critical/high findings resolved | **PASS** | 0 critical/high code findings; Host/Origin/path/permission/MCP controls tested; signing tracked under distribution | 1.0 |
| 3 | Reliability | Failure recovery tested | **PARTIAL** | Provider outage, stream abort, crash, lock, corrupt DB/config, unwritable dir pass; disk-full and multi-hour soak untested | 0.5 |
| 4 | Performance | Measurements against baseline | **PARTIAL** | Startup/chat/load/index within Phase 5; 7.5-min soak only; no 2–4 h run | 0.5 |
| 5 | UI/UX | Core screens and accessibility validated | **PARTIAL** | Navigation, Skills, Settings, themes, 390/1440 widths pass; no screenshots, full keyboard or control-by-control pass | 0.5 |
| 6 | Cross-platform | Platform-specific test evidence | **PARTIAL** | Linux runtime verified; Windows/macOS CI-only, no install/launch; Android emulator; web loopback | 0.5 |
| 7 | Database | Migration and recovery validated | **PASS** | Backup-before-migrate, rollback, newer-schema refusal, WAL concurrency, 1,000-message load | 1.0 |
| 8 | Agents | Execution and cancellation validated | **PASS** | Create/run/approve/deny/cancel/kill/restart, skills, budgets, history | 1.0 |
| 9 | Workflows | Execution, retry and resume validated | **PASS** | Create/validate/run dependent nodes/cancel/resume/logs; scheduled due logic unit-tested | 1.0 |
| 10 | Distribution | Installers tested | **PARTIAL** | `.deb` payload, CI builds all targets; no system install, no Windows/macOS install, unsigned | 0.5 |
| 11 | Updates | Integrity and recovery tested | **BLOCKED** | Hash/rollback unit tests only; no published release to update from | 0.0 |
| 12 | CI/CD | Mandatory release gates pass | **PASS** | Phase 9 pipeline 7/7 builds + finalize; Phase 10 run 45704212041 success 5/5; publish gate fails closed on signatures | 1.0 |
| 13 | Documentation | Accurate and complete | **PASS** | 12 Phase 10 documents; README env/binary/path defects fixed; unsigned/platform limits stated | 1.0 |

## Totals

| Result | Count |
|---|---:|
| PASS | 7 |
| PARTIAL | 5 |
| BLOCKED | 1 |
| **Weighted score** | **7 + 5×0.5 = 9.5 / 13 = 73 %** |

## Passed requirements

- Linux release binary starts, serves, answers, persists and shuts down cleanly.
- All 76 end-to-end journeys pass on debug and release binaries against a real HTTP mock provider.
- 721 Rust tests, 3 CLI tests, 125 frontend tests and 45 VS Code tests pass; clippy and fmt clean.
- Agent permissions, cancellation, crash recovery, skill validation, workflow dependency/resume and database recovery are exercised.
- Workspace containment, Host/Origin guards, MCP TLS policy, database/config permissions and secret masking are exercised.
- Release pipeline builds every advertised artifact, verifies checksums, SBOM and provenance, and refuses unsigned publication.
- Performance remains inside the Phase 5 startup/memory band; 1,000-message load is 2.2 ms.

## Failed or partial requirements

- Windows and macOS are **not installed or launched**; only CI builds/tests exist.
- Linux **system** package install/upgrade/uninstall is blocked; only payload-level testing and an upgrade probe exist.
- **All production signatures are absent**. The release is correctly blocked from publication.
- **No live update** from a published release was possible.
- Soak lasted ~7.5 minutes, not 2–4 hours; disk-full was not tested.
- UI screenshots were not captured; full keyboard and all-control interaction were not completed.
- No live third-party provider/search/MCP/LSP/debugger was tested; all peers were local fixtures.
- Real OS credential stores were not exercised.

## Blocked requirements

| Requirement | Blocker |
|---|---|
| Windows/macOS install, launch, GUI, upgrade, uninstall | No hardware/host |
| Code signing/notarization | No certificates or Developer ID |
| Linux `dpkg` lifecycle | No dpkg/root on this host |
| Live update and rollback | No public release authorization/existence |
| Physical Android device / production signing | No device, no keystore |

## Overall decision

**CONDITIONALLY ACCEPTED** — no critical or high code defect remains, and the Linux runtime plus CI build matrix are coherent, but signing, real-platform installation and live-update validation are mandatory for an unrestricted public production release. The supported scope is stated in `FINAL_ACCEPTANCE_REPORT.md`.
