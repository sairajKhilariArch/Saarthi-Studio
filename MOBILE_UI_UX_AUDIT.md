# Mobile UI/UX Audit — Saarthi Studio Android

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`

## Before

The app was a single programmatic `Activity` with a usable but unfinished
visual shell:

- No bottom navigation. History and Settings were small text buttons in the
  header; every other feature was buried in a long `Tools` dialog.
- Dark-only palette. The v27 resource override changed the accent to orange
  while the Java UI used indigo, so the same product looked different across
  Android versions.
- Assistant replies were bare text on the page background, with a separate
  `Copy response` button inside the markdown renderer and another
  `Message options` button below it.
- No model indicator in chat. The model could only be changed by opening the
  whole Settings form.
- No scroll-to-bottom control. Auto-scroll was unconditional during streaming,
  so a user reading earlier messages was pulled back down.
- The empty conversation was a wall of setup instructions rather than a
  welcoming product surface.
- Conversation history used a fixed `430dp` scroll area, which clips when the
  screen is short or the keyboard opens.
- Projects, Skills, Workflows, Memory and Extensions had no mobile route at all.
- Full-screen feature surfaces did not exist; large forms lived in default
  `AlertDialog` windows with no safe-area handling.
- The status line and action buttons competed for one row at 320dp width.

## After

### Visual identity

The app now uses one explicit design system in `MainActivity`:

| Token | Dark | Light |
|---|---|---|
| Background | `#061111` | `#F7F4ED` |
| Panel | `#091212` | `#FFFDF9` |
| Panel elevated | `#0E1C1B` | `#EFEBE2` |
| Text | `#F1EFE8` | `#121E1C` |
| Muted | `#8C9A94` | `#586863` |
| Accent | `#7C8CFF` | `#4354D6` |
| Success | `#3DDC97` | `#0A845E` |
| Danger | `#FF7B72` | `#B52E26` |
| Line | `#1E302E` | `#DAD3C7` |
| User bubble | `#132423` | `#E8EBFF` |

Both themes are selected at runtime before `super.onCreate`, persisted in
`sarthi_theme`, and apply to every programmatic surface, status/navigation bars,
dialogs and full-screen panels. The `values-v27` orange override was removed.

### Navigation

A five-destination bottom bar replaces the text-button shell:

1. **Chat** — returns to the conversation and scrolls to the latest message.
2. **Chats** — conversation history with search, filters, folders, tags, pin,
   favourite, rename and delete.
3. **Projects** — real Desktop Host projects: list, create, activate.
4. **Agents** — real host agent runs with detail, approval and cancellation.
5. **More** — Projects, Skills, Workflows, Extensions, Memory, Models,
   Appearance, Settings, presets, agents, backup, diagnostics and About.

Header actions remain one tap away: conversation history (`☰`) and Settings
(`⚙`). The model chip sits beside the connection status and opens a real model
picker.

### Chat

- Assistant replies now sit in a bordered surface with an accent author label.
- Streaming starts with a visible `SARTHI · THINKING` state and animated dots
  (text-based, no blocking animation), then renders a caret while tokens arrive.
- A floating scroll-to-bottom control appears only when the reader is more than
  180dp from the end and disappears on tap.
- The composer has a dedicated attach button, rounded input and a high-contrast
  Send/Stop button.
- Empty state is now “How can I help?” with three real quick prompts that fill
  the composer and focus the keyboard.
- Message actions remain complete: edit/resend, copy, read aloud, regenerate,
  retry after failure.

### Full-screen feature surfaces

Projects, Skills, Workflows, Extensions, Memory and workflow-run details open in
a full-height panel with a back action, title, subtitle, safe-area padding and
scrollable content. They are not squeezed into small dialogs.

### Settings

Settings keeps all existing connection, key, model, fallback and host controls
and now exposes an Appearance row that switches dark/light and recreates the
activity with the persisted choice.

## Accessibility and touch

- Bottom destinations are 50dp high and 64dp wide at 360dp width.
- Header icon buttons are 42dp; the scroll control is 44dp; primary composer
  controls are 46dp.
- Every icon-only control has a content description (`Conversation history`,
  `Settings`, `Choose model`, `Attach a file, image or tool`, `Scroll to latest
  message`, bottom destinations).
- Text uses the platform font scale; no fixed pixel text sizes.
- Both themes were checked for contrast in the palette above; muted text remains
  above a 4.5:1 target on panels.

## Screenshots

See `MOBILE_RESPONSIVE_TEST_REPORT.md` for captured dark/light screenshots and
device sizes.
