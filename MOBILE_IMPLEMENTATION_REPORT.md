# Mobile Implementation Report

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`

## Summary

The Android client was redesigned in place. No demo app, static prototype or
separate mockup was created. All screens use the existing `MainActivity`,
`ChatDatabase`, `SecureSettings`, `DirectRouterTransport` and
`DesktopHostTransport`; new behaviour was added to those real paths.

## Files changed

| File | Change |
|---|---|
| `android/app/src/main/java/com/saarthi/studio/MainActivity.java` | Theme tokens and light palette, persisted theme, model chip and model picker, bottom navigation, More hub, full-screen panel framework, Projects/Skills/Workflows/Extensions/Memory screens, workflow run detail, chat frame with jump control, assistant surfaces, streaming thinking state, quick prompts, Appearance setting, About, host request helper |
| `android/app/src/main/res/values/styles.xml` | Indigo accent, light activity theme |
| `android/app/src/main/res/values-v27/styles.xml` | Removed orange accent, added API-27-safe light theme |
| `android/app/src/androidTest/java/com/saarthi/studio/MobileNavigationInstrumentedTest.java` | Navigation, model selector, composer and persisted light-theme tests |
| `app/src/host_api.rs` | Authenticated mobile-safe routes for projects, skills, workflows, runs, extensions and memory; capabilities flags; route/auth tests |
| `app/src/skills.rs` | Handler visibility for host reuse |
| `app/src/memory.rs` | Handler visibility for host reuse |
| `app/src/Cargo.toml`, `android/app/build.gradle.kts` | Version bump |

## Theme

`darkTheme` is read from `sarthi_theme` before `super.onCreate`, the matching
activity theme is applied with `setTheme`, then palette fields are set. Switching
the theme writes the preference and recreates the activity. The status/navigation
bar colours and light icon flags are applied at the same time.

## Chat

- Author label + bordered assistant surface.
- `SARTHI · THINKING` placeholder with dots; streamed tokens replace it and add
  a caret; final history rendering removes the caret.
- Floating jump-to-latest control driven by scroll distance.
- Model chip with live model id and a real discovery/picker flow for both
  Direct Router and Desktop Host.
- Empty state quick prompts insert editable text, focus the composer and show
  the keyboard.
- Existing streaming, stop, regenerate, retry, edit, copy, TTS, markdown, code
  highlighting and attachments were preserved.

## Navigation

- Bottom bar: Chat, Chats, Projects, Agents, More.
- Header: history and Settings icons.
- More provides Projects, Skills, Workflows, Extensions, Memory, Models,
  Appearance, Settings, presets, agents, backup, diagnostics and About.
- Every entry leads to a real screen or to the existing real settings functionality.

## Desktop feature screens

All use `GET`/`POST /api/host/v1/...` with the saved host token:

- **Projects:** list, create, activate; active project marked `ACTIVE`.
- **Skills:** list with version, source, compatibility, enabled state; toggle.
- **Workflows:** list with step count, manual/scheduled state, attached skill;
  Run button; recent runs with status and progress; Cancel live runs; full run
  step detail.
- **Extensions:** read-only inventory with ready/blocked state, version and
  runtime.
- **Memory:** list with kind/global scope, add and delete.

If no host is configured, these screens show a full-screen explanation and a
button to open Desktop Host settings. No sample projects, skills, workflows or
memories are fabricated.

## Safety and state

- Host tokens still come from the existing Keystore-encrypted settings.
- All new network I/O runs on the existing executor, never the UI thread.
- New forms validate names/titles before sending and surface server errors.
- Model selection falls back to an in-session change if encrypted persistence
  fails, with an explicit message.
- Deleting memory and cancelling a run require explicit confirmation where the
  action is destructive.

## Build and checks

See `MOBILE_TEST_REPORT.md`. Debug APK: `android/app/build/outputs/apk/debug/app-debug.apk`.
