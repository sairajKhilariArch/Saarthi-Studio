# Sarthi Studio — Reliability and Fault-Injection Report (Phase 5)

**Date:** 2026-11-25  
**Branch:** `remediation/security-2026-11-24`  
**Build:** release 3.88.5/3.88.6 code, isolated scratch homes  
**Scope:** controlled failures only; no test touched the real user config or database.

## Environment and method

- Linux 7.3.5-arch1-1, AMD Ryzen 9 8940HX, 14 GiB RAM.
- A dedicated `FREE_AI_STUDIO_HOME` per scenario under `.scratch/phase5/`.
- Provider faults used a loopback Python fixture that returns HTTP 500, a
  truncated 200 response, or a slow SSE stream. Provider-down used a closed port.
- Database faults used copies of the scratch database; the corrupt case used
  random bytes in a new home.
- A kill test sent SIGKILL during a live stream, then restarted the same binary
  against the same database.

## Fault matrix

| Scenario | Expected | Observed | Data result |
|---|---|---|---|
| Provider port closed | actionable error, server stays up | `HTTP 502` in **12.1 ms**: "Could not reach router…"; `/api/ready` 200 afterwards | User turn saved once; no assistant turn; `request_log` error row |
| Provider HTTP 500 | error surfaced, no false answer | `HTTP 502` in **14.7 ms**: "Router returned HTTP 500: provider exploded"; ready 200 | No assistant answer for that turn |
| Provider 200 with malformed/truncated JSON | protocol error or clear failure | **Limitation:** `HTTP 200` and the malformed body is stored/shown as the answer | Not data loss, but not treated as a protocol error |
| Client disconnects mid-stream | stop work, keep only what arrived | Partial answer (`"chunk 0 "`) stored by the recorder; server ready 200 | No duplicate or full answer after disconnect; partial is intentional |
| Writer holds a 3 s `BEGIN IMMEDIATE` lock | wait, then commit | Chat returned **200 after 3.09 s**; `integrity_check` ok | Exactly one message persisted |
| Eight concurrent writers | all wait and commit | Regression test passes; all 8 rows present | No `database is locked` |
| SIGKILL during an active stream | no corruption; clear interrupted state | Process died; `integrity_check` = `ok`; only the user turn existed | Restart ready 200; conversation readable (1 message) |
| Corrupt database | refuse to boot half-working | Exit **1** with "file is not a database"; no listener | File left untouched; e2e also verifies port silence and restore |
| Unusable/unwritable data path | refuse safely with a clear error | Startup exits 1 with a data-directory error; e2e readiness reports an unwritable directory and recovery | No writes attempted to real data |
| No provider with an agent run | fail rather than hang | e2e: failed run records provider error; studio answers afterwards | No orphaned run |
| Agent workflow killed mid-run | marked interrupted, steps readable | e2e: restart marks the run interrupted; step log readable | No duplicated steps |
| Provider outage after restart | usable app | e2e: studio answers after provider outage | History intact |

## Notable behavior and remaining issue

### Malformed 200 response is surfaced as an answer

The fixture returned
`{"choices":[{"message":{"content":` followed by 200k `x` bytes and no closing
JSON. The app answered `HTTP 200` with a stringified object and saved it as the
assistant turn. This is not silent data loss, but it is not a protocol error
either. The provider-response parser falls back to the pretty-printed body when
the expected `choices/0/message/content` path is absent. A future fix should
distinguish "nonstandard but valid text" from "invalid JSON" before persisting.

### Client disconnect semantics

The stream recorder is designed to save whatever text arrived if the response
body is dropped. The test saw the first chunk saved and no later chunks. This is
consistent with that design; the UI shows the partial turn on reopen. It is not
counted as cancellation of the provider request in this report, because the test
could not prove when the upstream connection was dropped.

### Corrupt/unwritable storage is fail-closed

A corrupt database exits with a readable error and does not bind the port. A
recoverable copy is not automatic; the e2e suite separately verifies that
restoring the database brings the server back. The startup path now names the
settings/data directory it could not create instead of returning a bare OS error.

## Regression coverage

- `concurrent_chat_writes_wait_instead_of_failing` (new Rust test).
- `message_lookups_use_the_conversation_index` (new Rust test).
- e2e 75/75 includes provider outage, run failure, kill-mid-run, interrupted run
  recovery, corrupt database refusal, port silence, restore, unwritable-data
  readiness and recovery.
- Full results in `PERFORMANCE_OPTIMIZATION_REPORT.md` and the final Phase 5
  completion summary.

## What was not tested

- Machine power loss / filesystem crash during a commit (only process SIGKILL).
- Disk-full injection (no safe isolated filesystem quota was available).
- Windows/macOS-specific file locking and child-process behavior.
- Multi-hour soak or repeated application restarts under production data size.
- Destructive fault tests against real user data were deliberately not run.
