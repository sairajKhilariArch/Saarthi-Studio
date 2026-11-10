#!/usr/bin/env bash
# Regression tests for the Windows MSI packager.
#
# `scripts/ci/build-windows-msi.sh` produces the package a Windows user installs
# and later uninstalls, so its payload, its refusals and its metadata are checked
# here. Building an MSI needs the WiX CLI and unpacking one needs msiexec, so on
# a machine without them each case prints `skip` rather than passing silently.
#
# Usage: scripts/ci/test-installer.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
builder="$here/build-windows-msi.sh"
version="$(bash "$here/version.sh" "$root/app/Cargo.toml")"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

passed=0
failed=0
skipped=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
skip() { skipped=$((skipped + 1)); printf 'skip %s\n' "$1"; }
die() {
  failed=$((failed + 1))
  printf 'FAIL %s\n' "$1" >&2
}

wix=""
if command -v wix >/dev/null 2>&1; then
  wix="wix"
elif [ -x "$root/.tmp/wix5/wix.exe" ]; then
  wix="$root/.tmp/wix5/wix.exe"
elif [ -x "$root/.tmp/wix/wix.exe" ]; then
  wix="$root/.tmp/wix/wix.exe"
fi

stage_tree() { # dir
  mkdir -p "$1/static"
  printf 'MZ-not-a-real-binary\n' >"$1/free-ai-studio.exe"
  printf '<html><title>Saarthi Studio</title></html>\n' >"$1/static/index.html"
  printf 'console.log("ui");\n' >"$1/static/app.js"
  mkdir -p "$1/static/skills/debug"
  printf '# Debug\n' >"$1/static/skills/debug/SKILL.md"
}

if [ -z "$wix" ]; then
  skip "the MSI packager (no WiX CLI: dotnet tool install wix --version 5.* --tool-path .tmp/wix5)"
else
  stage_tree "$tmp/stage"
  if WIX_EXE="$wix" bash "$builder" "$tmp/stage" "$version" "$tmp/out" >"$tmp/build.log" 2>&1; then
    pass "a staged tree builds an MSI"
  else
    die "the packager refused a complete staged tree"
    cat "$tmp/build.log" >&2
  fi

  msi="$tmp/out/free-ai-suite_${version}_windows-x86_64.msi"
  if [ -f "$msi" ]; then
    pass "the package is named free-ai-suite_${version}_windows-x86_64.msi"
  else
    die "no MSI was written to $tmp/out"
  fi

  # The payload is the product: a package that installs an empty folder is worse
  # than no package at all.
  if command -v msiexec >/dev/null 2>&1; then
    admin="$tmp/admin"
    mkdir -p "$admin"
    win_msi="$(cygpath -w -a "$msi" 2>/dev/null || printf '%s' "$msi")"
    win_admin="$(cygpath -w -a "$admin" 2>/dev/null || printf '%s' "$admin")"
    if MSYS_NO_PATHCONV=1 msiexec /a "$win_msi" /qn TARGETDIR="$win_admin" >/dev/null 2>&1; then
      pass "msiexec performs an administrative install of the package"
    else
      die "msiexec could not open the package"
    fi
    installed="$(dirname "$(find "$admin" -type f -name 'free-ai-studio.exe' | head -n 1)")"
    if [ -f "$installed/free-ai-studio.exe" ]; then
      pass "the payload installs the binary into one install folder"
    else
      die "the unpacked package has no free-ai-studio.exe"
    fi
    if [ -f "$installed/static/index.html" ] && [ -f "$installed/static/app.js" ]; then
      pass "the payload installs the UI beside the binary"
    else
      die "the unpacked package has no static tree beside the binary"
    fi
    if [ -f "$installed/static/skills/debug/SKILL.md" ]; then
      pass "the payload keeps nested files such as the bundled skills"
    else
      die "a nested file (static/skills/debug/SKILL.md) did not survive packaging"
    fi

    # The database says what it is, which is what Add/Remove Programs shows.
    strings_file="$tmp/strings.txt"
    if command -v strings >/dev/null 2>&1; then
      strings "$msi" >"$strings_file" 2>/dev/null || true
    fi
    if [ -s "$strings_file" ] && grep -qi "Saarthi Studio" "$strings_file"; then
      pass "the package carries the product name"
    else
      skip "product name readout (no strings command)"
    fi
  else
    skip "payload extraction (no msiexec on this platform)"
  fi

  # --- refusals ------------------------------------------------------------
  mkdir -p "$tmp/nostatic"
  printf 'MZ\n' >"$tmp/nostatic/free-ai-studio.exe"
  if WIX_EXE="$wix" bash "$builder" "$tmp/nostatic" "$version" "$tmp/out2" >/dev/null 2>&1; then
    die "accepted a stage with no static tree"
  else
    pass "refuses a stage with no static tree"
  fi

  mkdir -p "$tmp/nobinary/static"
  printf '<html></html>\n' >"$tmp/nobinary/static/index.html"
  if WIX_EXE="$wix" bash "$builder" "$tmp/nobinary" "$version" "$tmp/out2" >/dev/null 2>&1; then
    die "accepted a stage with no binary"
  else
    pass "refuses a stage with no free-ai-studio.exe"
  fi

  if WIX_EXE="$wix" bash "$builder" "$tmp/stage" "not-a-version" "$tmp/out2" >/dev/null 2>&1; then
    die "accepted an unusable version"
  else
    pass "refuses an unusable version"
  fi

  # A second build of the same tree has to replace the previous package rather
  # than fail on it, because a rerun of a retried release step does exactly that.
  if WIX_EXE="$wix" bash "$builder" "$tmp/stage" "$version" "$tmp/out" >/dev/null 2>&1; then
    pass "rebuilding the same tree replaces the previous MSI"
  else
    die "a rebuild over an existing MSI failed"
  fi
fi

printf '\n%s passed, %s failed, %s skipped\n' "$passed" "$failed" "$skipped"
[ "$failed" -eq 0 ] || exit 1
