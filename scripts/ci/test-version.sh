#!/usr/bin/env bash
# Regression tests for the release pipeline's version parsing and artifact
# naming. These failures shipped twice (v1.26.0, v1.26.1) because the pipeline
# had no test at all, so run this on every push, not only on a tag.
#
# Usage: scripts/ci/test-version.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$here/version.sh"
gate="$here/verify-artifacts.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() {
  failed=$((failed + 1))
  printf 'FAIL %s\n' "$1" >&2
}

expect_version() { # description, manifest, expected
  local got
  if got="$(bash "$tool" "$2" 2>/dev/null)"; then
    if [ "$got" = "$3" ]; then
      pass "$1"
    else
      die "$1 (expected '$3', got '$got')"
    fi
  else
    die "$1 (expected success, tool failed)"
  fi
}

expect_failure() { # description, manifest
  if bash "$tool" "$2" >/dev/null 2>&1; then
    die "$1 (expected failure, tool succeeded)"
  else
    pass "$1"
  fi
}

expect_name_ok() { # description, basename
  if bash "$tool" --check-name "$2" >/dev/null 2>&1; then
    pass "$1"
  else
    die "$1 (rejected a valid name)"
  fi
}

expect_name_bad() { # description, basename
  if bash "$tool" --check-name "$2" >/dev/null 2>&1; then
    die "$1 (accepted an invalid name)"
  else
    pass "$1"
  fi
}

# --- manifests -------------------------------------------------------------
printf 'version = "1.26.4"\n' >"$tmp/lf.toml"
# The exact shape a CRLF checkout produced: the CR must never reach a filename.
printf 'version = "1.26.4"\r\n' >"$tmp/crlf.toml"
printf 'version = "1.26.4"   \n' >"$tmp/trailing.toml"
printf 'version = "1.26.4-rc.1"\n' >"$tmp/prerelease.toml"
printf 'version = ""\n' >"$tmp/empty.toml"
printf 'name = "not-a-version"\n' >"$tmp/missing.toml"
printf 'version = "1.26"\n' >"$tmp/short.toml"
printf 'version = "abc"\n' >"$tmp/garbage.toml"

expect_version "LF manifest" "$tmp/lf.toml" "1.26.4"
expect_version "CRLF manifest (v1.26.1 regression)" "$tmp/crlf.toml" "1.26.4"
expect_version "trailing whitespace" "$tmp/trailing.toml" "1.26.4"
expect_version "prerelease suffix" "$tmp/prerelease.toml" "1.26.4-rc.1"
expect_failure "empty version" "$tmp/empty.toml"
expect_failure "missing version line" "$tmp/missing.toml"
expect_failure "two-component version" "$tmp/short.toml"
expect_failure "non-numeric version" "$tmp/garbage.toml"
expect_failure "missing manifest file" "$tmp/does-not-exist.toml"

# --- artifact names --------------------------------------------------------
expect_name_ok "windows zip name" "sarthi-studio_1.26.4_windows-x86_64.zip"
expect_name_ok "deb name" "free-ai-studio_1.26.4-1_amd64.deb"
expect_name_ok "checksums file" "SHA256SUMS.txt"
expect_name_bad "name with CR" "$(printf 'sarthi-studio_1.26.4\r_windows-x86_64.zip')"
expect_name_bad "name with space" "free ai suite.zip"
expect_name_bad "empty name" ""
expect_name_bad "name with path separator" "dist/x.zip"

# --- artifact gate ---------------------------------------------------------
mkdir -p "$tmp/good"
printf 'payload\n' >"$tmp/good/sarthi-studio_1.26.4_windows-x86_64.zip"
(cd "$tmp/good" && sha256sum ./* >SHA256SUMS.txt)

if bash "$gate" "$tmp/good" >/dev/null 2>&1; then
  pass "artifact gate accepts a complete directory"
else
  die "artifact gate rejected a complete directory"
fi

# Corrupt a checksummed file: the gate must catch it before upload.
printf 'corrupted\n' >"$tmp/good/sarthi-studio_1.26.4_windows-x86_64.zip"
if bash "$gate" "$tmp/good" >/dev/null 2>&1; then
  die "artifact gate missed a checksum mismatch"
else
  pass "artifact gate detects a checksum mismatch"
fi

mkdir -p "$tmp/bad-name"
printf 'payload\n' >"$tmp/bad-name/with space.zip"
if bash "$gate" "$tmp/bad-name" >/dev/null 2>&1; then
  die "artifact gate accepted an unsafe filename"
else
  pass "artifact gate rejects an unsafe filename"
fi

mkdir -p "$tmp/empty"
if bash "$gate" "$tmp/empty" >/dev/null 2>&1; then
  die "artifact gate accepted an empty directory"
else
  pass "artifact gate rejects an empty directory"
fi

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ] || exit 1
