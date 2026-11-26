# Sarthi Studio — Final UI/UX Acceptance

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**UI under test:** `http://127.0.0.1:38100/` served by the live app, Chromium preview, 3.88.16 process with 3.88.17 static assets reloaded  
**Viewports:** 1440×900 desktop, 390×844 narrow/mobile

## What was checked live

| Check | Result | Evidence |
|---|---|---|
| Page loads with correct title | PASS | `Sarthi Studio` document title; no console errors |
| Primary navigation | PASS (presence) | 10 labelled buttons: Explorer, Search, Source control, Run history, Modules, Agents, Skills, Projects, Settings |
| Skills panel | PASS | Opened live; 3 bundled skills (`code-review`, `debug`, `tests`), descriptions, paths, Enabled state and Disable buttons |
| Settings panel | PASS | Opened live; 14 sections, provider presets, router URL/key/model, Save/Test actions, provider list |
| Empty state | PASS | “No folder open — Open a folder to browse and edit files”; workspace tree explains containment |
| Dark theme | PASS | Toggle produced `data-theme=dark`, background `rgb(6, 17, 17)` |
| Light theme | PASS | Toggle round-trip produced `data-theme=light`, background `rgb(244, 240, 232)` |
| Narrow layout (390 px) | PASS | `scrollWidth - clientWidth = 0`; sidebar remains flex; panels reachable |
| Desktop layout (1440 px) | PASS | No horizontal overflow; primary navigation and panels present |
| Credential help text | **FIXED** | Live DOM now reads “A saved key goes in the OS credential store.” |
| Status bar | PASS | Project, count, idle, router state, model, cursor, spaces, encoding shown |
| Error states | PARTIAL | E2E exercises provider errors, missing skills, unwritable data; loading/empty states in UI inspected only where reachable |
| Keyboard accessibility | PARTIAL | Focusable controls and semantic labels present in the accessibility tree; a full keyboard-only traversal, focus-trap and screen-reader pass was not run |
| Every visible control performs its action | NOT FULLY VERIFIED | 281 buttons present; Skills, Settings and theme were exercised. No scripted click-through of all controls was performed, and some need credentials or a workspace |
| Long conversation rendering | PARTIAL | 1,000-message API load 2.2 ms and Phase 5 paged rendering (200 rows) exist; Phase 10 did not re-time browser rendering |
| Large project tree | PASS (server) | 1,000 files / 20 directories opened and indexed; browser tree not visually inspected at that scale |
| Terminal / Agents / Workflows panels | PARTIAL | Present in navigation; behaviour covered by E2E, not clicked in the live UI |
| Toast/loading polish | PARTIAL | Not exhaustively reviewed |

## Screenshots

**Not captured.** `preview_screenshot` repeatedly returned that the page “has not been drawn yet” because the Freebuff window is minimized/covered on this host. No screenshot file was created; the previously intended `.scratch/phase10-ui/desktop-dark.png` does not exist. DOM snapshots, live text extraction and computed-style checks were used instead. A visual pixel review remains outstanding.

## Visual/design observations

- Dark palette is near-black teal (`rgb(6,17,17)`); light palette is warm paper (`rgb(244,240,232)`). Both expose readable contrast in the inspected panels, but no automated contrast audit or screenshot review was performed.
- The activity bar, sidebar tabs and bottom status bar remain usable at 390 px; no horizontal scrollbar appeared.
- Skills paths are shown as absolute local filesystem paths. This is consistent with a local tool but exposes the username/home path on screen; acceptable for a local single-user app, worth noting for screenshots/screen sharing.
- The settings form uses a mix of listbox presets and free-text URL/key fields; the “auto” model hint and provider list are visible without scrolling at desktop width.

## Defects found and repaired

1. Saved-key help text contained a duplicated preposition (“goes to in the OS credential store”). Fixed in `app/static/app.js`; verified in the live DOM.
2. README runtime commands used a non-existent uppercase binary name and wrong environment variables. Fixed in `README.md`; no UI impact.

No UI crash, blank screen, horizontal overflow or console error was observed in the exercised paths.

## Verdict

**PARTIAL.** Core navigation, Skills, Settings, themes, empty state and responsive width pass. Screenshot capture, full control-by-control interaction, keyboard-only traversal and long-thread browser rendering were not completed and are not claimed.
