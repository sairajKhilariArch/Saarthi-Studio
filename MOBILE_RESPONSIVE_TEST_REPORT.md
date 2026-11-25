# Mobile Responsive Test Report

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`  
Status: **EMU_NOT_YET_STARTED** — desktop system image install was in progress; results below are programmatic.

## Goal

Verify the redesigned shell at several device sizes, portrait orientation, keyboard
open/closed, long conversations, long code blocks and both themes. Enforce no
accidental horizontal scroll, no clipping, and correct safe-area behaviour.

## Verified before emulator

### Device sizes simulated by constraints

All dimensions below were confirmed by reading the layout code and the
`dp()`-based sizing, not by guessing:

| Device (portrait) | Bottom bar | Header actions | Composer input width |
|---|---|---|---|
| 320×568 | 5×64dp items fit | 38dp logo + two 42dp icons | ≥ 150dp remaining |
| 360×800 | fits | fits | fits |
| 375×812 | fits | fits | fits |
| 390×844 | fits | fits | fits |
| 412×915 | fits | fits | fits |
| 432×960 | fits | fits | fits |

The composer, status row, model chip and bottom nav all use `layout_weight` or
`wrap_content` against the remaining width, so there is no hardcoded screen-width
assumption smaller than the real viewport.

### No horizontal scroll

- Assistant message panels are `MATCH_PARENT` width minus a configured end margin.
- Code blocks cap the message container width and scroll internally if a line is
  wider than the block.
- The welcome, settings, projects, skills, workflows, extensions and memory
  panels are full-width with a ScrollView, never a fixed-width card.
- Root padding is driven by live window insets, so no fixed inset assumption can
  cause overflow.

### Keyboard and safe area

- `MainActivity` runs `adjustResize`, so the root is resized when the keyboard
  opens.
- Root has an `OnApplyWindowInsetsListener` using `systemBars() | displayCutout()`
  on API 30+ and legacy insets below that, re-applied on every change.
- Full-screen panels apply the same inset listener and `SOFT_INPUT_ADJUST_RESIZE`.
- With the keyboard open, the bottom bar stays above the system bar because the
  root padding already includes the navigation-bar inset.

### Theme portability

- Dark and light are selected before `super.onCreate`, then palette fields are set.
- The same programmatic constants are used everywhere, so a theme switch recreates
  the activity and repaints every surface.
- Status/navigation bar colours and (on API 27+) light bar icons are updated at the
  same time.
- `values-styles.xml` and `values-v27/styles.xml` both use `#7C8CFF`, so the badge
  looks the same on API 26 and API 27+.

### Scroll and lists

- Spy/message history uses `MAX_VISIBLE_MESSAGES = 1500` and `MESSAGE_PAGE_SIZE =
  150`, loading older messages on demand.
- Long code blocks render inside stay-width containers that scroll horizontally
  inside the message rather than widening the whole chat.

## Tested with a real device

None yet. The emulator system image (`aosp_atd;x86_64`, API 35) was installed but
the install did not complete during this session.

## Intended emulator checks

After the emulator is running, these should be captured both themes:

1. Launch, no conversation: welcome, quick prompts, composer, bottom bar, model
   chip — dark and light.
2. Conversation header, status, model chip — dark and light.
3. Bottom navigation all five destinations — dark and light.
4. More hub — dark and light.
5. Conversation history — dark and light.
6. Settings (connection, key, model, host, Appearance) — dark and light.
7. Projects (Desktop-required empty state, and a real list if a Host is attached).
8. Skills list.
9. Workflows list + run + run detail.
10. Extensions inventory.
11. Memory list + add + delete.
12. Model picker.
13. Keyboard open while in the composer after a quick prompt.

## Known limitations

- Dark/light texture cannot be confirmed on a real device until the emulator runs.
- Project/files, agent creation, workflow authoring, skill installation and
  extension execution are intentionally absent and not done here.
- Notifications are not implemented.
