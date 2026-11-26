# Sarthi Studio — Phase 7 Completion Report

**Date:** 2026-11-25
**Branch:** `ui-ux/phase-7-2026-11-25`
**Version:** 3.88.9
**Base commit:** `562d950` (`3.88.8`, security hardening) on `remediation/security-2026-11-24`

## Outcome

Phase 7 is **implemented and verified for the audited desktop surface**, with the limitations listed below. The work fixed concrete defects in an already substantial IDE-style shell; it did not rebuild working features or replace backend data with mock UI.

## Acceptance criteria

| Criterion | Status | Evidence |
|---|---|---|
| Consistent developer-workspace layout | **Met** | Activity rail + sidebar + main + assistant dock + tabbed bottom + status bar; live measurements at six widths |
| Project navigation works | **Met** | Real `/api/workspace`, project switcher, explorer, recent folders, Git/SCM pane; workbench now returns from page views |
| Chat and code rendering | **Met** | Streaming, stop, copy reply, attachments, media, paged history; code blocks now highlighted with labelled copy |
| Agent and skills interfaces reflect backend state | **Met (inspected)** | Existing `/api/agents`, `/api/skills`, run state and errors are rendered from server responses; no fake state added. Not redesigned. |
| Workflow interfaces connected to real execution | **Met (retained)** | Automation view and workflow graph call the existing workflow APIs; Phase 7 did not replace or simulate execution |
| Dark and light themes across the app | **Met** | Token themes, persisted choice, both measured; 12 sampled contrast pairs pass AA |
| Responsive layouts tested | **Met** | 1920/1600/1366/1024/768/390; no page overflow; phone dock overlap fixed |
| Accessibility addressed and documented | **Partially met** | Focus-visible, dialog trap/restore, live regions, labels, reduced motion implemented; no screen-reader or axe run |
| UI performance measured | **Met** | 4.3 ms for 40 mixed replies, 2.2 ms for 800k chars, 0.47 ms/filter at 5k threads, 0.02 ms theme toggle |
| Existing functionality not regressed | **Met** | 719 Rust + 3 CLI, 45 extension, 75 debug + 75 release E2E, 37-page website build |
| Required automated tests pass | **Met** | 125 frontend tests, including 9 new Phase 7 contracts |
| Required reports contain real evidence | **Met** | Seven reports plus screenshots and command results |
| All changes committed on the dedicated branch | **Met** | See the commit on `ui-ux/phase-7-2026-11-25` |

## Defects fixed

1. Blank main column on first load; workbench home never shown.
2. Page views trapped the main column with no route back to the workbench.
3. Chat fenced code was not syntax-highlighted.
4. Dialogs did not trap Tab or restore focus.
5. Streaming state was not announced (`aria-busy`/live status).
6. 390 px assistant dock covered the activity rail.
7. No thread filter/search in the Threads pane.
8. Blank terminal panel and truncated no-folder hint.
9. Dock header model pill clipped at the default width.

## Checks actually run

```
bash scripts/ci/test-frontend.sh                        125 passed, 0 failed
cd app && cargo test --locked                           719 passed + 3 CLI, 1 ignored
cargo clippy --all-targets -- -D warnings               pass
cargo fmt --check                                       pass
cd vscode-extension && npm test                         45 passed
E2E_NO_BUILD=1 bash scripts/ci/test-e2e.sh              75 passed
release binary + E2E_NO_BUILD=1                          75 passed
cd website && npm run build                             37 pages
bash scripts/ci/test-version.sh                         26 passed
bash scripts/ci/test-release.sh                         27 passed
```

Live browser checks: workbench visible at startup; Settings → Explorer returns home; terminal empty state with no folder; thread filter present; highlighted markdown spans; 390 px dock at x=48; `scrollWidth === innerWidth` at all six widths. Contrast and performance values are recorded in their reports.

`cargo audit` was not rerun because no dependency or lockfile dependency changed in Phase 7; the latest recorded audit (Phase 6) is 0 vulnerabilities and 5 allowed warnings.

## Explicitly incomplete or limited

- **No screen-reader or axe run**, and no complete keyboard-only walkthrough. Dialog focus logic is unit/source tested; a human assistive-technology pass is still required.
- **No real mobile/tablet device or touch testing.** 390 × 844 and 768 × 1024 are Chromium emulation.
- **Visual regression is manual, not automated.** Screenshots are stored, but there is no pixel-diff CI. Some screenshot canvases were cropped at emulated widths above ~984 px while the DOM reported the full viewport; width decisions use DOM measurements.
- **Workflow builder, agent management and skills screens were audited for wiring but not visually redesigned.** Their existing real execution paths were preserved.
- **Right contextual panels remain the existing Assistant/Router dock tabs.** No new context panel (file details, execution info) was added.
- **No new virtualization.** Long threads continue to use the proven paging strategy from 3.88.6.
- **Android and VS Code webview UIs were not visually tested.** The extension test suite passes; its `media/ui` copy is generated by its own build.
- **Two final cosmetic tweaks** (dock model pill hidden, shorter composer placeholder) were verified by computed style after the window became covered, not by a new screenshot.

## Recommendation

Ship this branch for review as a UI polish and navigation fix. Before a release, schedule the assistive-technology pass and a real-device responsive pass; neither blocks the implemented fixes, but neither is claimed here.
