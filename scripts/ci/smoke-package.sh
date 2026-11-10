#!/usr/bin/env bash
# Install smoke: prove a packaged artifact actually runs.
#
# Unpacks the artifact the way a user would — portable tarball, Windows zip,
# .deb, AppImage, or a bare binary — boots the binary inside it headless against
# a scratch home, and asks it for its own version. A build that packages but
# cannot boot, serves no UI, or boots as the wrong version fails here instead of
# failing for a user.
#
# Usage: scripts/ci/smoke-package.sh <artifact> [expected-version]
#   SMOKE_KEEP=1   keep the unpacked tree and the studio log
#
# Needs: curl, python3 (or python), tar; dpkg-deb for .deb, exec for AppImage.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

artifact="${1:-}"
expected="${2:-}"
[ -n "$artifact" ] || {
  echo "usage: scripts/ci/smoke-package.sh <artifact> [expected-version]" >&2
  exit 2
}
[ -e "$artifact" ] || {
  echo "no such artifact: $artifact" >&2
  exit 1
}
[ -n "$expected" ] || expected="$(bash "$here/version.sh" "$root/app/Cargo.toml")"

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python exists 2>/dev/null || command -v python >/dev/null 2>&1; then
  py=python
else
  echo "python is required to unpack a zip and read the version" >&2
  exit 1
fi
command -v curl >/dev/null 2>&1 || {
  echo "curl is required" >&2
  exit 1
}

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() {
  failed=$((failed + 1))
  printf 'FAIL %s\n' "$1" >&2
}

work="$(mktemp -d)"
studio_pid=""
cleanup() {
  if [ -n "$studio_pid" ]; then
    kill "$studio_pid" 2>/dev/null || true
    wait "$studio_pid" 2>/dev/null || true
  fi
  if [ "${SMOKE_KEEP:-0}" = "1" ]; then
    printf '\nkept %s\n' "$work"
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

# The binary is native and the sandbox home is passed to it, so on Windows a
# Git Bash `/tmp/...` path would not resolve: `cygpath -m` keeps it absolute.
native() { cygpath -m "$1" 2>/dev/null || printf '%s' "$1"; }

unpacked="$work/unpacked"
mkdir -p "$unpacked"
base="$(basename "$artifact")"

case "$base" in
  *.tar.gz | *.tgz)
    tar -xzf "$artifact" -C "$unpacked" || die "could not unpack $base"
    [ "$failed" -eq 0 ] && pass "unpacked $base as a tarball"
    ;;
  *.zip)
    "$py" - "$artifact" "$unpacked" <<'EOF' || die "could not unpack $base"
import sys, zipfile
zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])
EOF
    [ "$failed" -eq 0 ] && pass "unpacked $base as a zip"
    ;;
  *.deb)
    command -v dpkg-deb >/dev/null 2>&1 || {
      die "dpkg-deb is needed to smoke a .deb"
      exit 1
    }
    dpkg-deb -x "$artifact" "$unpacked" || die "could not unpack $base"
    [ "$failed" -eq 0 ] && pass "unpacked $base with dpkg-deb"
    ;;
  *.AppImage)
    cp "$artifact" "$work/app.AppImage"
    chmod +x "$work/app.AppImage"
    (cd "$work" && ./app.AppImage --appimage-extract >/dev/null 2>&1) ||
      die "could not extract $base"
    [ -d "$work/squashfs-root" ] || die "$base has no squashfs-root"
    [ "$failed" -eq 0 ] && pass "extracted $base with --appimage-extract"
    ;;
  *)
    # A bare binary is the local case: no packaging, same checks.
    mkdir -p "$work/bare"
    cp "$artifact" "$work/bare/$(basename "$artifact")"
    [ "$failed" -eq 0 ] && pass "smoking $base directly (no package)"
    ;;
esac
[ "$failed" -eq 0 ] || exit 1

# The artifact's own copy of the binary, not the build tree's: that is what a
# user runs, and what a broken staging step would leave out.
search="$unpacked"
[ -d "$work/squashfs-root" ] && search="$work/squashfs-root"
[ -d "$work/bare" ] && search="$work/bare"
binary="$(find "$search" -type f \( -name 'free-ai-studio' -o -name 'free-ai-studio.exe' \) 2>/dev/null | head -n1)"
[ -n "$binary" ] || {
  die "$base contains no free-ai-studio binary"
  exit 1
}
chmod +x "$binary" 2>/dev/null || true
pass "artifact contains the binary ($(basename "$search")/$(realpath --relative-to="$search" "$binary" 2>/dev/null || basename "$binary"))"

# The UI is resolved from static/ beside the binary; a package without it boots
# a server that serves nothing. A macOS .app keeps it one level up instead, in
# Contents/Resources/static, which the binary resolves as
# `<exe dir>/../Resources/static`.
static="$(dirname "$binary")/static"
bundle_static="$(dirname "$binary")/../Resources/static"
if [ -f "$static/index.html" ] || [ -f "$static/app.js" ]; then
  pass "the packaged static tree is beside the binary"
elif [ -f "$bundle_static/index.html" ] || [ -f "$bundle_static/app.js" ]; then
  pass "the packaged static tree is in the app bundle's Resources"
else
  die "no static tree beside the packaged binary ($static or $bundle_static)"
fi

# A free port, chosen by binding one and letting go.
port="$("$py" - <<'EOF'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
EOF
)"
base_url="http://127.0.0.1:$port"
home="$(native "$work/home")"
mkdir -p "$work/home"

( cd "$work" && FREE_AI_STUDIO_HOME="$home" FREE_AI_STUDIO_PORT="$port" \
  FREE_AI_STUDIO_SKIP_ROUTER=1 exec "$binary" --headless ) >>"$work/studio.log" 2>&1 &
studio_pid=$!

ready="no"
for _ in $(seq 1 80); do
  if [ "$(curl -sS -o /dev/null -w '%{http_code}' "$base_url/api/ready" 2>/dev/null)" = "200" ]; then
    ready="yes"
    break
  fi
  sleep 0.25
done
if [ "$ready" = "yes" ]; then
  pass "the packaged binary boots headless and reports ready on $port"
else
  die "the packaged binary never became ready"
  tail -20 "$work/studio.log" >&2
fi

ui="$(curl -sS -o "$work/index.html" -w '%{http_code}' "$base_url/" 2>/dev/null)"
if [ "$ui" = "200" ] && grep -qi '<title>' "$work/index.html"; then
  pass "the packaged UI answers with an HTML document"
else
  die "the packaged UI did not answer (HTTP ${ui:-none})"
fi

curl -sS -o "$work/bundle.json" "$base_url/api/diagnostics/bundle" 2>/dev/null
actual="$("$py" - "$work/bundle.json" <<'EOF' 2>/dev/null
import json, sys
try:
    print(json.load(open(sys.argv[1], encoding="utf-8")).get("app", {}).get("version", ""))
except Exception:
    print("")
EOF
)"
if [ "$actual" = "$expected" ]; then
  pass "the packaged binary reports its own version ($actual)"
else
  die "the packaged binary reports version '${actual:-none}', expected $expected"
fi

printf '\n%s passed, %s failed (%s)\n' "$passed" "$failed" "$base"
[ "$failed" -eq 0 ] || exit 1
