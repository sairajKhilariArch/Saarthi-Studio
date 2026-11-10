#!/usr/bin/env bash
# Regression tests for the macOS application-bundle packager.
#
# `scripts/ci/bundle-macos-app.sh` produces the thing a macOS user actually
# double-clicks, so its layout, its Info.plist and its refusals are checked on
# every push instead of being discovered in a release run.
#
# Usage: scripts/ci/test-bundle.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bundler="$here/bundle-macos-app.sh"
root="$(cd "$here/../.." && pwd)"
version="$(bash "$here/version.sh" "$root/app/Cargo.toml")"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() {
  failed=$((failed + 1))
  printf 'FAIL %s\n' "$1" >&2
}

py=""
if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
fi

stage_tree() { # dir
  mkdir -p "$1/static"
  printf '#!/bin/sh\nexit 0\n' >"$1/free-ai-studio"
  chmod +x "$1/free-ai-studio"
  printf '<html><title>Saarthi Studio</title></html>\n' >"$1/static/index.html"
  printf 'console.log("ui");\n' >"$1/static/app.js"
}

# --- a complete stage builds the bundle ------------------------------------
stage_tree "$tmp/stage"
if bash "$bundler" "$tmp/stage" "$version" "$tmp/out" >"$tmp/bundle.log" 2>&1; then
  pass "a staged tree builds a bundle"
else
  die "the bundler refused a complete staged tree"
  cat "$tmp/bundle.log" >&2
fi

app="$tmp/out/Saarthi Studio.app"
plist="$app/Contents/Info.plist"

if [ -x "$app/Contents/MacOS/free-ai-studio" ]; then
  pass "Contents/MacOS holds the executable, still executable"
else
  die "Contents/MacOS/free-ai-studio is missing or not executable"
fi

if [ -f "$app/Contents/Resources/static/index.html" ] &&
  [ -f "$app/Contents/Resources/static/app.js" ]; then
  pass "Contents/Resources/static holds the whole UI"
else
  die "the UI was not copied into Contents/Resources/static"
fi

if [ "$(cat "$app/Contents/PkgInfo" 2>/dev/null)" = "APPL????" ]; then
  pass "PkgInfo marks the bundle as an application"
else
  die "PkgInfo is missing or says something else"
fi

missing=""
for key in CFBundleExecutable CFBundleIdentifier CFBundlePackageType \
  CFBundleShortVersionString CFBundleVersion CFBundleName; do
  grep -q "<key>$key</key>" "$plist" 2>/dev/null || missing="$missing $key"
done
if [ -z "$missing" ]; then
  pass "Info.plist declares the executable, identifier, type, name and versions"
else
  die "Info.plist is missing:$missing"
fi

if grep -q "<string>$version</string>" "$plist" &&
  grep -q "<string>free-ai-studio</string>" "$plist" &&
  grep -q "com.saarthi.freeaistudio" "$plist"; then
  pass "Info.plist carries the version and the bundled binary's name"
else
  die "Info.plist does not carry $version, free-ai-studio and the bundle id"
fi

zip_path="$tmp/out/free-ai-suite_${version}_macos-universal.app.zip"
if [ -f "$zip_path" ]; then
  pass "the bundle is archived as $(basename "$zip_path")"
else
  die "no bundle zip was written"
fi

if [ -n "$py" ]; then
  listing="$("$py" - "$zip_path" <<'EOF'
import sys
import zipfile

for name in zipfile.ZipFile(sys.argv[1]).namelist():
    print(name)
EOF
)"
  if printf '%s\n' "$listing" | grep -qx "Saarthi Studio.app/Contents/Info.plist"; then
    pass "the zip carries the bundle's Info.plist at the right path"
  else
    die "the zip does not contain Saarthi Studio.app/Contents/Info.plist"
  fi
  if printf '%s\n' "$listing" |
    grep -qx "Saarthi Studio.app/Contents/Resources/static/index.html"; then
    pass "the zip carries the UI inside Resources with forward-slash paths"
  else
    die "the zip's UI path is missing or uses the host separator"
  fi
else
  printf 'skip zip listing (no python available)\n'
fi

# --- refusals --------------------------------------------------------------
mkdir -p "$tmp/nostatic"
printf '#!/bin/sh\nexit 0\n' >"$tmp/nostatic/free-ai-studio"
if bash "$bundler" "$tmp/nostatic" "$version" "$tmp/out2" >/dev/null 2>&1; then
  die "accepted a stage with no static tree"
else
  pass "refuses a stage with no static tree"
fi

mkdir -p "$tmp/nobinary/static"
printf '<html></html>\n' >"$tmp/nobinary/static/index.html"
if bash "$bundler" "$tmp/nobinary" "$version" "$tmp/out2" >/dev/null 2>&1; then
  die "accepted a stage with no binary"
else
  pass "refuses a stage with no binary"
fi

if bash "$bundler" "$tmp/stage" "not-a-version" "$tmp/out2" >/dev/null 2>&1; then
  die "accepted an unusable version"
else
  pass "refuses an unusable version"
fi

# --- a rebuild replaces the tree ------------------------------------------
printf 'stale\n' >"$app/Contents/Resources/stale.txt"
if bash "$bundler" "$tmp/stage" "$version" "$tmp/out" >/dev/null 2>&1; then
  if [ -e "$app/Contents/Resources/stale.txt" ]; then
    die "a rebuild kept a file from the previous bundle"
  else
    pass "a rebuild replaces the previous bundle instead of merging into it"
  fi
else
  die "a rebuild over an existing bundle failed"
fi

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ] || exit 1
