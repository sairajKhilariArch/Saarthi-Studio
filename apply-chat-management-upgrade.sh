#!/usr/bin/env bash
set -euo pipefail

SOURCE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="${1:-}"

if [[ -z "$TARGET_DIR" ]]; then
  echo "Usage: $0 /path/to/Saarthi-Studio"
  echo "Example: $0 ~/Projects/Saarthi-Studio"
  exit 2
fi
TARGET_DIR="$(realpath "$TARGET_DIR")"

if [[ ! -d "$TARGET_DIR/.git" ]]; then
  echo "ERROR: '$TARGET_DIR' is not a Git working tree."
  exit 2
fi
if [[ ! -f "$TARGET_DIR/app/src/main/java/com/saarthi/studio/MainActivity.java" || ! -f "$TARGET_DIR/app/build.gradle.kts" ]]; then
  echo "ERROR: expected Android Gradle project at repository root; app source files were not found."
  echo "This script is intended for the Saarthi-Studio android branch layout with app/ at repository root."
  exit 2
fi
BRANCH="$(git -C "$TARGET_DIR" branch --show-current)"
if [[ "$BRANCH" != "android" ]]; then
  echo "ERROR: current branch is '$BRANCH', not 'android'."
  echo "Switch to android first: git -C '$TARGET_DIR' switch android"
  exit 2
fi

mkdir -p "$TARGET_DIR/.github/workflows"
cp "$SOURCE_DIR/app/src/main/java/com/saarthi/studio/MainActivity.java" \
  "$TARGET_DIR/app/src/main/java/com/saarthi/studio/MainActivity.java"
cp "$SOURCE_DIR/app/build.gradle.kts" "$TARGET_DIR/app/build.gradle.kts"
cp "$SOURCE_DIR/.github/workflows/build-android.yml" \
  "$TARGET_DIR/.github/workflows/build-android.yml"

printf '\nUpdated files on branch %s:\n' "$BRANCH"
git -C "$TARGET_DIR" status --short
printf '\nNext steps:\n'
printf '  cd %q\n' "$TARGET_DIR"
printf '  git diff --check\n'
printf '  git diff -- app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts .github/workflows/build-android.yml\n'
printf '  git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts .github/workflows/build-android.yml\n'
printf '  git commit -m "Add conversation management to Android app"\n'
printf '  git push origin android\n'
