#!/usr/bin/env bash
# Regression tests for the Android instrumentation wrapper using a fake adb.
# No emulator, Android SDK, or built APK is required to run this test.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
RUNNER_SCRIPT="${SCRIPT_DIR}/run-android-instrumentation.sh"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf -- "$TMP_DIR"' EXIT

WORKSPACE="${TMP_DIR}/workspace"
mkdir -p \
    "${TMP_DIR}/bin" \
    "${WORKSPACE}/app/build/outputs/apk/debug" \
    "${WORKSPACE}/app/build/outputs/apk/androidTest/debug"
printf 'fake apk\n' > "${WORKSPACE}/app/build/outputs/apk/debug/app-debug.apk"
printf 'fake instrumentation apk\n' > "${WORKSPACE}/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"

cat > "${TMP_DIR}/bin/adb" <<'FAKE_ADB'
#!/usr/bin/env bash
set -u
case "${1:-}" in
    devices)
        printf 'List of devices attached\nemulator-5554 device\n'
        exit 0
        ;;
    wait-for-device)
        exit 0
        ;;
    logcat)
        if [[ "${2:-}" == "-b" ]]; then
            printf 'I/TestRunner: fake diagnostic log\n'
        fi
        exit 0
        ;;
    install)
        if [[ "${FAKE_ADB_MODE:-pass}" == "install-fail" ]]; then
            printf 'Failure [INSTALL_FAILED_TEST_ONLY]\n' >&2
            exit 1
        fi
        printf 'Success\n'
        exit 0
        ;;
    shell)
        if [[ "${2:-}" == "am" && "${3:-}" == "instrument" ]]; then
            case "${FAKE_ADB_MODE:-pass}" in
                pass)
                    # -1 is normal completion; the wrapper must accept this.
                    printf 'INSTRUMENTATION_STATUS: numtests=2\n'
                    printf 'INSTRUMENTATION_STATUS: class=ExampleTest\n'
                    printf 'INSTRUMENTATION_STATUS_CODE: -1\n'
                    printf 'INSTRUMENTATION_STATUS_CODE: -1\n'
                    printf 'INSTRUMENTATION_CODE: -1\n'
                    exit 0
                    ;;
                fail)
                    printf 'INSTRUMENTATION_STATUS: numtests=2\n'
                    printf 'INSTRUMENTATION_STATUS_CODE: -2\n'
                    printf 'INSTRUMENTATION_CODE: -1\n'
                    exit 0
                    ;;
                crash)
                    printf 'INSTRUMENTATION_RESULT: shortMsg=Process crashed.\n'
                    printf 'INSTRUMENTATION_FAILED: com.saarthi.studio.test/androidx.test.runner.AndroidJUnitRunner\n'
                    exit 0
                    ;;
                no-tests)
                    printf 'INSTRUMENTATION_STATUS: numtests=0\n'
                    printf 'INSTRUMENTATION_CODE: -1\n'
                    exit 0
                    ;;
                incomplete)
                    printf 'INSTRUMENTATION_STATUS: numtests=2\n'
                    printf 'INSTRUMENTATION_STATUS_CODE: -1\n'
                    exit 0
                    ;;
                runner-exit)
                    printf 'INSTRUMENTATION_STATUS: numtests=2\n'
                    printf 'INSTRUMENTATION_STATUS_CODE: -1\n'
                    printf 'INSTRUMENTATION_CODE: -1\n'
                    exit 7
                    ;;
            esac
        fi
        exit 0
        ;;
esac
exit 0
FAKE_ADB
chmod +x "${TMP_DIR}/bin/adb"

run_case() {
    local mode="$1"
    local expected_exit="$2"
    local expected_summary="$3"
    local actual_exit
    local output_file="${TMP_DIR}/${mode}.output"

    set +e
    PATH="${TMP_DIR}/bin:${PATH}" \
        GITHUB_WORKSPACE="$WORKSPACE" \
        FAKE_ADB_MODE="$mode" \
        bash "$RUNNER_SCRIPT" > "$output_file" 2>&1
    actual_exit=$?
    set -e

    if [[ "$actual_exit" -ne "$expected_exit" ]]; then
        printf 'FAIL: mode=%s expected exit=%s, received=%s\n' "$mode" "$expected_exit" "$actual_exit" >&2
        cat "$output_file" >&2
        return 1
    fi

    if ! grep -Fq "$expected_summary" "${WORKSPACE}/ci-diagnostics/test-summary.txt"; then
        printf 'FAIL: mode=%s did not produce expected summary: %s\n' "$mode" "$expected_summary" >&2
        cat "${WORKSPACE}/ci-diagnostics/test-summary.txt" >&2
        cat "$output_file" >&2
        return 1
    fi

    printf 'PASS: %-12s exit=%s\n' "$mode" "$actual_exit"
}

run_case pass 0 'PASS: AndroidJUnitRunner completed 2 test(s)'
run_case fail 1 'reported a test failure'
run_case crash 1 'reported a test failure'
run_case no-tests 1 'did not report a positive test count'
run_case incomplete 1 'did not report normal completion'
run_case runner-exit 1 'instrumentation command exited 7'
run_case install-fail 1 'APK installation failed'

printf 'All instrumentation-wrapper regression cases passed.\n'
