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
  echo "ERROR: expected Android Gradle project at repository root; Android app files were not found."
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

cat <<MSG

Applied Saarthi Studio 1.3.0 model-picker update to branch '$BRANCH'.
Only MainActivity.java and app/build.gradle.kts were replaced.
The database schema, saved conversation data, encrypted settings, and workflow were not changed.

Review and push with:
  cd "$TARGET_DIR"
  git diff --check
  git diff -- app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
  git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
  git commit -m "Add FreeLLMAPI model picker"
  git push origin android
MSG
