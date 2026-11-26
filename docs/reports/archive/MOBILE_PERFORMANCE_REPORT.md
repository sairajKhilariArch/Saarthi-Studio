# Mobile Performance Report

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`

## What was measured

Only the build pipeline, archive sizes and the architecture that bounds runtime
work. A device-level profiler is not available in this environment, so real
frame-rate, heap and startup timings are reported as **not measured**, not as
results.

## Build

Command: `gradle --no-daemon assembleDebug assembleDebugAndroidTest testDebugUnitTest lintDebug`
in `android/`, with JDK 17, Gradle 8.9 and the Android SDK at the repo-local
`.android-tools/sdk`.

| Step | Result |
|---|---|
| `testDebugUnitTest` | **39 tests, 0 failures, 0 errors** |
| `lintDebug` | **0 errors** (1 warning remains, stylistic) |
| `assembleDebug` | success |
| `assembleDebugAndroidTest` | success |

Debug APK: **252 KB** at `android/app/build/outputs/apk/debug/app-debug.apk`.
Instrumentation APK: **1.6 MB** at
`android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk`.

## What keeps the UI light

- One activity, one `ScrollView`-backed message list, no nested scrolling in
  normal chat.
- Pagination: conversations render at most `MESSAGE_PAGE_SIZE` pages, with a
  hard cap of `MAX_VISIBLE_MESSAGES` (1 500) visible rows.
- Long messages render as many separated views, so a 25 KB monolith does not
  inflate a single inflated tree the way one wrapped widget would.
- Host API representations are bounded: the capabilities and model summaries are
  capped by the Rust side before they reach the phone.
- All new network I/O is on the existing Java executor; the UI is never blocked on
  a project list, skills or workflows fetch.

## What was deliberately not changed

- The app still targets API 26 and the existing ripple/density/size approach.
- No list virtualization library was added; the existing offset replay model is
  reused.
- No new animation chain was introduced for message appearance, so the current
  “no animation on appearance” profile is unchanged (nothing was added here that
  could regress).
- Code-block scroll containers are stay-width; they never cause the whole chat to
  widen.

## What is not measured

- Cold-start time, view inflation cost and memory at first render.
- CPU / frame budget during streaming or during a long scroll.
- Background/foreground transitions, paused streaming lifecycle, and session
  expiry behaviour on a physical or emulated device.
- Network latency under actual flaky conditions.

These are realistic next steps once a real device or emulator is available; they
are listed as limitations here.
