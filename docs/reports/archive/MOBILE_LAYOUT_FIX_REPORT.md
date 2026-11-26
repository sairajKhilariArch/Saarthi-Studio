# Mobile Layout Fix Report

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`

This report lists the actual layout defects found in the existing Android UI and
the structural fix applied to each. No arbitrary fixed widths were added to hide
overflow.

## 1. Header competed with status and actions at 320dp

**Before:** logo 40dp + brand + `Chats` + `Settings` text buttons. At 320dp the
title and both buttons could squeeze or clip, especially with large font scale.

**Fix:** the logo is 38dp, the brand block is `layout_weight=1`, and history and
settings are 42dp icon buttons with content descriptions. The model selector
moved out of the header into the status row, where it is a compact pill.

**Parent/child check:** header is `MATCH_PARENT` wide; brand takes remaining
space; actions have explicit 42dp targets. No text button participates in the
header width calculation.

## 2. Status line and model chip overlapped

**Before:** a single status `TextView` used the full row; adding a model control
would have pushed it off-screen.

**Fix:** status is now a weighted row: `statusText` uses weight 1 with ellipsis,
the model chip is `wrap_content` and truncates long model ids to 26 characters.
At 320dp the status truncates, never the send/stop control.

## 3. Chat had no bounded frame for overlays

**Before:** the `ScrollView` was added directly with weight 1, so a
scroll-to-bottom control could not be positioned without affecting layout.

**Fix:** the scroll view sits inside a `FrameLayout` that occupies the same
weight. The 44dp jump control is a bottom/end overlay with 10dp margins and
`GONE` until needed. The scroll view still fills the frame.

## 4. Assistant text had no surface and duplicated actions

**Before:** assistant content was bare text; `Copy response` was inside the
markdown renderer and `Message options` sat below it.

**Fix:** assistant content is wrapped in a bordered panel (`MATCH_PARENT` width
minus a 22dp end margin, height `WRAP_CONTENT`). The message-level action is a
single compact `••• Message actions` control; code blocks keep their own
language label and copy action. Long code blocks scroll horizontally inside
their own container rather than widening the message column.

## 5. Composer had no attachment affordance and a wide Send button

**Before:** attachments were hidden in `Tools`; Send was 78dp with no attach
control.

**Fix:** composer is a horizontal row with a 42dp attach control, a weighted
multiline input (1–4 lines) and a 78dp Send/Stop button. At 320dp the input
still receives the remaining width. `adjustResize` remains enabled, so the
composer stays above the keyboard; the root also applies live system-bar and
cutout insets.

## 6. Bottom navigation would sit under the navigation bar

**Before:** there was no bottom navigation.

**Fix:** the root already pads by live `systemBars() | displayCutout()` insets
(API 30+) and legacy insets below that. The bottom bar is the last child of the
padded root, so it sits above the gesture/navigation bar and respects the
display cutout. The bar is 50dp of content plus a 1dp divider.

## 7. Full-screen panels ignored safe areas

**Before:** feature forms used default dialog windows.

**Fix:** `fullScreenPanel()` builds a full-height `Dialog` with its own window
insets listener using the same API-level split as the activity, plus
`SOFT_INPUT_ADJUST_RESIZE`. A short screen scrolls its content instead of
clipping the header or the last row.

## 8. Conversation history used a fixed 430dp scroll area

**Status:** the existing history dialog still uses a bounded 430dp list to keep
the dialog's positive/negative buttons visible. It is functional and scrolls,
but it is a **remaining layout limitation** on very short screens or with the
keyboard open. It was not silently replaced with a fake responsive container.
The new feature surfaces avoid this pattern entirely.

## 9. Theme-version inconsistency

**Before:** `values/styles.xml` used indigo `#6B7FFF` while
`values-v27/styles.xml` used orange `#FF8A3D`, so Android 8.1+ looked different
from Android 8.0.

**Fix:** both use `#7C8CFF`; both status/navigation bars use `#061111`. A
`AppTheme.Light` was added for light-theme dialogs, with
`windowLightNavigationBar` limited to `values-v27` (it requires API 27 and the
app supports API 26). Lint `NewApi` error was fixed by moving that attribute,
not by suppressing it.

## 10. Empty state height and copy

**Before:** a long instruction block occupied most of the viewport.

**Fix:** the welcome surface is a single panel with a short “How can I help?”
headline, three 42dp quick-prompt rows and one short connection hint. It fits a
320×568 viewport without scrolling while remaining inside the chat `ScrollView`.

## Verification

- `lintDebug`: 0 errors after the API 26 attribute fix.
- `assembleDebug`: success.
- Screenshot/emulator evidence is recorded in `MOBILE_RESPONSIVE_TEST_REPORT.md`.
