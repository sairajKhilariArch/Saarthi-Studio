# Sarthi Studio — Final Performance Acceptance

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Binary:** release 3.88.17 (`1d6e458`), `--headless`, loopback mock provider  
**Host:** AMD Ryzen 9 8940HX, 32 threads, 14 GiB RAM, Arch Linux kernel 7.3.5  
**Scratch homes:** `/tmp/phase10-cold`, `/tmp/phase10-perf` (real user data never opened)

## Method

- Startup: execute the release binary, poll `/api/ready` every 10 ms, record wall time from process spawn to first HTTP 200.
- Memory/CPU: `/proc/<pid>/statm`, `/proc/<pid>/stat`, `/proc/<pid>/status`; CPU seconds converted with `CLK_TCK`.
- Chat: sequential `POST /api/chat` against `scripts/mock-router.py`; latency includes HTTP, prompt assembly, provider round trip and SQLite write. Provider network time is loopback only.
- Conversation loading: `GET /api/conversations/<id>/messages` after each target size.
- Large project: 1,000 Python files in 20 directories; open, index refresh, root entries, text search.
- Soak: continuous chat + workspace search with 5 s sampling. The command runner capped a single call at 295 s; a second 120 s run completed. **Total continuous soak ≈ 7.5 minutes, not the requested 2–4 hours.**
- Comparison baseline: Phase 5 `PERFORMANCE_BASELINE.md` and `PERFORMANCE_OPTIMIZATION_REPORT.md` on the same host family. No improvement is claimed where the method or dataset differs.

## Startup and shutdown

| Metric | Phase 5 after optimization | Phase 10 | Assessment |
|---|---:|---:|---|
| Cold start to `/api/ready` (new home) | 67–93 ms median ~71 ms | **83 ms** | Comparable |
| Warm start to `/api/ready` (existing DB) | 67–93 ms | **67, 69, 84 ms** (3 runs) | Comparable |
| Shutdown (SIGTERM → process exit) | 0–5 ms | **12 ms** | Comparable; all connections closed |
| Boot RSS | 88,588–90,184 KB | **87,728–88,252 KB** | Comparable |
| Port after shutdown | closed | `curl` connection refused (`000`) | Correct cleanup |

## Idle and sustained resources

| Metric | Result |
|---|---|
| Idle CPU | **0.2–0.4 %** sampled by `ps`; 0.29 % mean over 120 s of periodic load |
| RSS after 1,000-message conversation + 1,000-file index | **92,484 KB** |
| RSS at start of 7.5-minute soak | **88,252 KB** |
| RSS after soak (last 120 s measured separately) | **90,848 KB** |
| Peak sampled RSS during measured 120 s | **90,848 KB** |
| RSS growth during measured 120 s | **+108 KB** |
| Threads at end | **35** |
| Chat turns during measured 120 s | 24, **0 errors** |

The 7.5-minute window is consistent with SQLite/allocator warm-up seen in Phase 5 and does not show unbounded growth. It is **not evidence of leak-freedom over hours**. Phase 5’s longer 14-cycle stress reached a 206.5 MB plateau; Phase 10 did not rerun that 200,000-message dataset.

## Chat and streaming

| Workload | Turns | p50 | p95 | Max | Stored result |
|---|---:|---:|---:|---:|---|
| 100-message conversation | 50 | **45.03 ms** | 46.14 ms | 47.06 ms | 100 messages |
| 500-message conversation | 200 | **45.15 ms** | 46.25 ms | 47.88 ms | 500 messages |
| 1,000-message conversation | 250 | **45.15 ms** | 46.32 ms | 49.51 ms | 1,000 messages |

All 500 turns answered successfully through the mock provider. Phase 5 measured median 54.3 ms pre-optimization and 59.1 ms after; the Phase 10 fixture’s 45 ms is not a claimed speedup because the provider fixture, prompt sizes and host load differ.

Streaming correctness is covered by the E2E suite (real SSE from the mock, Stop, partial saved exactly once, next turn succeeds). The Phase 5 browser measurement (19k-character stream: 186 ms, 0 long tasks) was not repeated in Phase 10; no new browser timing is claimed.

## Conversation and project loading

| Operation | Dataset | Result |
|---|---|---:|
| Load 100 messages | 1 conversation | **0.68 ms** |
| Load 500 messages | 1 conversation | **1.16 ms** |
| Load 1,000 messages | 1 conversation | **2.20 ms** |
| Open 1,000-file project | 20 dirs / 1,000 files | **8.7 ms** |
| Index refresh | 1,000 files | **39.1 ms** (indexed 1,000 files, 1,000 symbols) |
| Root directory entries | 20 dirs | **0.6 ms** |
| Workspace text search | 1,000 files | **4.9 ms** |

The Phase 5 2,000-message thread read was 2.0–5.3 ms; 1,000 messages at 2.2 ms is consistent.

## Agent and workflow startup

| Metric | Result |
|---|---:|
| Create agent run (`POST /api/runs`) | **4.4–6.6 ms** |
| First run reaching `running` when a slot is free | **5.1 ms** |
| First run reaching `running` while two runs occupy the 2-slot limit | **1.31 s / 1.36 s** (queued, not failed) |
| Create workflow | **0.54 ms** |
| Accept workflow run | **0.90 ms** |

The queued starts are the documented `max_concurrent_runs` boundary, not a regression. A deliberately write-requiring run under `allowWrites:false` failed after its step budget instead of writing, which is permission enforcement, not a performance result.

## Baseline comparison and limitations

| Claim | Supported? |
|---|---|
| Startup within the Phase 5 67–93 ms band | **Yes** |
| Idle RSS within the Phase 5 88–90 MB band | **Yes** on boot; 92.5 MB after 1,000 messages and indexing |
| Conversation loading improved versus 3.88.4 | **Yes**, consistent with the Phase 5 index fix; Phase 10 measured 2.2 ms at 1,000 messages |
| No memory leak over 2–4 hours | **Not established** — only ~7.5 minutes |
| Browser render / long-task performance | **Not re-measured in Phase 10** |
| Cross-platform performance | **Not measured** — no Windows/macOS runtime |
| Real provider latency | **Not measured** — mock loopback only |

## Verdict

**PASS with a limited-duration soak.** Startup, chat, conversation load, project indexing and agent/workflow acceptance meet or remain within the Phase 5 baseline on this host. The 2–4 hour soak requirement is **BLOCKED by session limits** and is recorded as not run, not as passed.
