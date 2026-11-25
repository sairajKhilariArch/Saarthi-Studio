#!/usr/bin/env bash
# Verify SHA256SUMS.txt covers every non-checksum file and matches its bytes.
#
# Usage: scripts/ci/verify-checksums.sh <dist-dir>

set -euo pipefail

dist="${1:-}"
[ -n "$dist" ] && [ -d "$dist" ] || { echo "usage: scripts/ci/verify-checksums.sh <dist-dir>" >&2; exit 2; }
[ -f "$dist/SHA256SUMS.txt" ] || { echo "ERROR: no SHA256SUMS.txt in $dist" >&2; exit 1; }

if command -v sha256sum >/dev/null 2>&1; then
  check="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  check="shasum -a 256"
else
  echo "ERROR: no sha256sum or shasum available" >&2
  exit 1
fi

listed="$(mktemp)"
actual="$(mktemp)"
trap 'rm -f "$listed" "$actual"' EXIT
sed -E 's/^[0-9a-fA-F]{64}[[:space:]]+\*?//; s|^\./||' "$dist/SHA256SUMS.txt" | sort >"$listed"
( cd "$dist" && find . -maxdepth 1 -type f ! -name SHA256SUMS.txt -exec basename {} \; | sort ) >"$actual"

if ! diff -u "$listed" "$actual" >/dev/null; then
  echo "ERROR: SHA256SUMS.txt does not list exactly the release files" >&2
  diff -u "$listed" "$actual" >&2 || true
  exit 1
fi

( cd "$dist" && $check -c SHA256SUMS.txt )
echo "ok   SHA256SUMS.txt covers and matches $(wc -l <"$actual" | tr -d ' ') files"
