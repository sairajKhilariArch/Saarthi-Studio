#!/usr/bin/env bash
# Gate an artifact directory before it is uploaded or attached to a release.
#
# Checks: at least one artifact exists, every basename is safe (no control
# characters, no spaces, restricted charset), and SHA256SUMS.txt verifies when
# the directory publishes one. Runs on ubuntu, macOS and Windows (Git Bash),
# so it only uses POSIX tools plus bash builtins.
#
# Usage: scripts/ci/verify-artifacts.sh [dist-dir] [expected-artifact ...]
#
# With expected names it also fails when one was not produced, naming every
# missing file: a job that silently builds nothing must not hand an empty or
# partial directory to upload-artifact or the release API.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dir="${1:-dist}"
if [ "$#" -gt 0 ]; then shift; fi
expected=("$@")

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

# An expected name is a filename the job promised to produce; it is held to the
# same safety rules before it is used to build a path.
if [ "${#expected[@]}" -gt 0 ]; then
  for want in "${expected[@]}"; do
    if ! bash "$here/version.sh" --check-name "$want" >/dev/null 2>&1; then
      fail "unsafe expected artifact name: $(printf '%q' "$want")"
    fi
  done
  missing=0
  for want in "${expected[@]}"; do
    if [ ! -f "$dir/$want" ]; then
      printf 'ERROR: missing expected artifact: %s\n' "$want" >&2
      missing=$((missing + 1))
    fi
  done
  [ "$missing" -eq 0 ] || fail "$missing expected artifact(s) missing from $dir"
fi

if [ -f "$dir/SHA256SUMS.txt" ]; then
  (cd "$dir" && sha256sum -c SHA256SUMS.txt) || fail "checksum verification failed in $dir"
fi

if [ "${#expected[@]}" -gt 0 ]; then
  printf 'artifact gate OK: %s file(s) in %s, all %s expected present\n' \
    "$count" "$dir" "${#expected[@]}"
else
  printf 'artifact gate OK: %s file(s) in %s\n' "$count" "$dir"
fi
