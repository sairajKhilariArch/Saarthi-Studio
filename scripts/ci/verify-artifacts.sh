#!/usr/bin/env bash
# Gate an artifact directory before it is uploaded or attached to a release.
#
# Checks: at least one artifact exists, every basename is safe (no control
# characters, no spaces, restricted charset), and SHA256SUMS.txt verifies when
# the directory publishes one. Runs on ubuntu, macOS and Windows (Git Bash),
# so it only uses POSIX tools plus bash builtins.
#
# Usage: scripts/ci/verify-artifacts.sh [dist-dir]

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dir="${1:-dist}"

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

[ -d "$dir" ] || fail "artifact directory not found: $dir"

count=0
while IFS= read -r -d '' file; do
  base="$(basename "$file")"
  if ! bash "$here/version.sh" --check-name "$base" >/dev/null 2>&1; then
    fail "unsafe artifact filename in $dir: $(printf '%q' "$base")"
  fi
  count=$((count + 1))
done < <(find "$dir" -maxdepth 1 -type f -print0)

[ "$count" -gt 0 ] || fail "no artifacts found in $dir"

if [ -f "$dir/SHA256SUMS.txt" ]; then
  (cd "$dir" && sha256sum -c SHA256SUMS.txt) || fail "checksum verification failed in $dir"
fi

printf 'artifact gate OK: %s file(s) in %s\n' "$count" "$dir"
