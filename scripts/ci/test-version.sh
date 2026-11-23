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
expect_name_ok "windows zip name" "SAARTHI-STUDIO_1.26.4_windows-x86_64.zip"
expect_name_ok "deb name" "SAARTHI-STUDIO_1.26.4-1_amd64.deb"
expect_name_ok "checksums file" "SHA256SUMS.txt"
expect_name_bad "name with CR" "$(printf 'SAARTHI-STUDIO_1.26.4\r_windows-x86_64.zip')"
expect_name_bad "name with space" "free ai suite.zip"
expect_name_bad "empty name" ""
expect_name_bad "name with path separator" "dist/x.zip"

# --- artifact gate ---------------------------------------------------------
mkdir -p "$tmp/good"
printf 'payload\n' >"$tmp/good/SAARTHI-STUDIO_1.26.4_windows-x86_64.zip"
(cd "$tmp/good" && sha256sum ./* >SHA256SUMS.txt)

if bash "$gate" "$tmp/good" >/dev/null 2>&1; then
  pass "artifact gate accepts a complete directory"
else
  die "artifact gate rejected a complete directory"
fi

# Corrupt a checksummed file: the gate must catch it before upload.
printf 'corrupted\n' >"$tmp/good/SAARTHI-STUDIO_1.26.4_windows-x86_64.zip"
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

# --- one version parser in the release workflow ------------------------------
# v1.26.0/v1.26.1 lost their assets to a per-job `sed` (a CR reached the filename)
# and the macOS job's copy of that regex was over-escaped. The shared parser is
# only a fix while every job actually calls it.
steps="$here/verify-version-steps.sh"
workflow="$here/../../.github/workflows/desktop-release.yml"
if bash "$steps" "$workflow" >"$tmp/steps.out" 2>&1; then
  pass "every release job reads the version through scripts/ci/version.sh"
else
  cat "$tmp/steps.out" >&2
  die "a release job reads the version with its own parser"
fi

# A job that grows its own reader again is caught, and the line is named.
sed 's|version="$(bash scripts/ci/version.sh app/Cargo.toml)"|version="$(sed -n '"'"'s/^version = "\\(.*\\)"/\\1/p'"'"' app/Cargo.toml)"|' \
  "$workflow" >"$tmp/inline.yml"
if bash "$steps" "$tmp/inline.yml" >"$tmp/inline.out" 2>&1; then
  die "the check accepted an inline version parser"
else
  grep -q 'inline `sed` version reader' "$tmp/inline.out" ||
    die "the inline reader was not named"
  pass "an inline version parser is caught and named"
fi

# A step that keeps `id: meta` but stops calling the parser is caught by name.
sed '0,/scripts\/ci\/version.sh app\/Cargo.toml/s//echo 9.9.9/' \
  "$workflow" >"$tmp/no-parser.yml"
if bash "$steps" "$tmp/no-parser.yml" >"$tmp/no-parser.out" 2>&1; then
  die "the check accepted a version step without the parser"
else
  grep -q 'does not call scripts/ci/version.sh' "$tmp/no-parser.out" ||
    die "the step that lost the parser was not named"
  grep -q 'Read and verify the version' "$tmp/no-parser.out" ||
    die "the failing step's name is missing"
  pass "a step that stops using the parser is caught and named"
fi

# --- a job must define the meta step it reads -------------------------------
# The deb job read steps.meta.outputs.version without an `id: meta` step; the
# expression resolved to nothing and the artifact gate demanded a versionless
# filename after the package had already built (v3.74.1/v3.74.2 release runs).
contract="$here/verify-release-contract.sh"
if bash "$contract" "$workflow" >"$tmp/contract.out" 2>&1; then
  pass "the committed workflow holds the release contract"
else
  cat "$tmp/contract.out" >&2
  die "the committed workflow broke the release contract"
fi

sed '60,$ s/^        id: meta$/        id: notmeta/' "$workflow" >"$tmp/no-meta.yml"
if bash "$contract" "$tmp/no-meta.yml" >"$tmp/no-meta.out" 2>&1; then
  die "the contract check accepted a job without its meta step"
else
  grep -q "defines no 'id: meta' step" "$tmp/no-meta.out" ||
    die "the job without a meta step was not named"
  pass "a job that reads steps.meta.outputs without defining it is caught"
fi

# The Windows gate ran bash continuations under the job's pwsh default shell;
# the backslash reached the gate as an expected artifact name. A step that
# loses its `shell: bash` again must be caught before a tagged run.
grep -v '^        shell: bash$' "$workflow" >"$tmp/no-bash-shell.yml"
if bash "$contract" "$tmp/no-bash-shell.yml" >"$tmp/no-bash.out" 2>&1; then
  die "the contract check accepted bash continuations under a pwsh default"
else
  grep -q "without 'shell: bash'" "$tmp/no-bash.out" ||
    die "the step missing shell: bash was not named"
  pass "bash continuations under a pwsh default shell are caught"
fi

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ] || exit 1
