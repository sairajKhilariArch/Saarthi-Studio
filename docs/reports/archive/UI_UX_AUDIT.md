# Sarthi Studio — Phase 7 UI/UX Audit

**Date:** 2026-11-25
**Branch:** `ui-ux/phase-7-2026-11-25`
**Version:** 3.88.9
**Surface audited:** the real `app/static` desktop UI served by the Rust binary in headless mode, with an isolated `HOME=/tmp/sarthi-phase7`.

## Method

- Read the actual shell (`index.html`), application script (`app.js`, 10,992 lines), IDE layer (`ide.js`, 3,189 lines) and both stylesheets (`styles.css`, `ide.css`).
- Ran the app at `http://127.0.0.1:8765` and inspected the live DOM at the six required widths.
- Measured `documentElement.scrollWidth` against `innerWidth`, computed rectangles for the activity rail, sidebar, main column, assistant dock, bottom panel and status bar, and computed WCAG contrast for sampled text/background pairs in both themes.
- Captured before/after screenshots in `screenshots/phase7/`.
- Checked every finding against backend wiring before calling it a defect; no mock data was introduced.

## Findings and disposition

| ID | Severity | Finding | Evidence | Disposition |
|---|---|---|---|---|
| UI-01 | High (usability) | First load left the main column completely blank. The workbench home existed in `ide.js` but was created after `app.js` applied its first view and was never unhidden; the Explorer activity did not call `setView`. | `screenshots/phase7/before-dark-1600.png` shows only the tab bar in the main column. | **Fixed.** Boot now applies the same `editorVisible \|\| pageVisible` rule used by later navigation. The home shows project state and real Open folder / New thread / Search actions. |
| UI-02 | Medium | Chat fenced code blocks were escaped plain text. The language label and Copy button existed, but the tested tokenizer used by the editor was not applied. | `codeBlockHtml` called only `escapeHtml`; `highlightCode` existed in `highlight.js`. | **Fixed.** Chat blocks now use `highlightCode` with an escaped-language fallback, a `language-*` class and a labelled `type="button"` copy control. |
| UI-03 | Medium (accessibility) | Dialogs moved focus into the first field but let Tab leave the dialog and did not restore focus to the opener. | `showModal` only called `focus()`; `hideModal` only added `hidden`. | **Fixed.** `modalFocusables`/`trapModalFocus` keep Tab and Shift+Tab inside the open dialog, and `hideModal` restores the remembered opener. |
| UI-04 | Medium (accessibility) | Streaming state was visual only: the thinking indicator had no live-region role and the message list did not expose busy state. | `#thinking` had only a spinner and text; `#messages` had `aria-live` but no `aria-busy`. | **Fixed.** `#thinking` is `role="status" aria-live="polite"`; `setBusy` maintains `aria-busy` on the message list. |
| UI-05 | Medium (responsive) | At 390 px the assistant dock started at x≈23 while the activity rail is 48 px wide, covering part of the primary navigation. | Live rect measurement before the fix: dock `{left: 23.4, width: 366.6}`, rail `{width: 48}`. | **Fixed.** A `max-width: 480px` rule sizes the dock as `calc(100vw - var(--activity-w))`. After: dock `{left: 48, width: 342}`, no page overflow. |
| UI-06 | Medium (usability) | The thread pane had no way to search or filter conversations; only the project scope toggle existed. | `renderConversations` rendered every thread unconditionally. | **Fixed.** Added a search field with Escape-to-clear and a pure `filterConversations` matcher over title and project. |
| UI-07 | Low | With no folder open the bottom panel was a blank 240 px rectangle, and the Explorer hint was clipped to “Open a folder to browse and edi…”. | `screenshots/phase7/before-dark-1600.png`; `#wsPath` was `white-space: nowrap` with ellipsis. | **Fixed.** Terminal shows an explanatory empty state; the hint wraps to two lines. |
| UI-08 | Medium (navigation) | After opening a page view (for example Settings), clicking Explorer/Search/Source control changed the sidebar but left the page in the main column, with no visible route back to the workbench. | Live check: Settings visible, workbench hidden before the fix. | **Fixed.** Pane activities return the main column to the workbench when no editor is open. Verified live: Settings → Explorer yields workbench visible and Settings hidden. |
| UI-09 | Low | The assistant header clipped its model pill to “au” at the default 300 px dock width. | Full-page screenshot at 1600×900. | **Fixed.** The duplicate model pill is hidden in the dock header; the composer chip and status bar still show the model. |

## Observations left unchanged

- The assistant dock defaults to 300 px. It is resizable and persisted; widening it would reduce the editor column at 1366 px. No change without a measured need.
- Workflow, runs, agents, skills, settings and source-control views already have real loading, empty and error states. They were inspected but not redesigned.
- Some legacy inline styles remain (`style="grid-column: 1 / 3"`, hidden model wrappers). They are layout-only and were not mass-rewritten to avoid churn.
- No fake controls were found in the audited surfaces. Buttons that need a provider or folder still surface their real backend error.

## Measurements (live, after fixes)

| Width × height | `scrollWidth` | Horizontal overflow | Notes |
|---|---|---|---|
| 1920 × 1200 | 1920 | none | four-column workbench |
| 1600 × 900 | 1600 | none | dock 300 px at x=1300 |
| 1366 × 768 | 1366 | none | dock and bottom panel intact |
| 1024 × 768 | 1024 | none | dock becomes overlay |
| 768 × 1024 | 768 | none | sidebar is an intentional drawer at `left: -292` |
| 390 × 844 | 390 | none | dock starts at the 48 px rail, composer 321 px wide |

## Verification

- Frontend suite: **125 passed, 0 failed** (`bash scripts/ci/test-frontend.sh`), including nine new Phase 7 contracts.
- Rust: **719 passed + 3 CLI**, clippy `-D warnings`, `cargo fmt --check`.
- Real-binary E2E: **75/75 debug and 75/75 release**.
- VS Code extension: **45/45**; website build: **37 pages**.

Screenshots above were captured before the final two cosmetic tweaks (dock-header model pill hidden, shorter composer placeholder); those were verified by computed style (`display: none`, textarea `scrollHeight === clientHeight`) because the Freebuff window became covered.
