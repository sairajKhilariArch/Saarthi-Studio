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

# The deb job builds without Tauri, so it must install the dbus headers that
# keyring's Secret Service backend probes for; the GUI jobs get them through
# GTK. Pin that dependency here so it cannot silently disappear again.
workflow="$here/../../.github/workflows/desktop-release.yml"
deb_job="$(sed -n '/^  deb:/,/^  release:/p' "$workflow")"
if grep -Eq 'libdbus-1-dev.*pkg-config|pkg-config.*libdbus-1-dev' <<<"$deb_job"; then
  pass "deb job installs the dbus headers and pkg-config"
else
  die "deb job is missing libdbus-1-dev or pkg-config"
fi

# --- install smokes for the two artifacts that had none ---------------------
# The VSIX and the APK used to be built, uploaded and never opened. Guard that
# each job runs its smoke, then run both smokes over fixtures so the scripts
# themselves stay honest: a good package passes, a broken one fails.
vsix_job="$(sed -n '/^  vscode-extension:/,/^  android:/p' "$workflow")"
if grep -q 'smoke-vsix.sh' <<<"$vsix_job"; then
  pass "vsix job smokes the packaged extension"
else
  die "vsix job never runs smoke-vsix.sh"
fi
android_job="$(sed -n '/^  android:/,/^  verify-release:/p' "$workflow")"
if grep -q 'smoke-apk.sh' <<<"$android_job"; then
  pass "android job smokes the packaged apk and aab"
else
  die "android job never runs smoke-apk.sh"
fi

mkdir -p "$tmp/packages"
"$py" - "$tmp/packages" <<'PY'
import json
import sys
import zipfile

out = sys.argv[1]


def vsix(name, entry=True, secret=False):
    with zipfile.ZipFile(f"{out}/{name}", "w") as archive:
        package = {
            "name": "free-ai-studio",
            "publisher": "sarthi",
            "version": "9.9.9",
            "engines": {"vscode": "^1.92.0"},
            "main": "./dist/extension.js",
        }
        archive.writestr("extension/package.json", json.dumps(package))
        archive.writestr(
            "extension.vsixmanifest",
            '<PackageManifest><Metadata><Identity Language="en-US" '
            'Id="free-ai-studio" Version="9.9.9" Publisher="sarthi" />'
            "</Metadata></PackageManifest>",
        )
        if entry:
            archive.writestr("extension/dist/extension.js", "// activates\n")
        if secret:
            archive.writestr("extension/.env", "API_KEY=do-not-ship\n")


def apk(name, bundle=True, signed=True):
    with zipfile.ZipFile(f"{out}/{name}", "w") as archive:
        archive.writestr("AndroidManifest.xml", b"\x03\x00\x08\x00" + b"\x00" * 16)
        archive.writestr("classes.dex", b"dex\n035\x00" + b"\x00" * 64)
        archive.writestr("resources.arsc", b"\x02\x00\x0c\x00" + b"\x00" * 32)
        if bundle:
            archive.writestr("assets/index.android.bundle", "var x = 1;\n" * 8)
        archive.writestr("lib/arm64-v8a/libreact_native_so.so", b"\x7fELF" + b"\x00" * 64)
        if signed:
            archive.writestr("META-INF/CERT.RSA", b"\x30\x82" + b"\x00" * 32)


vsix("good.vsix")
vsix("no-entry.vsix", entry=False)
vsix("secret.vsix", secret=True)
apk("good.apk")
apk("no-bundle.apk", bundle=False)
apk("unsigned.apk", signed=False)
PY

vsix_smoke="$here/smoke-vsix.sh"
apk_smoke="$here/smoke-apk.sh"
if bash "$vsix_smoke" "$tmp/packages/good.vsix" 9.9.9 >"$tmp/vsix-good.out" 2>&1; then
  pass "vsix smoke accepts an installable package"
else
  cat "$tmp/vsix-good.out" >&2
  die "vsix smoke rejected an installable package"
fi
if bash "$vsix_smoke" "$tmp/packages/no-entry.vsix" 9.9.9 >"$tmp/vsix-entry.out" 2>&1; then
  die "vsix smoke accepted a package with no activation entry"
else
  grep -q 'main entry' "$tmp/vsix-entry.out" ||
    die "vsix failure did not name the missing entry"
  pass "vsix smoke rejects a package with no activation entry"
fi
if bash "$vsix_smoke" "$tmp/packages/secret.vsix" 9.9.9 >"$tmp/vsix-secret.out" 2>&1; then
  die "vsix smoke accepted a package carrying a .env"
else
  pass "vsix smoke rejects a package carrying a secret file"
fi

if bash "$apk_smoke" "$tmp/packages/good.apk" >"$tmp/apk-good.out" 2>&1; then
  pass "apk smoke accepts an installable apk"
else
  cat "$tmp/apk-good.out" >&2
  die "apk smoke rejected an installable apk"
fi
if bash "$apk_smoke" "$tmp/packages/no-bundle.apk" >"$tmp/apk-bundle.out" 2>&1; then
  die "apk smoke accepted an apk with no JavaScript bundle"
else
  grep -q 'index.android.bundle' "$tmp/apk-bundle.out" ||
    die "apk failure did not name the missing bundle"
  pass "apk smoke rejects an apk with no JavaScript bundle"
fi
if bash "$apk_smoke" "$tmp/packages/unsigned.apk" >"$tmp/apk-unsigned.out" 2>&1; then
  die "apk smoke accepted an unsigned apk"
else
  pass "apk smoke rejects an unsigned apk"
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
