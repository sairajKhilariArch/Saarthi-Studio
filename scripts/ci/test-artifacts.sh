#!/usr/bin/env bash
# Offline regression test for scripts/ci/verify-artifacts.sh.
#
# A release job that builds nothing and uploads the empty directory looked like
# a success; so did one that built only half of its promised files. The gate now
# takes the expected names and fails on each missing one, which this test drives
# against fixture directories with no network, no token and no packaging tools.
#
# Usage: scripts/ci/test-artifacts.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$here/verify-artifacts.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() { failed=$((failed + 1)); printf 'FAIL %s\n' "$1" >&2; }

out="$tmp/out"
err="$tmp/err"

# Runs the gate and expects it to fail; leaves its output in $out/$err.
refuse() { # label, then the command
  local label="$1"
  shift
  if "$@" >"$out" 2>"$err"; then
    die "$label (the gate accepted it)"
  else
    pass "$label"
  fi
}

accept() { # label, then the command
  local label="$1"
  shift
  if "$@" >"$out" 2>"$err"; then
    pass "$label"
  else
    die "$label ($(cat "$err"))"
  fi
}

# --- a complete directory -----------------------------------------------------
dist="$tmp/dist"
mkdir -p "$dist"
printf 'binary\n' >"$dist/sarthi-studio_9.9.9_x86_64.AppImage"
printf 'tarball\n' >"$dist/sarthi-studio_9.9.9_linux-x86_64.tar.gz"

accept "two files pass without an expected list" bash "$tool" "$dist"
accept "every expected file present passes" \
  bash "$tool" "$dist" "sarthi-studio_9.9.9_x86_64.AppImage"
accept "extra files do not fail the gate" \
  bash "$tool" "$dist" "sarthi-studio_9.9.9_x86_64.AppImage" \
  "sarthi-studio_9.9.9_linux-x86_64.tar.gz"

# --- a missing expected file --------------------------------------------------
refuse "a missing expected file fails" \
  bash "$tool" "$dist" "sarthi-studio_9.9.9_x86_64.AppImage" "SHA256SUMS.txt"
if grep -q "missing expected artifact: SHA256SUMS.txt" "$err"; then
  pass "the missing file is named in the error"
else
  die "the missing file was not named ($(cat "$err"))"
fi
if grep -q "1 expected artifact(s) missing" "$err"; then
  pass "the failure counts what is missing"
else
  die "the failure did not count the missing files"
fi

# --- an unsafe expected name --------------------------------------------------
refuse "an expected name with a space is refused" \
  bash "$tool" "$dist" "sarthi studio.tar.gz"

# --- an empty directory -------------------------------------------------------
empty="$tmp/empty"
mkdir -p "$empty"
refuse "an empty directory is refused" bash "$tool" "$empty"

# --- checksums ----------------------------------------------------------------
sums="$tmp/sums"
mkdir -p "$sums"
printf 'payload\n' >"$sums/a.tar.gz"
(
  cd "$sums"
  sha256sum a.tar.gz >SHA256SUMS.txt
)
accept "a matching SHA256SUMS.txt passes" bash "$tool" "$sums"

printf 'tampered\n' >"$sums/a.tar.gz"
refuse "a tampered file fails its checksum" bash "$tool" "$sums"

# --- the shipped release names -------------------------------------------------
# The names every release job promises; a typo in one of them is the defect this
# test exists for, and the gate must be able to check all of them at once.
names="$tmp/names"
mkdir -p "$names"
all=(
  "free-ai-studio-android.aab"
  "free-ai-studio-android.apk"
  "free-ai-studio-vscode.vsix"
  "free-ai-studio_9.9.9-1_amd64.deb"
  "sarthi-studio_9.9.9_linux-x86_64.tar.gz"
  "sarthi-studio_9.9.9_macos-universal.app.zip"
  "sarthi-studio_9.9.9_macos-universal.tar.gz"
  "sarthi-studio_9.9.9_windows-x86_64.msi"
  "sarthi-studio_9.9.9_windows-x86_64.zip"
  "sarthi-studio_9.9.9_x86_64.AppImage"
)
for name in "${all[@]}"; do
  printf 'artifact\n' >"$names/$name"
done
# A real checksum file, so the gate is exercised on a directory shaped exactly
# like the one the Linux packaging job publishes.
(
  cd "$names"
  sha256sum ./* >SHA256SUMS.txt
)
all+=("SHA256SUMS.txt")
accept "the full release set passes" bash "$tool" "$names" "${all[@]}"
rm "$names/free-ai-studio-vscode.vsix"
refuse "dropping one release file fails" bash "$tool" "$names" "${all[@]}"

printf '\n%d passed, %d failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
