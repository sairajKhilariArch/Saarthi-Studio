# Sarthi Studio — Design System (implementation reference)

**Date:** 2026-11-25
**Scope:** desktop web UI and IDE shell in `app/static/`.
**Canonical narrative:** `docs/DESIGN-SYSTEM.md`. This file records how the system is implemented and how Phase 7 uses it.

## Principles

1. **Semantic tokens only.** Components read `--bg`, `--panel`, `--surface*`, `--line*`, `--text*`, `--accent*`, `--ok*`, `--warn`, `--danger*`, `--code-*`. No component introduces a literal colour. Phase 7 additions (`thread-filter`, terminal empty state, phone dock rule) use tokens exclusively.
2. **One accent for actions, one brand warmth.** Interaction blue (`--accent`) for primary actions and focus; terracotta (`--brand-grad`) only in the mark.
3. **Workbench density.** 11–12.5 px chrome text, 13.5 px message text, 12.5 px code, 24 px status bar, 44 px tab bar. Code is the only monospace surface.
4. **Both themes are first-class.** `:root` is the dark developer theme; `:root[data-theme="light"]` re-values the same names. The writer is `applyTheme(theme)` in `app.js`, persisted under `freeai-theme` and restored before first paint.

## Tokens (dark / light)

| Purpose | Token | Dark | Light |
|---|---|---|---|
| Canvas | `--bg` | `#061111` | `#f4f0e8` |
| Panel | `--panel` | `#071312` | `#f8f5ee` |
| Card surface | `--surface` | `#0c1b1a` | `#fffdf8` |
| Raised surface | `--surface-3` | `#152827` | `#e7e0d2` |
| Border | `--line` | `#1d3230` | `#e0d8c8` |
| Primary text | `--text` | `#efede4` | `#211d18` |
| Secondary text | `--dim` | `#b4bdb6` | `#4e483e` |
| Muted text | `--mute` | `#7e8a84` | `#6e675a` |
| Action accent | `--accent` | `#6b7fff` | `#3d56d9` |
| Success | `--ok` | `#34d399` | `#0f7a52` |
| Danger | `--danger` | `#ff7b72` | `#c02626` |
| Code canvas | `--code-bg` | `#081413` | `#fbf8f1` |

Syntax tokens live in `ide.css` (`--tok-key`, `--tok-str`, `--tok-num`, `--tok-com`, `--tok-fn`, `--tok-type`, `--tok-prop`, `--tok-tag`, `--tok-attr`, `--tok-punct`) and are re-valued for light. The chat now uses the same tokenizer, so a code block looks the same in chat and in the editor.

## Type and rhythm

- Faces: `--sans` (Inter/system), `--mono` (JetBrains Mono/Cascadia/Consolas), `--display` (serif, brand and welcome heading only).
- Scale: 10.5–11 px labels, 12–12.5 px chrome, 13.5 px body/message, 16.5–19 px message headings, 17–27 px page/welcome headings.
- Spacing increments used: 2, 4, 6, 8, 10, 12, 14, 18, 28 px. Page padding 28 px, dock padding 10–12 px, control padding 6–13 px.
- Radii: 5 px (chips/buttons in dense chrome), 7–9 px (inputs, cards, tabs), 12 px (`--radius`, dialogs/panels).
- Shadows: `--shadow-1` for cards/menus, `--shadow-2` for overlays and the narrow-screen dock.

## Components and states

| Component | Implementation | States covered |
|---|---|---|
| Buttons | `.btn`, `.btn-accent`, `.btn-quiet`, `.btn-small`, `.icon-btn` | hover, active, disabled, `:focus-visible` |
| Inputs | `.field`, `.agents-search`, new `.thread-filter` | placeholder, focus ring, invalid notice |
| Tabs | `.tabs`, `.segmented`, `.ai-dock-tab`, `.bottom-tab` | `aria-selected`, active underline/pill |
| Panels | `.sidebar`, `.main`, `.ai-dock`, `.bottom-panel`, `.status-bar` | hidden by user toggle, resizable dock/bottom |
| Dialogs | `.modal` + `.modal-card` | scrim, focus trap, Escape close, focus restore |
| Messages | `.msg`, `.msg-body`, `.code-block`, `.msg-actions` | streaming, error, stopped, attachments, media, copy |
| Status | `.status-pill`, `.conn-dot`, `.status-item` | connected/checking/bad, text + colour |
| Empty states | `.pane-empty`, `.empty-note`, `#workbenchEmpty`, terminal note | explanatory copy plus real actions where applicable |

## Accessibility rules

- Global `:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }`.
- Status is never colour-only: labels accompany dots and counts.
- Dialogs trap Tab and restore focus; `#toast` and `#thinking` are polite live regions; `#messages` exposes `aria-busy` while streaming.
- `@media (prefers-reduced-motion: reduce)` disables the message rise, spinner and status pulse.

## Responsive rules

- ≥1101 px: activity rail + sidebar + main + assistant dock in a grid; bottom panel spans main+dock.
- ≤1100 px: dock is a fixed overlay, dismissible through the panel state.
- ≤980 px: sidebar becomes a drawer; bottom panel and dock share the remaining width.
- ≤480 px: dock width is `calc(100vw - var(--activity-w))`, so it never covers the activity rail.
- User layout preferences (activity/left/right/bottom/status/center/maximize) are persisted through the existing layout menu.

## Phase 7 additions

- `.thread-filter` — token-based search field with focus-within ring.
- `#wsPath` two-line clamp so explanatory copy is not truncated.
- `.terminal-host .terminal-empty` — informative empty state in the real bottom panel.
- `#workbenchEmpty` actions — Open folder, New thread, Search files, all calling existing functions.
- Dock header model pill hidden at 300 px to prevent clipping; the model remains in the composer chip and status bar.
