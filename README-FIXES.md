# Saarthi Studio CI repair notes

This patch is intended for the `android` branch.

## Confirmed failures in the recent Actions runs

- Run 14 failed the committed-whitespace guard. That stopped unit tests, lint, APK assembly, and instrumentation tests from running.
- Run 15 passed unit tests, lint and APK assembly, but `AppLaunchInstrumentedTest.activityLaunchesAndShowsHeaderAndComposer` failed with `Message composer should be visible`.
- Run 16 passed unit tests, lint and APK assembly, but the Android instrumentation process crashed before reporting a test (`Starting 0 tests`, followed by `Instrumentation run failed due to Process crashed`).
- The final validation gate is behaving as designed; do not remove it or force the status green.
- The current PR targets `main`, but the original `pull_request.branches` filter only included `android`. GitHub applies that filter to the PR base branch, so the PR build trigger would not run for a PR into `main`.

## Files

1. `.github/workflows/build-android.yml`
   - Keeps the strict final gate.
   - Builds both the app and instrumentation APK before starting the emulator.
   - Runs PR validation when the PR base is `main` or `android`.
   - Lets whitespace validation report failure without skipping the diagnostic build/test phases; the final gate still fails on whitespace errors.
   - Builds the app APK separately from the instrumentation APK, so a test-APK build problem does not prevent uploading a successfully assembled app APK.
   - Uses a clean API 35 emulator with snapshots disabled.
   - Captures logcat and emulator process/memory state inside the emulator action before it shuts down, then uploads test/log/crash artifacts when instrumentation fails.
   - Adds a workflow concurrency group to avoid overlapping builds for the same branch.

2. `app/src/androidTest/java/com/saarthi/studio/AppLaunchInstrumentedTest.java`
   - Tests the composer EditText's presence and `VISIBLE` state by its hint without depending on `View.isShown()`, which can be sensitive to attachment/window state during an instrumentation callback.
   - Keeps the header assertion.

3. `CHANGELOG.md`
   - Documents the CI corrections and removes the duplicated v1.11.1 stabilization heading. This is a documentation cleanup, not a prerequisite for the APK build.

## Before calling this fixed

- Run `./BUILD-APK-UBUNTU.sh` locally with JDK 17 and Android SDK 35 installed.
- Run the workflow on `android` and inspect any generated `saarthi-studio-android-failure-diagnostics` artifact.
- If instrumentation still crashes before tests start, inspect logcat and instrumentation test reports before changing app code. A process crash is not the same as a failed assertion.
- Perform a physical-device smoke test for app launch, endpoint configuration, chat send/stop, history, backup/restore, voice, and image attachment before release.

This patch has not itself been executed on a live emulator here; CI must confirm the result.
