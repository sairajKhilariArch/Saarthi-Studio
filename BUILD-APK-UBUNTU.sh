#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
if ! command -v java >/dev/null 2>&1; then
  echo "Java not found. Install JDK 17 or use Android Studio." >&2; exit 1
fi
if ! command -v gradle >/dev/null 2>&1; then
  echo "Gradle not found. Open this folder in Android Studio and choose Build > Build APK(s), or install a compatible Gradle version (8.9+)." >&2
  exit 2
fi
if [[ -z "${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}" ]]; then
  echo "ANDROID_HOME / ANDROID_SDK_ROOT is not set. Install Android SDK Platform 35 in Android Studio first." >&2; exit 3
fi
gradle assembleDebug
echo "APK created: $(pwd)/app/build/outputs/apk/debug/app-debug.apk"
