# Sarthi Studio — Phase 7 Accessibility Report

**Date:** 2026-11-25
**Build:** 3.88.9

## Implemented

### Keyboard and focus

- Global `:focus-visible` outline: 2 px `var(--accent)` with 2 px offset. Dense-chrome exceptions (composer, agent search, skill back, preset chips, SCM hunks) keep a visible ring.
- Dialogs (`#folderModal`, `#promptModal`, extension panel) now:
  - move focus to the first control on open;
  - keep Tab and Shift+Tab inside the dialog (`modalFocusables` + `trapModalFocus`, skipping disabled and hidden controls);
  - restore focus to the control that opened them on close.
- Escape closes the folder dialog, prompt dialog, layout menu, palette, find bar and skill/model menus through their existing handlers.
- Tab semantics: segmented sidebar, dock tabs, bottom tabs and settings navigation expose `role="tab"`/`aria-selected`; activity buttons expose `aria-pressed`.

### Screen-reader semantics

- `#thinking` is `role="status" aria-live="polite"`.
- `#toast` is `role="status" aria-live="polite"`.
- `#messages` is a polite live region and now carries `aria-busy="true"` while a reply streams, `false` otherwise.
- Icon-only controls have `aria-label`s (panel toggles, terminal restart, thread delete/budget, dialog close, dock actions).
- New thread filter is a labelled `type="search"` input; SCM selection summary and problem counts are already live regions.
- Errors are text plus colour (`.notice`, `.msg.error`, permission refusals), never colour alone.

### Motion

- `prefers-reduced-motion: reduce` removes the message rise animation, spinner, status pulse and run-step pulse.

## Contrast (computed from the live DOM)

WCAG AA normal text requires 4.5:1. Every sampled pair passes in both themes.

| Element | Light foreground / background | Ratio | Dark foreground / background | Ratio |
|---|---|---|---|---|
| Muted note | `#6e675a` / `#f8f5ee` | 5.14 | `#7e8a84` / `#071312` | 5.27 |
| Brand subtitle | same | 5.14 | same | 5.27 |
| Sidebar tab | `#6e675a` / `#fffdf8` | 5.51 | `#7e8a84` / `#0c1b1a` | 4.93 |
| Activity icon | `#211d18` / `#e7e0d2` | 12.76 | `#efede4` / `#152827` | 13.12 |
| Tab | `#211d18` / `#f4f0e8` | 14.74 | `#efede4` / `#061111` | 16.34 |
| Status bar | `#4e483e` / `#fffdf8` | 8.90 | `#b4bdb6` / `#0c1b1a` | 9.18 |
| Accent button | white / `#3d56d9` | 5.95 | `#0a0c1e` / `#6b7fff` | 5.63 |
| Pane label | `#6e675a` / `#f8f5ee` | 5.14 | `#7e8a84` / `#071312` | 5.27 |
| Dock tab (active) | `#171310` / `#e7e0d2` | 14.06 | `#f7f4ea` / `#152827` | 13.98 |
| Input | `#211d18` / `#faf7f0` | 15.66 | `#efede4` / `#081514` | 15.88 |
| Workbench body | — | — | `#7e8a84` / `#061111` | 5.34 |
| Workbench heading | — | — | `#f7f4ea` / `#061111` | 17.41 |

## Automated coverage

`app/static/tests/ui-phase7.test.js` asserts:

- modal focus helpers skip disabled and hidden nodes;
- theme writer persists both themes;
- `#thinking` exposes `role="status" aria-live="polite"` and `#messages` exposes `aria-busy`;
- the thread filter is labelled and is a search input;
- the workbench and phone-dock behaviours that affect reachability.

The broader frontend suite covers view rendering, command palette and IDE layout contracts (125 tests total).

## Not verified

- **No screen-reader session** (NVDA/JAWS/Orca/VoiceOver) was run. Live-region markup is verified in source and DOM, not by hearing an announcement.
- **No automated axe/Lighthouse audit.** Contrast was calculated directly for sampled surfaces; every visible element was not exhaustively paired.
- **No full keyboard-only walkthrough** of every dialog and menu. Focus trapping and Escape handlers are unit/source verified; a human keyboard pass remains recommended, especially for the extension panel.
- No real touch or mobile assistive technology at 390 px.
- The Codebuff “Freebuff” pill visible in screenshots is injected by the hosting browser, not by Sarthi Studio, and is outside this application’s DOM/accessibility surface.

## Follow-ups

1. Run a keyboard-only pass and one screen-reader smoke on Linux.
2. Add an axe-core run against the served page for landmark/name/role regressions.
3. Give the dock header’s tablist `aria-controls` for `aiChatHost`/`aiRouterHost`.
