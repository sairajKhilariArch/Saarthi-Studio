# Sarthi Studio — Final Reliability Report

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Code verified:** 3.88.17 (`1d6e458`)  
**Primary evidence:** 76-check release-binary E2E (`scripts/ci/test-e2e.sh`), 721 Rust unit/integration tests, live UI, targeted boot tests

## Failure-scenario matrix

| Scenario | How tested | Observed result | State |
|---|---|---|---|
| Provider unavailable | E2E stops the mock router and runs an agent/chat; Phase 10 boot with `FREE_AI_STUDIO_SKIP_ROUTER=1` | Actionable connection error; no hang; server remains ready; next request after recovery succeeds | PASS |
| Network interruption | E2E aborts a real streaming fetch after a delta | Partial reply saved exactly once; next turn on the same conversation succeeds | PASS |
| Application crash | E2E kills the process mid-agent run, then restarts | Run marked `interrupted` on restart; no permanently `running` row; step log retained | PASS |
| Invalid credentials | Mock rejects/handles auth paths; no live vendor key | Provider error surfaced; no secret echoed | PARTIAL (no live vendor 401) |
| Database locked | Unit/concurrency coverage; Phase 5 16-/32-worker tests | WAL + immediate transactions; 0 lock failures up to 32 workers in Phase 5 | PASS (Phase 5 evidence) |
| Database migration failure | Rust tests: backup before migrate, failure rollback, newer-schema refusal | Existing database preserved and error returned | PASS |
| Unwritable application directory | E2E makes the data directory unwritable | Clear startup error; no silent fallback or data loss | PASS |
| Disk space exhaustion | **Not tested** | No safe way to fill the host filesystem in this session | BLOCKED |
| Corrupted configuration | Phase 10 release binary with `{broken` config | Boots with defaults, **keeps `config.json.corrupt-<epoch>`**, logs clear message | PASS (fixed) |
| Interrupted update | Unit tests for staged download/hash/rollback | Staged file discarded on hash/HTTP failure; current install untouched | PARTIAL (no live update) |
| Failed workflow | E2E missing skill and provider outage | Workflow rejects/ends with clear error; no stuck running state | PASS |
| Crashed agent process | E2E kill + restart | `interrupted`, resumable where allowed; resources released | PASS |
| User cancellation | E2E cancel/stop | Cancellation recorded; partial stream saved once; next turn works | PASS |
| Interrupted streaming response | E2E slow stream + abort | Same as network interruption | PASS |
| Corrupt database | E2E corrupt DB file | Startup refuses with a clear database error; no overwrite | PASS |
| Database permissions | E2E inspects directory/DB modes | `0700` directory, `0600` database | PASS |

## Resource cleanup

| Check | Result |
|---|---|
| Port closed after SIGTERM | Yes, `curl` refused; shutdown 12 ms |
| Child terminal/agent processes after cancel/kill | E2E found no orphaned children; process tree cleaned |
| Workflow child run after parent cancel | Marked cancelled/interrupted, not left `running` |
| File descriptors after load | Phase 5 held at 27; Phase 10 threads stable at 35 |
| Router ownership | Stops Docker router only if Studio started it (unit/E2E) |

## Double-submit and duplicate-write safety

- Chat writes are transactional; a stream aborted after a delta stores the partial assistant message once (E2E asserts exactly one).
- Config saves use temp file + rename, so a crash cannot leave a partially written `config.json`.
- Migration creates a backup before changing schema; a failed upgrade restores it.
- Agent approval is explicit per action; denied actions do not execute.

## Error clarity

Errors observed in E2E and live tests named the failing boundary and suggested an action: provider base URL unreachable, missing skill with the requested name, workspace path outside the opened folder, database unreadable, settings directory not writable. No test observed a silent failure where the UI remained “running” indefinitely.

## Repairs made

**Unreadable settings were silently lost.** Before 3.88.17, an invalid `config.json` was parsed as defaults and then rewritten by the active-project pointer, destroying the original. The release binary now copies it to `config.json.corrupt-<epoch>` and logs the backup path. Reproduction against 3.88.17 confirmed the original bytes survive; two unit tests cover the backup and the valid-config no-op.

## Soak and ongoing stability

The bounded soak is in `FINAL_PERFORMANCE_ACCEPTANCE.md`: ~7.5 minutes continuous chat/search, 0 errors, RSS 88,252 → 90,848 KB, last 2 minutes +108 KB. A 2–4 hour soak was **not run**; long-term drift and disk-exhaustion behaviour remain unverified.

## Verdict

**PASS for the tested failure modes, with two explicit gaps:** disk-full behaviour and a genuine multi-hour soak. The previously silent config-loss path is repaired and verified.
