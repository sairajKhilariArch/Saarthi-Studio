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
  echo "Error: target is not the Saarthi Studio Android project root: $REPO_DIR" >&2
  exit 1
fi
if [[ ! -d "$REPO_DIR/.git" ]]; then
  echo "Error: target is not a Git repository: $REPO_DIR" >&2
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

echo "Saarthi Studio 1.5.0 Backup & Export source copied to: $REPO_DIR"
echo "Backup of the previous source saved at: $BACKUP_DIR"
echo
echo "Next commands:"
echo "  cd '$REPO_DIR'"
echo "  git diff --check"
echo "  git diff --stat"
echo "  git add $JAVA_REL $GRADLE_REL"
echo "  git commit -m 'Add chat backup and export'"
echo "  git push origin android"
echo
echo "Then run the Android APK workflow on branch android and download the artifact after it succeeds."
