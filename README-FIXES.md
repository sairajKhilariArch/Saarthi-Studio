# Saarthi Studio CI repair notes — follow-up

This patch is intended for the `android` branch.

## New evidence from run #17

Run 17: https://github.com/sairajKhilariArch/Saarthi-Studio/actions/runs/39144492386

- JVM tests, lint, the app APK, and the instrumentation APK all built successfully.
- The whitespace guard failed on `VALIDATION-REPORT.md:1`, where the heading had a trailing space.
- The instrumentation result contained `INSTRUMENTATION_RESULT: shortMsg=Process crashed`, `INSTRUMENTATION_CODE: 0`, and zero tests. No test assertion actually ran, so this is not evidence that the new composer assertion failed.
- The uploaded diagnostics artifact did not contain `ci-diagnostics/logcat.txt`. The prior workflow used relative paths inside the emulator action. This follow-up writes to `$GITHUB_WORKSPACE/ci-diagnostics` explicitly.

## Files in this bundle

1. `.github/workflows/build-android.yml`
   - Keeps JVM tests, lint, both APK build stages, whitespace validation, and a strict final gate.
   - Installs the already-built app and instrumentation APKs, then launches `AndroidJUnitRunner` with `adb shell am instrument -w -r` rather than the Gradle UTP wrapper that crashed before reporting tests in runs 16 and 17.
   - Parses `INSTRUMENTATION_STATUS_CODE` error/failure markers and requires the runner to report a nonzero test count plus normal completion. A zero-test or crashed run remains a failure.
   - Captures instrumentation output and logcat under the absolute workspace path before emulator teardown, then uploads the diagnostics artifact if the test step fails.

2. `VALIDATION-REPORT.md`
   - Removes the trailing space from the first heading, the exact whitespace defect in run 17.

3. `app/src/androidTest/java/com/saarthi/studio/AppLaunchInstrumentedTest.java`
   - Preserves the already-updated composer-hint assertion. It is included for convenience; run 17 did not execute any tests, so this file is not being changed to guess at the process crash.

4. `CHANGELOG.md`
   - Documents the follow-up CI adjustments.

## Important verification note

I cannot claim the process crash is fixed until a new workflow run passes. The new instrumentation invocation should at least expose the runner's actual failure or the Android logcat crash trace. If the runner still exits before starting tests, download `saarthi-studio-android-failure-diagnostics` and inspect `instrumentation.log` and `logcat.txt`; do not disable the final gate.
