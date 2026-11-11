#!/usr/bin/env bash
# Offline regression test for scripts/ci/verify-release.sh.
#
# The gate exists because v1.62.0-v1.71.0 were published with zero assets. This
# test serves a fixture release from a loopback HTTP server, so it checks both
# the complete-release pass and the missing-asset failure without a token or a
# network connection.
#
# Usage: scripts/ci/test-release.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$here/verify-release.sh"
audit="$here/audit-releases.sh"
tmp="$(mktemp -d)"
server=""
trap 'if [ -n "$server" ]; then kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true; fi; rm -rf "$tmp"' EXIT

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  echo "python is required for the release-gate test" >&2
  exit 1
fi

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() { failed=$((failed + 1)); printf 'FAIL %s\n' "$1" >&2; }

release_dir="$tmp/www/repos/fixture/repo/releases/tags"
list_dir="$tmp/www/repos/fixture/repo/releases"
mkdir -p "$release_dir"
release_file="$release_dir/v9.9.9"
list_file="$list_dir/index.html"

write_release() { # missing asset name (empty for complete)
  "$py" - "$release_file" "$1" <<'PY'
import json
import sys

path, missing = sys.argv[1:3]
version = "9.9.9"
names = [
    "free-ai-studio-android.aab",
    "free-ai-studio-android.apk",
    "free-ai-studio-vscode.vsix",
    f"free-ai-studio_{version}-1_amd64.deb",
    f"sarthi-studio_{version}_linux-x86_64.tar.gz",
    f"sarthi-studio_{version}_macos-universal.app.zip",
    f"sarthi-studio_{version}_macos-universal.tar.gz",
    f"sarthi-studio_{version}_windows-x86_64.msi",
    f"sarthi-studio_{version}_windows-x86_64.zip",
    f"sarthi-studio_{version}_x86_64.AppImage",
    "SHA256SUMS.txt",
]
assets = [{"name": name, "size": 1024} for name in names if name != missing]
with open(path, "w", encoding="utf-8") as handle:
    json.dump({"draft": False, "assets": assets}, handle)
PY
}

write_releases_index() {
  # http.server serves index.html for the directory URL, and the audit's curl
  # follows the directory redirect. The body is JSON; the content type is not
  # consulted.
  printf '%s\n' '[{"tag_name":"v9.9.9","draft":false,"prerelease":false}]' >"$list_file"
}

port="$("$py" - <<'PY'
import socket
sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
PY
)"
base="http://127.0.0.1:$port"
write_release ""
"$py" -m http.server "$port" --bind 127.0.0.1 --directory "$tmp/www" >"$tmp/server.log" 2>&1 &
server=$!
ready=no
for _ in $(seq 1 50); do
  if curl -fsS -o /dev/null "$base/" 2>/dev/null; then
    ready=yes
    break
  fi
  sleep 0.1
done
[ "$ready" = "yes" ] || {
  cat "$tmp/server.log" >&2
  echo "FAIL fixture server did not start" >&2
  exit 1
}

if GH_TOKEN=test GH_API_URL="$base" GH_REPOSITORY=fixture/repo \
  bash "$tool" v9.9.9 9.9.9 >"$tmp/complete.out" 2>&1; then
  pass "accepts a release with every expected asset"
else
  cat "$tmp/complete.out" >&2
  die "rejected a complete release"
fi

write_releases_index
if GH_TOKEN=test GH_API_URL="$base" GH_REPOSITORY=fixture/repo \
  MIN_RELEASE_VERSION=9.9.9 bash "$audit" >"$tmp/audit-complete.out" 2>&1; then
  pass "audit accepts a complete release at the minimum version"
else
  cat "$tmp/audit-complete.out" >&2
  die "audit rejected a complete release"
fi

write_release "free-ai-studio-vscode.vsix"
if GH_TOKEN=test GH_API_URL="$base" GH_REPOSITORY=fixture/repo \
  bash "$tool" v9.9.9 9.9.9 >"$tmp/missing.out" 2>&1; then
  die "accepted a release with a missing asset"
else
  grep -q "free-ai-studio-vscode.vsix" "$tmp/missing.out" ||
    die "missing-asset failure did not name the asset"
  pass "rejects a release with a missing asset"
fi

if GH_TOKEN=test GH_API_URL="$base" GH_REPOSITORY=fixture/repo \
  MIN_RELEASE_VERSION=9.9.9 bash "$audit" >"$tmp/audit-missing.out" 2>&1; then
  die "audit accepted a release with a missing asset"
else
  grep -q "free-ai-studio-vscode.vsix" "$tmp/audit-missing.out" ||
    die "audit failure did not name the missing asset"
  pass "audit rejects a release with a missing asset"
fi

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ] || exit 1
