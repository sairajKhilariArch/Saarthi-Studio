# Sarthi Studio — Performance Baseline (Phase 5)

**Date:** 2026-11-25  
**Branch:** `remediation/security-2026-11-24`  
**Baseline commit:** `4722887` (3.88.4)  
**Measurement commits:** `db12aad` (3.88.5), `24c3359` (3.88.6)  
**Build:** `cargo build --release` (optimized), headless mode  
**Author:** Phase 5 performance work

## Environment

| Item | Value |
|---|---|
| CPU | AMD Ryzen 9 8940HX, 16 cores / 32 threads |
| RAM | 14 GiB |
| OS | Linux 7.3.5-arch1-1 |
| Filesystem | local ext4/btrfs worktree (project folder) |
| GPU | not used by the headless server; no GPU measurement applicable |
| Rust | pinned project toolchain, release profile |
| Provider fixture | `scripts/mock-router.py` on loopback; no external network latency included |
| Database | isolated `FREE_AI_STUDIO_HOME/.local/share/free-ai-studio/history.sqlite3` |
| Dataset | 100 conversations × 2,000 messages = **200,000 messages**, ~36 MB |
| Test duration | short functional runs plus a sustained 14-cycle soak; details in `STRESS_TEST_REPORT.md` |

All measurements used a scratch home. The real user configuration and database were
not opened. The benchmark home's config was pinned to the loopback fixture; a
separate concurrent server instance was deliberately avoided because it rewrote the
shared scratch config during an early attempt.

## Method

- Startup: launch the release binary, poll `/api/ready`, record wall time; sample
  `VmRSS`, `VmHWM`, `/proc/<pid>/io` and CPU after 10 s idle; send SIGTERM and
  time exit. Five or more runs per build.
- HTTP: `curl`/`urllib` against `/api/conversations`,
  `/api/conversations/:id/messages`, `/api/chat` and `/api/chat/stream`.
- Browser: the in-app Chromium preview at `http://127.0.0.1:39105/`, measured with
  `PerformanceObserver` long tasks and `MutationObserver` on `#messages`.
- Database: `EXPLAIN QUERY PLAN` through `sqlite3` and the Rust regression test.
- Streaming fixture: `bench-long-stream` returns ~19k visible Markdown
  characters from the mock router; earlier server-side totals count 3,904 delta
  frames and 31,227 delta characters.

No `hyperfine`, `perf`, `heaptrack`, `valgrind` or `strace` was installed. Timing
therefore uses wall-clock polling and `/proc`; browser main-thread work uses
`PerformanceObserver`. These are the limitations of this baseline, not a claim of
profiler-grade attribution.

## Baseline results (3.88.4, before optimization)

### Database plans and latency

| Check | Result |
|---|---|
| Thread read plan | `SCAN messages` (full table) |
| Conversation list plan | `SCAN v` plus a correlated `SCAN m` |
| `GET /api/conversations` | 0.68–0.79 s |
| `GET /api/conversations/bench-001/messages` | 16–22 ms, 459,894–465,301 bytes |
| Opening a 2,000-message thread in the browser | **3,589.5 ms** |

### Startup, idle and shutdown

| Metric | Result |
|---|---|
| Startup to `/api/ready` | 66–68 ms |
| Idle RSS | 88,588 KB |
| Idle CPU | 0.0% |
| Shutdown | 0–3 ms |
| Process I/O at idle sample | `rchar=2,151,474`, `wchar=11,556` |

### Chat and provider path

| Metric | Result |
|---|---|
| Non-stream chat, 30 turns | median 54.3 ms, p95 84.0 ms, max 110.3 ms |
| Chat on a 2,000-message thread | median 48.1 ms |
| Time to first token (server) | 26.6–29.6 ms |
| Server stream completion | 51.9–67.8 ms for 3,904 deltas |
| Browser first contentful paint | 406.9 ms |
| Conversation-list resource in browser | 790.9 ms |

### Streaming UI

An 18.9k-character answer produced **4 long tasks totalling ~106.6 s**, with the
longest ~37.9 s. The observer overlap may inflate the total, but the browser was
unresponsive for tens of seconds; the effect is not a borderline result.

## Reproduce

```bash
# scratch dataset described above; release binary
CARGO_ENCODED_RUSTFLAGS=$'-C\x1flink-arg=-fuse-ld=lld' cargo build --release
FREE_AI_STUDIO_HOME=$PWD/.scratch/phase5/bench-home \
FREE_AI_STUDIO_PORT=39105 FREE_AI_STUDIO_SKIP_ROUTER=1 \
app/target/release/free-ai-studio --headless

curl -s -o /dev/null -w '%{time_total}\n' http://127.0.0.1:39105/api/conversations
sqlite3 "$DB" "EXPLAIN QUERY PLAN SELECT id,role,content FROM messages \
  WHERE conversation_id='bench-001' ORDER BY id;"
```

The complete raw evidence is in `.scratch/phase5/` (`startup-before.txt`,
`chat-before.txt`, `startup-after.txt`, `stress-*.txt`, `memory-*.txt`,
`fault-*.txt`). That directory is intentionally gitignored.
