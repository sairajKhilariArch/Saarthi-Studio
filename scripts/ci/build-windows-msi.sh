#!/usr/bin/env bash
# Build the Windows MSI installer from the staged release tree.
#
# The MSI is what a Windows user expects in Add/Remove Programs: one product, a
# version, a Start Menu entry and an uninstall that removes what it installed.
# The payload is the same tree the portable zip ships, harvested by
# app/packaging/windows/installer.wxs, so this script only has to validate the
# input, pass the version and prove the archive it produced holds the payload.
#
# Usage: scripts/ci/build-windows-msi.sh <stage-dir> <version> <out-dir>
#   WIX_EXE   override the WiX CLI (default: `wix` on PATH, else .tmp/wix/wix.exe)
#
# Needs: the WiX CLI (`dotnet tool install wix --global` or --tool-path).
# Verifying the result needs msiexec, which ships with Windows; without it the
# build still succeeds and the payload check is reported as not run.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

stage="${1:-}"
version="${2:-}"
out="${3:-}"

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

[ -n "$stage" ] && [ -n "$version" ] && [ -n "$out" ] ||
  die "usage: scripts/ci/build-windows-msi.sh <stage-dir> <version> <out-dir>"
[ -d "$stage" ] || die "no such staged tree: $stage"
bash "$here/version.sh" --check-version "$version" ||
  die "unusable version '$version' (expected MAJOR.MINOR.PATCH)"
[ -f "$stage/free-ai-studio.exe" ] ||
  die "$stage has no free-ai-studio.exe, so there is nothing to install"
[ -f "$stage/static/index.html" ] ||
  die "$stage/static/index.html is missing, so the installed app would serve no UI"

wix="${WIX_EXE:-}"
if [ -z "$wix" ]; then
  if command -v wix >/dev/null 2>&1; then
    wix="wix"
  elif [ -x "$root/.tmp/wix5/wix.exe" ]; then
    wix="$root/.tmp/wix5/wix.exe"
  elif [ -x "$root/.tmp/wix/wix.exe" ]; then
    wix="$root/.tmp/wix/wix.exe"
  else
    die "no WiX CLI found: 'dotnet tool install wix --version 5.* --tool-path .tmp/wix5', or set WIX_EXE"
  fi
fi

# WiX resolves a bind path against the authoring file's directory, not the
# working directory, so the stage has to be absolute — as a Windows path, since
# wix.exe is a native program even when this script runs under Git Bash.
stage_abs="$(cd "$stage" && pwd)"
win_stage="$(cygpath -w "$stage_abs" 2>/dev/null || printf '%s' "$stage_abs")"

mkdir -p "$out"
msi="$out/free-ai-suite_${version}_windows-x86_64.msi"
rm -f "$msi"

# `-bindpath stage=<dir>` is what `!(bindpath.stage)` in the authoring resolves
# to; `-arch x64` keeps the package a single, predictable platform.
"$wix" build \
  -arch x64 \
  -d Version="$version" \
  -bindpath "stage=$win_stage" \
  -o "$msi" \
  "$root/app/packaging/windows/installer.wxs"

[ -f "$msi" ] || die "the WiX build reported success but wrote no MSI"
bytes="$(wc -c <"$msi" | tr -d ' ')"
[ "$bytes" -gt 4096 ] || die "the MSI is only $bytes bytes, so it carries no payload"

printf 'msi   %s\n' "$msi"
printf 'size  %s bytes\n' "$bytes"
printf 'scope perUser, %s, Start Menu shortcut, major upgrade\n' "$version"

# Prove the archive holds the payload before anyone installs it. `msiexec /a` is
# an administrative install: it unpacks the files without touching the machine.
if command -v msiexec >/dev/null 2>&1; then
  admin="$(mktemp -d)"
  win_msi="$(cygpath -w "$msi" 2>/dev/null || printf '%s' "$msi")"
  win_admin="$(cygpath -w "$admin" 2>/dev/null || printf '%s' "$admin")"
  # An administrative install never prompts; a failure here means the package
  # cannot be opened at all, which is worth knowing at build time. The verbose
  # log is kept beside it so a failure names the MSI error instead of "could not
  # unpack".
  log="$admin/msiexec.log"
  win_log="$(cygpath -w "$log" 2>/dev/null || printf '%s' "$log")"
  # Git Bash rewrites an argument that starts with `/` into a Windows path, which
  # makes msiexec answer 103 and write no log at all. `MSYS_NO_PATHCONV` keeps
  # `/a`, `/qn` and `/l*v` intact; they are switches, not paths.
  if ! MSYS_NO_PATHCONV=1 msiexec /a "$win_msi" /qn /l*v "$win_log" TARGETDIR="$win_admin" >/dev/null 2>&1; then
    if [ -f "$log" ]; then
      grep -E "Error|error|return value 3" "$log" | tail -n 12 >&2 || true
    fi
    rm -rf "$admin"
    die "msiexec could not unpack the package it just built"
  fi
  # An administrative image keeps the package's directory tree below TARGETDIR
  # (`LocalApp\\Programs\\Saarthi Studio`), so the payload is found by searching
  # rather than by assuming a fixed depth.
  if [ -z "$(find "$admin" -type f -name 'free-ai-studio.exe' | head -n 1)" ] ||
    [ -z "$(find "$admin" -type f -path '*static/index.html' | head -n 1)" ]; then
    ls -R "$admin" >&2 || true
    rm -rf "$admin"
    die "the MSI unpacked without the binary or the UI"
  fi
  files="$(find "$admin" -type f | wc -l | tr -d ' ')"
  rm -rf "$admin"
  printf 'payload verified: binary + UI, %s files\n' "$files"
else
  printf 'payload check skipped: no msiexec on this platform\n'
fi
