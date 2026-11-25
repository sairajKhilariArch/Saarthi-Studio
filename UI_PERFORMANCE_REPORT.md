# Sarthi Studio — Phase 7 UI Performance Report

**Date:** 2026-11-25
**Build:** 3.88.9
**Runtime:** packaged Chromium, 1600 × 900, `performance.now()` around the real functions in the live page.

## Measurements

| Operation | Work | Measured |
|---|---|---|
| Markdown + code render | 40 replies, each with heading, bold, inline code, fenced `rust` block and list | **4.3 ms total** (~0.11 ms/reply) |
| Large plain render | 20 × 40,000-character messages | **2.2 ms total** |
| Thread filtering | 100 filters over 5,000 threads | **47.3 ms total** (~0.47 ms/filter) |
| Theme switch | 50 toggles (`applyTheme` + dataset + localStorage) | **~1 ms total** (~0.02 ms/toggle) |
| HTML escaping | 100 calls | **0.2 ms total** |
| Idle DOM | workbench + three panels, no conversation open | **2,002 elements** |

No measurable regression was introduced by syntax highlighting in chat: the tokenizer is the same function already used by the editor, it caps input (`HIGHLIGHT_LIMIT`), and both 40-mixed-reply and 800k-character test runs finished in single-digit milliseconds.

## Why this stays responsive

- **Long conversations are paged, not rebuilt.** Since 3.88.6 a thread opens with the latest 200 messages and an explicit earlier-page control. Phase 5 measured 2,000-message opens at ~462 ms, down from ~3.6 s.
- **Streaming is frame-batched.** 3.88.6 paints streamed text at most once per animation frame; a 19k-character answer completes in ~186 ms with no long tasks.
- **Conversation list is indexed.** The `messages(conversation_id, id)` index keeps the 200k-message list at ~1.46–3.79 ms (Phase 5 measurement).
- **Thread filter is linear and local.** It reads the already-loaded `state.conversations`; at 5,000 threads one pass is sub-millisecond, so no debounce was added.
- **Panel toggles do not re-render the chat.** They flip a body class and stored panel state; the dock and bottom panel are moved once at boot.
- **Theme changes only rewrite CSS custom properties.** No view re-render is involved (`applyTheme` + `<html data-theme>`), which matches the 0.02 ms toggle.
- **Bounded lists.** Problems (100), output (200), MCP logs and run history already cap their arrays; no unbounded Phase 7 list was added.

## Test conditions and limits

- Tests ran on an idle local server with an isolated profile; no provider request was made. Rendering time excludes network/model latency.
- No PerformanceObserver long-task run or heap/leak soak was performed in Phase 7. The numbers above are microbenchmarks of the real functions, not a full interaction trace.
- No virtualization was added. Paging is the existing strategy; a 10k-message single page remains untested because the UI never loads one.
- Thread filter was benchmarked directly, not through typing; keystroke latency includes browser input handling and is lower than the per-filter cost shown here.

## Follow-ups

1. Run a PerformanceObserver long-task trace during a real 20k-character stream on the packaged desktop app.
2. Add a 30-minute panel-toggle/heap soak to confirm listener and DOM stability.
3. If a thread ever exceeds the paged window, measure a virtualized message list before adding one.
