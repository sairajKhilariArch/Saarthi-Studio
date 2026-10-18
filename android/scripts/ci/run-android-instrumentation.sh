#!/usr/bin/env bash
# Run Android instrumentation tests in one shell process.
#
# reactivecircus/android-emulator-runner executes separate lines in its `script`
# input independently. Keep variables, exit codes, and diagnostic collection in
# this standalone script and invoke it from the workflow as a single command.

set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_PROJECT_DIR="${SAARTHI_ANDROID_PROJECT_DIR:-$(cd -- "${SCRIPT_DIR}/../.." && pwd)}"
WORKSPACE="${GITHUB_WORKSPACE:-$(cd -- "${ANDROID_PROJECT_DIR}/.." && pwd)}"
DIAG_DIR="${ANDROID_PROJECT_DIR}/ci-diagnostics"
APP_APK="${ANDROID_PROJECT_DIR}/app/build/outputs/apk/debug/app-debug.apk"
TEST_APK="${ANDROID_PROJECT_DIR}/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"
INSTRUMENTATION_LOG="${DIAG_DIR}/instrumentation.log"
RUNNER="com.saarthi.studio.test/androidx.test.runner.AndroidJUnitRunner"
TEST_EXIT=0

if ! mkdir -p -- "$DIAG_DIR"; then
    echo "Could not create instrumentation diagnostics directory: ${DIAG_DIR}" >&2
    exit 2
fi

