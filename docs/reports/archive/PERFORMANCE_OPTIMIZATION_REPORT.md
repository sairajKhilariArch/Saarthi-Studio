# Sarthi Studio — Performance Optimization Report (Phase 5)

**Date:** 2026-11-25  
**Branch:** `remediation/security-2026-11-24`  
**Baseline:** `4722887` (3.88.4)  
**Optimized:** `db12aad` (3.88.5), `24c3359` (3.88.6)  
**Environment:** AMD Ryzen 9 8940HX, 32 threads, 14 GiB, Linux 7.3.5-arch1-1, release binary, 200k-message scratch database

## Result summary

Three measured bottlenecks were found and fixed. Server-side provider latency was
not a bottleneck. The largest user-visible gain is streaming: an 18.9k-character
answer went from multiple seconds-long main-thread tasks to **zero long tasks** and
about **186 ms** in a foreground browser tab.

| Metric | Before (3.88.4) | After (3.88.5/3.88.6) | Change |
|---|---|---|---|
| `GET /api/conversations` (200k messages) | 680–790 ms | 1.46–3.79 ms (median ~1.7 ms) | ~400× faster |
| 2,000-message thread read | 16–22 ms | 2.0–5.3 ms | ~4.5× faster |
| Open 2,000-message thread | 3,589.5 ms, 480 ms long task | 743 ms full render / **462 ms paged**, 2×82 ms tasks | ~7.8× faster paged |
| 19k-character stream UI | ~106.6 s across 4 long tasks, max ~37.9 s | **186 ms, 0 long tasks** | catastrophic → smooth |
| Chat, one writer | median 54.3 ms | median 59.1 ms | within noise |
| 16 concurrent chats | 4 / 16 failed `database is locked` | 0 / 32 failed | correctness fixed |
| 32 concurrent chats | not reached reliably | 64 / 64 ok, p95 393 ms | bounded waiting |
| Startup (index present) | 66–68 ms | 67–93 ms (median ~71 ms) | ~unchanged |
| First boot with index build | — | 188 ms one-time | ~120 ms migration |
| Idle RSS | 88,588 KB | 90,184 KB | +1.6 MB (index/page cache) |
| Idle CPU / shutdown | 0.0% / 0–3 ms | 0.0% / 0–5 ms | unchanged |
| Soak RSS after warm-up | — | 206.5 MB plateau, flat over 8 further cycles | bounded |
| Frontend JS heap after 10 large-thread switches | — | 5.5 → 7.3 MB, DOM rows 201 | no retention |

## Bottleneck 1 — every history read scanned the messages table

**Evidence.** `EXPLAIN QUERY PLAN` for the thread read was `SCAN messages`; the
conversation list used `SCAN v` plus a correlated `SCAN m`. The list took
680–790 ms over 200k messages.

**Cause.** No index existed for `messages(conversation_id, id)`, so both the
thread read and the list's "newest message per thread" worked over the full table.

**Fix** (`db12aad`, `app/src/main.rs`). `migrate_message_index()` creates
`messages_conversation ON messages(conversation_id, id)` from `apply_schema`, so it
is present on a fresh database and after every boot. The plan is now
`SEARCH messages USING INDEX messages_conversation (conversation_id=?)`.

**Cost.** First boot over 200k messages took 188 ms including index creation and
wrote ~8.3 MB; subsequent boots take 67–93 ms. Idle RSS rose 1.6 MB. The index is
created only when absent.

## Bottleneck 2 — bulk history rendering forced layout per message

**Evidence.** Clicking a 2,000-message thread took 3,589.5 ms. The renderer called
`addMessage` for every row, and each call scrolled the chat container, forcing
layout after each insertion.

**Fix** (`24c3359`, `app/static/app.js`). `addMessage` accepts `{scroll:false}`;
the history path renders without scrolling and scrolls once at the end. The first
measurement after this fix was 743 ms with one 480 ms long task.

