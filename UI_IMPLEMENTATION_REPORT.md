# Sarthi Studio — Phase 7 Implementation Report

**Date:** 2026-11-25
**Branch:** `ui-ux/phase-7-2026-11-25`
**Version:** 3.88.9
**Base:** `562d950` (`3.88.8` security hardening) on `remediation/security-2026-11-24`

## What changed

The application already had a working IDE-style shell (activity bar, explorer, AI dock, tabbed bottom panel, status bar, command palette, real terminal, agents, skills, workflows, settings, themes). Phase 7 therefore fixed defects in that shell and connected the missing states, rather than replacing it.

### 1. Workbench home is reachable (`app/static/ide.js`)

- `#workbenchEmpty` was built after the first `setView` and left `hidden`; Explorer at boot never called `setView`.
- Boot now applies the same `editorVisible || pageVisible` rule as navigation.
- Pane activities (Explorer, Search, Source control) now leave a full-page view and return to the workbench when no editor is open. Clicking Settings → Explorer is verified live to hide Settings and show the home.
- Actions on the home call existing code: `openFolderDialog()`, `#newChat`, `openActivity("search")`.

### 2. Chat code rendering (`app/static/app.js`)

- `codeBlockHtml` now paints with the tested `highlightCode` from `highlight.js` (escaped before wrapping), falls back to escaped text when the highlighter is absent, adds `class="hljs language-<lang>"`, and gives the copy button `type="button"` plus `aria-label="Copy <lang> code"`.
- Unknown or hostile language labels are escaped; the highlighter itself escapes `<img>`/`<script>` payloads (regression test).
- Existing "Copy reply", "Resend", message attachments, media, stop-generation, streaming and paged history are untouched.

### 3. Thread filtering (`app/static/index.html`, `app.js`, `styles.css`)

- New labelled `type="search"` field in the Threads pane.
- `filterConversations(conversations, query)` matches title **or** project, case-insensitively; whitespace-only returns the original list.
- Empty result gets "No threads match that filter."; Escape clears the field.
- Existing scope toggle ("All projects" / "This project") and token-budget/delete actions are unchanged.

### 4. Dialog focus management (`app/static/app.js`)

- `modalFocusables(element)` collects enabled, visible links/buttons/inputs/selects/textareas.
- `trapModalFocus` wraps Tab and Shift+Tab at the ends.
- `showModal` attaches one listener per dialog and remembers `document.activeElement`; `hideModal` restores it.
- Applies to the folder dialog, prompt dialog, extension panel and any future `.modal` using these helpers.

### 5. Streaming semantics (`index.html`, `app.js`)

- `#thinking` is now `role="status" aria-live="polite"`.
- `#messages` starts with `aria-busy="false"`; `setBusy(value)` keeps it in sync with the real request lifecycle.

### 6. Responsive chrome (`app/static/ide.css`)

- New `@media (max-width: 480px)` rule gives the assistant dock `width: calc(100vw - var(--activity-w))`. Measured before/after at 390×844: dock left moved from 23.4 px to exactly 48 px, leaving the rail operable; page `scrollWidth` stayed 390.
- Dock header model pill hidden (duplicate of composer chip and status bar) to stop the "au" clipping at 300 px.

### 7. Empty-state polish (`index.html`, `styles.css`)

- Terminal host contains an explanatory note when no project is open; opening a terminal replaces it through the existing `replaceChildren` path.
- `#wsPath` clamps to two lines instead of truncating "Open a folder to browse and edi…".
- Composer placeholder shortened to fit the dock without clipping.

## Real integration (no fake UI)

| Control | Real path |
|---|---|
| Workbench Open folder | `openFolderDialog` → `/api/workspace/open` |
| New thread | `newConversation` → `POST /api/conversations` |
| Search files | `openActivity("search")` → indexed `/api/search` |
| Thread filter | client-side over `state.conversations` from `/api/conversations` |
| Code Copy | `navigator.clipboard` with textarea fallback |
| Terminal note | real `/api/terminal/open` still owns the panel |
| Theme | `applyTheme` → `<html data-theme>` + `localStorage` |
| Settings → Explorer | real view switch, no placeholder page |

## Files

```
 CHANGELOG.md                                        |  22 +
 app/Cargo.toml                                      |   2 +-
 app/Cargo.lock                                      |   2 +-
 app/packaging/...appdata.xml                        |   1 +
 app/static/app.js                                   |  79 +-
 app/static/ide.css                                  |  18 +-
 app/static/ide.js                                   |  19 +-
 app/static/index.html                               |  16 +-
 app/static/styles.css                               |  21 +
 app/static/tests/ui-phase7.test.js                  | new, 9 tests
 screenshots/phase7/                                 | before/after evidence
```

## Verification

| Check | Command | Result |
|---|---|---|
| Frontend | `bash scripts/ci/test-frontend.sh` | **125 passed / 0 failed** |
| Rust | `cd app && cargo test --locked` | **719 + 3 CLI passed**, 1 ignored |
| Lint/format | `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` | pass |
| Extension | `cd vscode-extension && npm test` | **45 passed** |
| Real binary E2E (debug) | `E2E_NO_BUILD=1 bash scripts/ci/test-e2e.sh` | **75 passed** |
| Real binary E2E (release) | release binary + `E2E_NO_BUILD=1` | **75 passed** |
| Website | `cd website && npm run build` | **37 pages** |
| Version contract | `bash scripts/ci/test-version.sh` | **26 passed** |
| Release contract | `bash scripts/ci/test-release.sh` | **27 passed** |

Live browser checks on 3.88.9: workbench visible on load; Settings → Explorer returns to workbench; terminal note present with no folder; code highlighting present in rendered markdown (`tok-key` spans); 390 px dock starts at 48 px; no horizontal overflow at 1920/1600/1366/1024/768/390.

Dependency audit was not rerun because Phase 7 changed no dependencies; the Phase 6 result remains 0 vulnerabilities and 5 allowed warnings.