{
    printf 'Workspace: %s\n' "$WORKSPACE"
    printf 'Working directory: %s\n' "$PWD"
    printf 'Runner: %s\n' "$RUNNER"
    printf 'Started at (UTC): %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} > "${DIAG_DIR}/environment.txt"

write_summary() {
    printf '%s\n' "$1" | tee "${DIAG_DIR}/test-summary.txt"
}

# Fail with a clear explanation if build outputs or adb are missing.
if [[ ! -s "$APP_APK" ]]; then
    write_summary "FAIL: debug APK is missing or empty: ${APP_APK}"
    TEST_EXIT=1
elif [[ ! -s "$TEST_APK" ]]; then
    write_summary "FAIL: instrumentation APK is missing or empty: ${TEST_APK}"
    TEST_EXIT=1
elif ! command -v adb >/dev/null 2>&1; then
    write_summary "FAIL: adb is not available on PATH."
    TEST_EXIT=1
else
    # The emulator action normally waits for boot, but keep an explicit device
    # readiness check and capture its output for diagnosis if that assumption fails.
    adb devices -l > "${DIAG_DIR}/adb-devices.txt" 2>&1 || true
    timeout --signal=TERM --kill-after=5s 90s adb wait-for-device > "${DIAG_DIR}/adb-wait-for-device.log" 2>&1
    ADB_READY_EXIT=$?

    if [[ "$ADB_READY_EXIT" -ne 0 ]]; then
        write_summary "FAIL: adb could not connect to the emulator (exit ${ADB_READY_EXIT})."
        TEST_EXIT=1
    else
        # Keep the captured log focused on this instrumentation run.
        adb logcat -c > "${DIAG_DIR}/logcat-clear.log" 2>&1 || true

        adb install -r "$APP_APK" > "${DIAG_DIR}/install-app.log" 2>&1
        APP_INSTALL_EXIT=$?
        adb install -r -t "$TEST_APK" > "${DIAG_DIR}/install-tests.log" 2>&1
        TEST_INSTALL_EXIT=$?

        cat "${DIAG_DIR}/install-app.log"
        cat "${DIAG_DIR}/install-tests.log"

        if [[ "$APP_INSTALL_EXIT" -ne 0 || "$TEST_INSTALL_EXIT" -ne 0 ]]; then
            {
                printf 'FAIL: APK installation failed (app exit=%s, tests exit=%s).\n' \
                    "$APP_INSTALL_EXIT" "$TEST_INSTALL_EXIT"
                printf 'See install-app.log and install-tests.log in this artifact.\n'
            } | tee "${DIAG_DIR}/test-summary.txt"
            TEST_EXIT=1
        else
            # The suite is small; cap a hung instrumentation invocation at 15 minutes.
            # GNU timeout is available on the ubuntu-latest runner.
            timeout --signal=TERM --kill-after=30s 15m \
                adb shell am instrument -w -r "$RUNNER" \
                > "$INSTRUMENTATION_LOG" 2>&1
            INSTRUMENT_EXIT=$?
            cat "$INSTRUMENTATION_LOG"

            # AndroidJUnitRunner emits -1 for a normally finished test. Only -2
            # is a failed test; matching -1 here would mark every passing suite
            # as failed. Also require a non-zero test count and normal run finish.
            NUM_TESTS="$(sed -nE \
                's/^[[:space:]]*INSTRUMENTATION_STATUS: numtests=([0-9]+)[[:space:]]*$/\1/p' \
                "$INSTRUMENTATION_LOG" | tail -n 1 | tr -d '\r')"

            if [[ "$INSTRUMENT_EXIT" -ne 0 ]]; then
                write_summary "FAIL: instrumentation command exited ${INSTRUMENT_EXIT} (124 usually means the 15-minute timeout)."
                TEST_EXIT=1
            elif grep -Eq \
                '^INSTRUMENTATION_STATUS_CODE: -2[[:space:]]*$|^INSTRUMENTATION_RESULT: shortMsg=|^INSTRUMENTATION_FAILED|^FAILURES!!!' \
                "$INSTRUMENTATION_LOG"; then
                write_summary "FAIL: AndroidJUnitRunner reported a test failure, runner error, or crash."
                TEST_EXIT=1
            elif [[ ! "$NUM_TESTS" =~ ^[1-9][0-9]*$ ]]; then
                write_summary "FAIL: AndroidJUnitRunner did not report a positive test count (reported: ${NUM_TESTS:-none})."
                TEST_EXIT=1
            elif ! grep -Eq '^INSTRUMENTATION_CODE: -1[[:space:]]*$' "$INSTRUMENTATION_LOG"; then
                write_summary "FAIL: AndroidJUnitRunner did not report normal completion."
                TEST_EXIT=1
            else
                write_summary "PASS: AndroidJUnitRunner completed ${NUM_TESTS} test(s) with no failure markers."
            fi
        fi
    fi
fi

# Always gather diagnostics where adb is available, including installation and
# instrumentation failures. These files are uploaded by the workflow on failure.
if command -v adb >/dev/null 2>&1; then
    adb logcat -b all -d -v threadtime > "${DIAG_DIR}/logcat.txt" 2>&1 || true
    adb shell dumpsys activity processes > "${DIAG_DIR}/activity-processes.txt" 2>&1 || true
    adb shell dumpsys meminfo > "${DIAG_DIR}/meminfo.txt" 2>&1 || true
    adb shell getprop > "${DIAG_DIR}/getprop.txt" 2>&1 || true
fi

if [[ -d "${ANDROID_PROJECT_DIR}/app/build/outputs" ]]; then
    find "${ANDROID_PROJECT_DIR}/app/build/outputs" -maxdepth 7 -type f \
        \( -name '*.xml' -o -name '*.txt' -o -name '*.log' \) \
        -print -exec cp --parents {} "$DIAG_DIR/" \; 2>/dev/null || true
fi

echo 'Recent Android runtime errors:'
if [[ -f "${DIAG_DIR}/logcat.txt" ]]; then
    grep -Ei \
        'FATAL EXCEPTION|AndroidRuntime|INSTRUMENTATION_FAILED|Process: com\.saarthi|Fatal signal|Permission Denial|lowmemorykiller' \
        "${DIAG_DIR}/logcat.txt" | tail -n 200 || true
fi

echo 'Diagnostic files captured:'
find "$DIAG_DIR" -maxdepth 8 -type f -printf '%p (%s bytes)\n' 2>/dev/null || true
printf 'Finished at (UTC): %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" >> "${DIAG_DIR}/environment.txt"
exit "$TEST_EXIT"