**Further fix — presentation paging.** A long thread now opens with its latest
200 messages plus a `Load 200 earlier messages (N hidden)` control. Older pages
are inserted before the control without losing scroll position. This reduced the
open to **462 ms** and split the work into two 82 ms tasks. All messages remain in
memory and reachable; this is not data truncation.

## Bottleneck 3 — streaming re-rendered Markdown for every delta

**Evidence.** A 19k-character answer arrived as 3,904 deltas. Each delta rebuilt
the row's Markdown, producing four long tasks totalling ~106.6 s (max ~37.9 s).

**Fix** (`24c3359`, `app/static/app.js`). Streaming text is accumulated and
painted at most once per animation frame (`appendStreamText`, `streamRenderText`,
`streamRenderHandle`). `finishStreamRow` cancels a queued paint before the final
render. Measured in a foreground tab: **186 ms end-to-end, zero long tasks**.
When the tab is hidden, `requestAnimationFrame` is throttled by Chromium; a hidden
tab measured 1.64 s. That is browser scheduling, not main-thread blocking, and is
not used as the headline result.

## Bottleneck 4 — concurrent chat writes failed instead of waiting

**Evidence.** 16 simultaneous `/api/chat` calls: 4 returned
`HTTP 500 {"error":"database is locked"}` (12/16 succeeded). The failure repeated
at 8 and 32 workers.

**Cause.** The user-message transaction was deferred: it ran a `SELECT` first and
then upgraded to a write. SQLite returns `SQLITE_BUSY` on that upgrade immediately
and does not invoke `busy_timeout`. Several chat saves also opened separate
connections, and rollback journal mode serialized readers with the writer.

**Fix** (`db12aad`, `app/src/main.rs`).
- `begin_write()` uses `TransactionBehavior::Immediate`, acquiring the write lock
  up front so `busy_timeout` (10 s) applies.
- The chat history/save path uses it.
- `journal_mode=WAL` is set once at startup after the newer-schema refusal check;
  the mode is persistent. WAL allows reads during commits and keeps the
  byte-for-byte refusal invariant for databases written by newer builds.
- Startup directory errors now name the directory instead of surfacing a bare
  `File exists`.

**Verification.** 1×50, 4×32, 8×32, 16×32 and 32×64 concurrent chats all returned
0 errors. The 32-worker run had median 126.4 ms, p95 393.0 ms, max 598.1 ms.
A dedicated Rust test (`concurrent_chat_writes_wait_instead_of_failing`) holds the
write lock in eight threads and asserts all eight messages persist.

## Considered and not changed

- **Virtualization instead of paging.** Paging gives the immediate-open win with
  far less complexity and keeps native browser find/selection behavior for the
  visible page. Virtualizing 2,000 rows would add scroll measurement code and
  still need the server payload. Revisit only if much larger histories become
  normal.
- **Connection pool.** Each request still opens a short-lived SQLite connection.
  Writes are serialized by SQLite anyway, and the measured p95 at 16 workers is
  186 ms. A pool would be justified by a profile showing connection setup cost,
  which this environment could not measure without `strace`/`perf`.
- **Provider retry/backoff changes.** Server TTFT was 27–30 ms and stream
  completion 52–68 ms; no application overhead was measurable against the mock.
  Retry policy was therefore not changed blindly.

## Remaining performance issues

1. Opening a 2,000-message thread still costs ~462 ms including the 525 KB
   response and JSON parse. Only the latest 200 rows are painted, but the full
   history is still transferred. Server-side cursor pagination would reduce
   bandwidth and parse time for very large histories.
2. A 200k-message conversation list is now fast, but the list query still returns
   every conversation. A cap or virtualized sidebar would matter at thousands of
   threads.
3. Soak RSS reaches ~206 MB after sustained 16-way chat and large-read load before
   plateauing. This appears to be allocator/SQLite page cache retention, not a
   leak: eight further cycles added only ~1.7 MB total and file descriptors stayed
   at 27. A long-duration (hours) soak was not run.
4. No GPU path applies to the headless server; the native Tauri window was not
   profiled separately.
