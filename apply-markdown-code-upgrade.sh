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
  exit 2
fi
BRANCH="$(git -C "$TARGET_DIR" branch --show-current)"
if [[ "$BRANCH" != "android" ]]; then
  echo "ERROR: current branch is '$BRANCH', not 'android'."
  echo "Switch first: git -C '$TARGET_DIR' switch android"
  exit 2
fi

cp "$SOURCE_DIR/app/src/main/java/com/saarthi/studio/MainActivity.java" \
  "$TARGET_DIR/app/src/main/java/com/saarthi/studio/MainActivity.java"
cp "$SOURCE_DIR/app/build.gradle.kts" "$TARGET_DIR/app/build.gradle.kts"

printf '
Applied Saarthi Studio 1.2.0 Markdown/code-copy update to branch %s.\n' "$BRANCH"
printf 'Only MainActivity.java and app/build.gradle.kts were replaced; the workflow and database schema were left unchanged.\n'
printf '
Review and push with:
'
printf '  cd %q
' "$TARGET_DIR"
printf '  git diff --check
'
printf '  git diff -- app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
'
printf '  git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
'
printf '  git commit -m "Improve Markdown and code copying in Android chat"
'
printf '  git push origin android
'
