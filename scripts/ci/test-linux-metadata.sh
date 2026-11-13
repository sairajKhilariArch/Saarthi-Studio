#!/usr/bin/env bash
# Offline regression test for scripts/ci/verify-linux-metadata.sh.
#
# The validator exists because the packaged AppStream metadata advertised
# 1.26.6 while the app was 3.31.0: valid XML, so appstreamcli accepted it, and
# the .deb told software centres the wrong version. Each case below breaks one
# cross-check in a fixture copy and expects the validator to name it.
#
# Usage: scripts/ci/test-linux-metadata.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
tool="$here/verify-linux-metadata.sh"
id="io.github.sairajkhilariarch.SarthiStudio"
appdata="$root/app/packaging/$id.appdata.xml"
desktop="$root/app/packaging/$id.desktop"
version="$(bash "$here/version.sh" "$root/app/Cargo.toml")"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() { failed=$((failed + 1)); printf 'FAIL %s\n' "$1" >&2; }

out="$tmp/out"
err="$tmp/err"

accept() { # label, then the command
  local label="$1"
  shift
  if "$@" >"$out" 2>"$err"; then
    pass "$label"
  else
    die "$label ($(cat "$err"))"
  fi
}

refuse() { # label, expected message, then the command
  local label="$1" message="$2"
  shift 2
  if "$@" >"$out" 2>"$err"; then
    die "$label (the validator accepted it)"
    return
  fi
  if grep -qF "$message" "$err"; then
    pass "$label"
  else
    die "$label (the error did not mention \"$message\": $(cat "$err"))"
  fi
}

# --- the committed files ------------------------------------------------------
accept "the committed metadata matches this build" \
  bash "$tool" "$appdata" "$desktop" "$version"

# --- version drift ------------------------------------------------------------
refuse "a stale <release> version is refused" "newest <release>" \
  bash "$tool" "$appdata" "$desktop" 9.9.9

# --- component identity -------------------------------------------------------
bad="$tmp/bad-id.appdata.xml"
sed "s|<id>$id</id>|<id>com.example.Other</id>|" "$appdata" >"$bad"
refuse "a component id that does not match the file name is refused" "component id" \
  bash "$tool" "$bad" "$desktop" "$version"

launch="$tmp/launch.appdata.xml"
sed 's|SarthiStudio.desktop|Other.desktop|' "$appdata" >"$launch"
refuse "a launchable that names another desktop file is refused" "launchable" \
  bash "$tool" "$launch" "$desktop" "$version"

summary="$tmp/summary.appdata.xml"
sed 's|<summary>.*</summary>||' "$appdata" >"$summary"
refuse "a missing summary is refused" "no <summary>" \
  bash "$tool" "$summary" "$desktop" "$version"

malformed="$tmp/malformed.appdata.xml"
head -n 5 "$appdata" >"$malformed"
refuse "a malformed AppStream file is refused" "well-formed" \
  bash "$tool" "$malformed" "$desktop" "$version"

# --- the desktop entry --------------------------------------------------------
noexec="$tmp/noexec.desktop"
grep -v '^Exec=' "$desktop" >"$noexec"
refuse "a desktop entry without Exec is refused" "no Exec" \
  bash "$tool" "$appdata" "$noexec" "$version"

categories="$tmp/categories.desktop"
sed 's|Categories=Development;|Categories=Development|' "$desktop" >"$categories"
refuse "a Categories entry without its semicolon is refused" "semicolon" \
  bash "$tool" "$appdata" "$categories" "$version"

otherbinary="$tmp/other.desktop"
sed 's|Exec=free-ai-studio|Exec=something-else|' "$desktop" >"$otherbinary"
refuse "an Exec that runs another binary is refused" "but the component provides" \
  bash "$tool" "$appdata" "$otherbinary" "$version"

printf '\n%d passed, %d failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
