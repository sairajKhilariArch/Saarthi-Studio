#!/usr/bin/env bash
set -euo pipefail
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${1:-$HOME/Projects/Saarthi-Studio}"
if [[ ! -d "$REPO/.git" ]]; then
  echo "Could not find a Git repository at: $REPO" >&2
  echo "Usage: $0 /path/to/Saarthi-Studio" >&2
  exit 1
fi
if [[ -e "$REPO/android/app" ]]; then
  echo "An Android app folder already exists at $REPO/android/app; refusing to overwrite it." >&2
  echo "Back it up or remove/rename that folder and retry." >&2
  exit 2
fi
mkdir -p "$REPO/android" "$REPO/.github/workflows"
cp -a "$SRC/app" "$REPO/android/"
cp "$SRC/build.gradle.kts" "$SRC/settings.gradle.kts" "$SRC/gradle.properties" "$SRC/README.md" "$SRC/LICENSE" "$REPO/android/"
cp "$SRC/repo-workflow.yml" "$REPO/.github/workflows/build-android.yml"
echo "Android project copied into: $REPO/android"
echo "Build workflow installed at: $REPO/.github/workflows/build-android.yml"
echo
printf 'Next commands:\n  cd %q\n  git add android .github/workflows/build-android.yml\n  git commit -m "Add Saarthi Studio Android app and APK workflow"\n  git push origin main\n' "$REPO"
echo "After the push, open the repository's Actions tab and download the saarthi-studio-android-debug-apk artifact from the completed workflow run."
