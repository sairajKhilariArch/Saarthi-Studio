# Sarthi Studio — Stress and Soak Test Report (Phase 5)

**Date:** 2026-11-25  
**Branch:** `remediation/security-2026-11-24`  
**Build:** release 3.88.5/3.88.6, scratch homes  
**Host:** AMD Ryzen 9 8940HX (32 threads), 14 GiB RAM, Linux 7.3.5-arch1-1

## Workloads

| Workload | Configuration |
|---|---|
| Large history | 100 conversations × 2,000 messages = 200,000 messages, ~36 MB |
| Sequential chat | 200 turns plus 200 large-thread reads |
| Concurrent chat | 1×50, 4×32, 8×32, 16×32, 32×64 turns |
| Concurrent streams | 4×16 and 8×16 `bench-long-stream` answers (~19k chars each) |
| Soak | 6 cycles of 32 chats + 16 large reads at 16 workers, then 8 more 32-chat cycles |
| Frontend | 10 switches between 2,000-message threads with the browser page visible |
| Agents/workflows | Existing 75-check e2e: direct run, dependent two-agent workflow, skill delivery, kill/restart, provider outage |

All chats used the loopback mock router, so provider network time is excluded.
This isolates application and database behavior.

## Concurrent chat results (after 3.88.5)

| Workers | Requests | OK | Errors | Median | p95 | Max |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 50 | 50 | 0 | 59.1 ms | 61.8 ms | 64.5 ms |
| 4 | 32 | 32 | 0 | 56.0 ms | 69.3 ms | 72.5 ms |
| 8 | 32 | 32 | 0 | 60.0 ms | 132.3 ms | 181.2 ms |
| 16 | 32 | 32 | 0 | 65.3 ms | 186.2 ms | 288.5 ms |
| 32 | 64 | 64 | 0 | 126.4 ms | 393.0 ms | 598.1 ms |

**Before the fix:** 16 workers produced 4 `database is locked` failures out of 16
(12 succeeded); 8 workers also produced failures. After the immediate transaction
and WAL change, no run failed at any level up to 32 workers. Latency grows with
contention as expected; it does not collapse or error.

## Concurrent streams

| Workers | Streams | Errors | Median | Max |
|---:|---:|---:|---:|---:|
| 4 | 16 | 1 (harness-side connection error) | 101.4 ms | 184.5 ms |
| 8 | 16 | 3 (harness-side connection errors) | 266.0 ms | 409.9 ms |

The server remained ready after both runs. The errors came from the Python test
client reading a closed keep-alive connection under concurrency, not from an
application 5xx; the successful streams completed in hundreds of milliseconds.
This is recorded as a harness limitation rather than presented as a clean pass.

## Memory and resource stability

| Point | RSS | File descriptors |
|---|---:|---:|
| Fresh server before stress | 89,360 KB | 12 |
| After 32-way concurrency | 137,872 KB | 26 |
| Before soak | 139,544 KB | 27 |
| Soak cycle 1 (32 chats + 16 large reads × 16 workers) | 170,056 KB | 27 |
| Cycle 2 | 184,824 KB | 27 |
| Cycle 3 | 194,560 KB | 27 |
| Cycle 4 | 201,472 KB | 27 |
| Cycle 5 | 203,408 KB | 27 |
| Cycle 6 | 204,824 KB | 27 |
| Eight further 32-chat cycles | **206,540 KB plateau** | 27 |
| Five seconds idle after load | 206,540 KB | 27 |

The first cycles grow as SQLite/allocator page caches warm; eight further cycles
added only ~1.7 MB total. File descriptors stayed constant. This is cache
retention with a stable ceiling, not an unbounded leak. A multi-hour soak was not
run, so long-term drift beyond this window is unknown.

### Frontend retention

Ten switches between 2,000-message threads:

- JS heap: **5.5 MB → 7.3 MB**
- `#messages` rows: **201** (latest page + control) at the end
- Thread items: 108

No per-switch DOM growth was observed. A live streaming turn was not repeated
hundreds of times in the browser; the server soak is the stronger evidence for
backend stability.

## Large histories

- Full thread read: 2.0–5.3 ms for 525,188 bytes after the index.
- Thread open: 743 ms when rendering all 2,000 rows; 462 ms with the 200-row page.
- Conversation list over 200k messages: 1.46–3.79 ms.
- First boot index build over 200k messages: 188 ms; subsequent startup 67–93 ms.

## Agent and workflow load

Agent concurrency is bounded in code: `max_concurrent_runs` is clamped to 1–4 and
non-git workspaces are forced to a single run (`app/src/agent.rs`). Workflows
execute their ready nodes sequentially. The existing e2e suite exercised the
following under a real binary and database:

- 75/75 checks passed, including direct agent runs, a two-agent dependent
  workflow, skill delivery, provider outage, a run killed mid-flight and marked
  `interrupted`, and a restart with the step log intact.
- No orphaned child process was observed after cancellation/shutdown in these
  tests; process cleanup remains covered by the existing command tests.

A separate high-volume agent stress (dozens of simultaneous tool-using runs) was
not run because it would require provisioning many provider responses and is
outside the available loopback fixture.

## Acceptance criteria

| Criterion | Result |
|---|---|
| No unbounded memory growth during sustained testing | Met within the measured window; 206.5 MB plateau |
| No orphaned child processes after cancellation/shutdown | Covered by existing tests; no orphan observed in this run |
| No unbounded agent/workflow concurrency | Met by code clamp (1–4, single-run without isolation) |
| No data corruption during recovery tests | Met: `integrity_check=ok` after lock, SIGKILL and restart |
| No blocking UI during normal streaming | Met: 186 ms, zero long tasks in a visible tab |
| No unexplained regression | Met: startup, idle RSS, CPU and shutdown within noise |
| Repeated operations return to a stable resource range | Met: RSS flat over eight extra cycles; fds constant |
| Improvements supported by benchmark evidence | Met: measured before/after in `PERFORMANCE_OPTIMIZATION_REPORT.md` |

## Known stress limitations

- The mock provider is local and instantaneous; real router queues, rate limits
  and token pacing are not represented.
- No disk-full, network-partition or multi-hour soak was run.
- Browser measurements use Chromium's observer APIs; a hidden tab throttles
  animation frames and is reported separately, not used as the headline.
