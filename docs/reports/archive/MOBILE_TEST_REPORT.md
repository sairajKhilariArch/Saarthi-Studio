# Mobile Test Report

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`

## Test commands run

```sh
export JAVA_HOME=.../.android-tools/jdk-17.0.21.1+2
export ANDROID_HOME=.../.android-tools/sdk
export PATH=$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:/tmp/android-tools/gradle-8.9/bin:$PATH

cd android
gradle --no-daemon testDebugUnitTest lintDebug assembleDebug assembleDebugAndroidTest
```

Rust tests:

```sh
cd app
RUSTUP_TOOLCHAIN=stable RUSTFLAGS='' cargo test host_api::tests
```

## Results

### Rust Host API

- `cargo test host_api::tests` — **18 passed, 0 failed**.
- Includes auth rejection, allow-list, malformed body, oversized body, model list,
  conversation paging, agent runs and the new workspace surfaces (projects, skills,
  workflows, workflow runs, extensions, memory).

### Android unit tests

- `testDebugUnitTest` — **39 tests, 0 failures, 0 errors**.
- Existing policy, network, host and backup-domain tests continue to pass.

### Lint

- `lintDebug` — **0 errors**.
- One non-blocking warning remains in the report; the previous `NewApi` error for
  `windowLightNavigationBar` below API 27 was fixed by moving the attribute to
  `values-v27/styles.xml`.

### Android instrumentation tests

| Test | Status |
|---|---|
| `AppLaunchInstrumentedTest` | present; not re-run on a device yet |
| `BackupRestoreInstrumentedTest` | present; not re-run on a device yet |
| `ChatHistoryPagingInstrumentedTest` | present; not re-run on a device yet |
| `ConversationSearchInstrumentedTest` | present; not re-run on a device yet |
| `DatabaseMigrationInstrumentedTest` | present; not re-run on a device yet |
| `MobileNavigationInstrumentedTest` | added; not re-run on a device yet |

The new instrumentation test was compiled and packaged (`assembleDebugAndroidTest`
succeeded), but is not executed here because no emulator or device was available.
It exercises:

- existence of all five bottom destinations with the correct content descriptions;
- header history/settings actions and the model selector content description;
- persistence of a light-theme choice across `recreate()`.

Full execution is pending the emulator.

## What was not executed

- No device/emulator interaction test was run.
- No screenshot was captured yet; this report will be updated with evidence in
  `MOBILE_RESPONSIVE_TEST_REPORT.md`.
- The behaviour of streaming, attachments, TTS, dictation, agents, projects, skills,
  workflows, memory and the More hub was not end-to-end-proved on a running client.
  These features wire to real APIs, so their risk is mainly in network/config and
  not in fake data.
