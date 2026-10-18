#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 /path/to/Saarthi-Studio-repository"
  exit 2
fi
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(realpath "$1")"
JAVA_REL="app/src/main/java/com/saarthi/studio/MainActivity.java"
GRADLE_REL="app/build.gradle.kts"

if [[ ! -f "$REPO_DIR/$JAVA_REL" || ! -f "$REPO_DIR/$GRADLE_REL" ]]; then
  echo "Error: this does not look like the Saarthi Studio Android project root: $REPO_DIR" >&2
  echo "Expected $JAVA_REL and $GRADLE_REL" >&2
  exit 1
fi
if [[ ! -d "$REPO_DIR/.git" ]]; then
  echo "Error: target path is not a Git repository: $REPO_DIR" >&2
  exit 1
fi
BRANCH="$(git -C "$REPO_DIR" branch --show-current)"
if [[ "$BRANCH" != "android" ]]; then
  echo "Error: switch to the android branch first (current branch: $BRANCH)." >&2
  echo "Run: git -C '$REPO_DIR' switch android" >&2
  exit 1
fi

BACKUP_DIR="$HOME/.local/share/saarthi-studio-upgrade-backups/$(basename "$REPO_DIR")-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$BACKUP_DIR"
cp "$REPO_DIR/$JAVA_REL" "$BACKUP_DIR/MainActivity.java"
cp "$REPO_DIR/$GRADLE_REL" "$BACKUP_DIR/app-build.gradle.kts"
cp "$SOURCE_DIR/app/src/main/java/com/saarthi/studio/MainActivity.java" "$REPO_DIR/$JAVA_REL"
cp "$SOURCE_DIR/app/build.gradle.kts" "$REPO_DIR/$GRADLE_REL"

echo "Usage & Diagnostics source copied to: $REPO_DIR"
echo "Backup saved at: $BACKUP_DIR"
echo
echo "Next commands:"
echo "  cd '$REPO_DIR'"
echo "  git diff --check"
echo "  git diff --stat"
echo "  git add $JAVA_REL $GRADLE_REL"
echo "  git commit -m 'Add usage and diagnostics dashboard'"
echo "  git push origin android"
echo
echo "Then open GitHub Actions, run the Android APK workflow on branch android, and download the APK artifact after the build succeeds."
