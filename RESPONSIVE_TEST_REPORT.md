# Sarthi Studio — Phase 7 Responsive Test Report

**Date:** 2026-11-25
**Build:** 3.88.9, served by the real Rust binary at `127.0.0.1:8765`
**Engine:** packaged Chromium via the Freebuff preview panel, viewport emulation.

## Matrix

| Viewport | `innerWidth` | `scrollWidth` | Horizontal overflow | Layout result |
|---|---|---|---|---|
| 1920 × 1200 | 1920 | 1920 | none | rail 48 + sidebar 286 + main + dock 300; bottom spans main+dock |
| 1600 × 900 | 1600 | 1600 | none | dock x=1300, width 300; composer fits its column |
| 1366 × 768 | 1366 | 1366 | none | main and dock remain usable; bottom panel 240 px |
| 1024 × 768 | 1024 | 1024 | none | ≤1100: dock becomes a fixed overlay; no page overflow |
| 768 × 1024 | 768 | 768 | none | sidebar is an off-canvas drawer (`left: -292` by design); dock 400 px at x=368 |
| 390 × 844 | 390 | 390 | none | dock 342 px at x=48; composer 321 px at x=59; rail fully exposed |

Overflow was measured by walking every visible element, excluding elements clipped by an ancestor with `overflow-x: auto|scroll|hidden`. The 768 px drawer elements appear outside the viewport but are translated off-canvas and do not create scroll; `scrollWidth` confirms this.

## Defect found and fixed

At 390 × 844, before the fix:

```
activity rail : x=0   width=48
assistant dock: x=23.4 width=366.6   ← covered 24.6 px of the rail
```

After adding `@media (max-width: 480px) { .app > .ai-dock { width: calc(100vw - var(--activity-w)); } }`:

```
activity rail : x=0  width=48
assistant dock: x=48 width=342
composer      : x=59 width=321
scrollWidth   : 390
```

Screenshot: `screenshots/phase7/after-mobile-390.png`.

## Panel behavior

- The assistant dock is resizable above 1100 px and its width is persisted (`freeai-panel-left/right/bottom`). During drag it is clamped to keep the editor column usable.
- Below 1100 px the dock is an overlay (`position: fixed`, `z-index: 40`, `--shadow-2`) and is dismissed through the same right-panel state; it is not a second permanent column.
- Below 980 px the sidebar is a drawer driven by `nav-open`; the activity rail remains the stable navigation.
- The bottom panel spans the main column and dock above 1100 px; below that it belongs to the remaining column and is hidden by the base stylesheet at ≤980 px, with its grid row collapsed to zero so chat gets the height.
- Dialogs use `width: min(560px, calc(100vw - 32px))` (and a narrower variant) and fit at 390 px.

## Theme behavior at width

Both themes were checked at 1600 px (full-page screenshots) and the 390 px dock rule uses tokens only, so it inherits both palettes. Theme persistence is covered by an automated test (`applyTheme` writes `<html data-theme>` and `freeai-theme`).

## What was not tested

- No physical phone or tablet; 390 × 844 and 768 × 1024 are Chromium emulation, not touch-device validation.
- The Freebuff window was covered during part of the session, so some screenshots used the full-page capture path. Screenshot canvases above ~984 px were cropped while the DOM still reported the emulated viewport; all width decisions above are based on DOM rectangles, not the cropped images.
- Android client and the VS Code webview are separate surfaces and were not visually tested in this phase.
- No automated pixel-diff suite. Visual changes were reviewed manually against the captured before/after images.

## Verdict

All six required widths render without accidental horizontal scrolling. Navigation remains reachable at every width, and the one concrete overlap defect (390 px dock over the rail) is fixed and regression-tested in `app/static/tests/ui-phase7.test.js`.
