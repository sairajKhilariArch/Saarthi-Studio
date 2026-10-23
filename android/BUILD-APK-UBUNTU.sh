#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
if ! command -v java >/dev/null 2>&1; then
  echo "Java not found. Install JDK 17 or use Android Studio." >&2; exit 1
fi
if ! command -v gradle >/dev/null 2>&1; then
  echo "Gradle not found. Open this folder in Android Studio or install Gradle 8.9." >&2; exit 2
fi
if [[ -z "${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}" ]]; then
  echo "ANDROID_HOME / ANDROID_SDK_ROOT is not set. Install Android SDK Platform 35 first." >&2; exit 3
fi
unit_status=0; lint_status=0; assemble_status=0; release_status=0
if gradle --no-daemon testDebugUnitTest; then unit_status=0; else unit_status=$?; fi
if gradle --no-daemon lintDebug; then lint_status=0; else lint_status=$?; fi
if gradle --no-daemon assembleDebug; then assemble_status=0; else assemble_status=$?; fi
if gradle --no-daemon assembleRelease bundleRelease; then release_status=0; else release_status=$?; fi
apk="$(pwd)/app/build/outputs/apk/debug/app-debug.apk"
if [[ "$assemble_status" -eq 0 && -f "$apk" ]]; then
  echo "Debug APK created: $apk"
else
  echo "Debug APK was not produced." >&2
fi
release_apk="$(pwd)/app/build/outputs/apk/release/app-release-unsigned.apk"
aab="$(pwd)/app/build/outputs/bundle/release/app-release.aab"
if [[ "$release_status" -eq 0 && -f "$release_apk" && -f "$aab" ]]; then
  echo "Release APK (unsigned): $release_apk"
  echo "Release AAB (unsigned): $aab"
else
  echo "Release APK/AAB were not produced." >&2
fi
if [[ "$unit_status" -ne 0 || "$lint_status" -ne 0 || "$assemble_status" -ne 0 || "$release_status" -ne 0 ]]; then
  echo "Validation failed: unit tests=$unit_status, lint=$lint_status, assemble=$assemble_status, release=$release_status." >&2
  exit 4
fi
echo "All local build checks passed (debug and R8 release). Physical-device upgrade/network/media testing is still required."
