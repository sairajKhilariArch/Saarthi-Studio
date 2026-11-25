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

# The fixture release is built from the same contract the real gate reads, so a
# name this test does not know about cannot be silently missing from it.
assets_list="$tmp/assets-9.9.9.txt"
bash "$here/release-assets.sh" 9.9.9 >"$assets_list"

write_release_for() { # path, assets file, then the names to leave out
  "$py" - "$@" <<'PY'
import json
import sys

path, assets_path, *missing = sys.argv[1:]
with open(assets_path, encoding="utf-8") as handle:
    names = [line.strip() for line in handle if line.strip()]
assets = [{"name": name, "size": 1024} for name in names if name not in missing]
with open(path, "w", encoding="utf-8") as handle:
    json.dump({"draft": False, "assets": assets}, handle)
PY
}

write_release() { # missing asset name (empty for complete)
  write_release_for "$release_file" "$assets_list" "$@"
}

write_audit_index() { # tag...
  # http.server serves index.html for the directory URL, and the audit's curl
  # follows the directory redirect. The body is JSON; the content type is not
  # consulted.
  "$py" - "$list_file" "$@" <<'PY'
import json
import sys

path, *tags = sys.argv[1:]
with open(path, "w", encoding="utf-8") as handle:
    json.dump(
        [{"tag_name": tag, "draft": False, "prerelease": False} for tag in tags],
        handle,
    )
PY
}

write_releases_index() {
  write_audit_index v9.9.9
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
"$py" - "$tmp/packages" "$here/../../android/app/build.gradle.kts" <<'PY'
import json
import re
import sys
import zipfile

out = sys.argv[1]
# The smoke reads the launch activity's namespace from the real project file;
# the fixtures must carry the same descriptor or they test a different app.
gradle = open(sys.argv[2], encoding="utf-8").read()
namespace = re.search(r'namespace\s*=\s*"([^"]+)"', gradle).group(1)
descriptor = f"L{namespace.replace('.', '/')}/MainActivity;".encode()


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


def apk(name, launch=True, signed=True):
    # The app is native Java: no React Native bundle or libraries. The dex
    # must carry the launch activity, and the signature is a v1 META-INF
    # block (the v2 fixture below covers the scheme AGP actually writes).
    with zipfile.ZipFile(f"{out}/{name}", "w") as archive:
        archive.writestr("AndroidManifest.xml", b"\x03\x00\x08\x00" + b"\x00" * 16)
        dex = b"dex\n035\x00" + b"\x00" * 64
        if launch:
            dex += descriptor
        archive.writestr("classes.dex", dex)
        archive.writestr("resources.arsc", b"\x02\x00\x0c\x00" + b"\x00" * 32)
        if signed:
            archive.writestr("META-INF/CERT.RSA", b"\x30\x82" + b"\x00" * 32)


def add_v2_block(name):
    # Insert a minimal APK Signing Block with a v2 ID pair before the central
    # directory and shift the EOCD's directory offset, exactly where a real
    # v2-signed APK carries it.
    path = f"{out}/{name}"
    raw = open(path, "rb").read()
    eocd = raw.rfind(b"PK\x05\x06")
    cd = int.from_bytes(raw[eocd + 16:eocd + 20], "little")
    pair = (4).to_bytes(8, "little") + (0x7109871A).to_bytes(4, "little")
    size = (len(pair) + 24).to_bytes(8, "little")
    block = size + pair + size + b"APK Sig Block 42"
    moved = raw[:cd] + block + raw[cd:]
    offset_at = eocd + len(block) + 16
    moved = (
        moved[:offset_at]
        + (cd + len(block)).to_bytes(4, "little")
        + moved[offset_at + 4:]
    )
    open(path, "wb").write(moved)


vsix("good.vsix")
vsix("no-entry.vsix", entry=False)
vsix("secret.vsix", secret=True)
apk("good.apk")
apk("good-v2.apk", signed=False)
add_v2_block("good-v2.apk")
apk("no-launch.apk", launch=False)
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
if bash "$apk_smoke" "$tmp/packages/good-v2.apk" >"$tmp/apk-v2.out" 2>&1; then
  grep -q 'APK Signature Scheme v2' "$tmp/apk-v2.out" ||
    die "the v2 signature was not named"
  pass "apk smoke accepts a v2-signed apk with no v1 entries"
else
  cat "$tmp/apk-v2.out" >&2
  die "apk smoke rejected a v2-signed apk"
fi
if bash "$apk_smoke" "$tmp/packages/no-launch.apk" >"$tmp/apk-launch.out" 2>&1; then
  die "apk smoke accepted an apk whose dex lacks the launch activity"
else
  grep -q 'MainActivity' "$tmp/apk-launch.out" ||
    die "apk failure did not name the missing launch activity"
  pass "apk smoke rejects an apk whose dex lacks the launch activity"
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

# --- the four known-incomplete historical releases ---------------------------
# These release pages predate the 11-asset contract and cannot be rebuilt. Each
# fixture removes exactly the names that release actually lacks, then one asset
# it does have: the first audit must pass and the second must fail. This pins
# all four allow-lists, not just the newest one.
check_known_release() { # version, an asset it has, then the names it lacks
  local version="$1" extra_gap="$2"
  shift 2
  local tag="v$version"
  local release="$release_dir/$tag"
  local assets="$tmp/assets-$version.txt"
  local out="$tmp/audit-$version.out"
  bash "$here/release-assets.sh" "$version" >"$assets"

  write_release_for "$release" "$assets" "$@"
  write_audit_index "$tag"
  if GH_TOKEN=test GH_API_URL="$base" GH_REPOSITORY=fixture/repo \
    MIN_RELEASE_VERSION="$version" bash "$audit" >"$out" 2>&1; then
    grep -q 'known-incomplete historical release' "$out" ||
      die "audit did not report the historical exception for $tag"
    pass "audit accepts the known-incomplete $tag"
  else
    cat "$out" >&2
    die "audit rejected the known-incomplete $tag"
  fi

  write_release_for "$release" "$assets" "$@" "$extra_gap"
  if GH_TOKEN=test GH_API_URL="$base" GH_REPOSITORY=fixture/repo \
    MIN_RELEASE_VERSION="$version" bash "$audit" >"$out" 2>&1; then
    die "audit accepted a new gap ($extra_gap) on $tag"
  else
    grep -Fq "$extra_gap" "$out" ||
      die "the new gap $extra_gap was not named for $tag"
    pass "audit rejects a new gap on $tag"
  fi
}

check_known_release 3.83.0 free-ai-studio-vscode.vsix \
  sarthi-studio_3.83.0_macos-universal.app.zip \
  sarthi-studio_3.83.0_macos-universal.tar.gz \
  sarthi-studio_3.83.0_windows-x86_64.msi \
  sarthi-studio_3.83.0_windows-x86_64.zip
check_known_release 3.75.9 free-ai-studio-android.apk \
  free-ai-studio-vscode.vsix \
  free-ai-studio_3.75.9-1_amd64.deb \
  sarthi-studio_3.75.9_linux-x86_64.tar.gz \
  sarthi-studio_3.75.9_x86_64.AppImage \
  SHA256SUMS.txt
check_known_release 3.74.2 free-ai-studio-vscode.vsix \
  free-ai-studio-android.aab \
  free-ai-studio-android.apk \
  free-ai-studio_3.74.2-1_amd64.deb \
  sarthi-studio_3.74.2_windows-x86_64.msi \
  sarthi-studio_3.74.2_windows-x86_64.zip
check_known_release 3.74.1 free-ai-studio-vscode.vsix \
  free-ai-studio-android.aab \
  free-ai-studio-android.apk \
  free-ai-studio_3.74.1-1_amd64.deb \
  sarthi-studio_3.74.1_windows-x86_64.msi \
  sarthi-studio_3.74.1_windows-x86_64.zip

# The exemption is keyed by release, not by the asset pattern.
write_release_for "$release_dir/v9.9.9" "$assets_list" "free-ai-studio-vscode.vsix"
write_audit_index v9.9.9
if GH_TOKEN=test GH_API_URL="$base" GH_REPOSITORY=fixture/repo \
  MIN_RELEASE_VERSION=9.9.9 bash "$audit" >"$tmp/audit-unknown.out" 2>&1; then
  die "audit applied the historical exemption to an unknown release"
else
  grep -q "free-ai-studio-vscode.vsix" "$tmp/audit-unknown.out" ||
    die "the unknown release's missing asset was not named"
  pass "audit rejects the same gap on an unknown release"
fi

# --- the release contract -----------------------------------------------------
# The jobs' expected files and the release gate's list are one contract now. A
# rename on one side used to be visible only after a whole tagged run had built
# every platform; these cases fail on a push instead.
contract="$here/verify-release-contract.sh"
if bash "$contract" "$workflow" 9.9.9 >"$tmp/contract.out" 2>&1; then
  pass "the workflow promises exactly the release contract"
else
  cat "$tmp/contract.out" >&2
  die "the workflow and the release contract disagree"
fi
count="$(wc -l <"$assets_list" | tr -d ' ')"
if [ "$count" = "14" ] && grep -q '^sbom.cdx.json$' "$assets_list" \
  && grep -q '^release-manifest.json$' "$assets_list" \
  && grep -q '^RELEASE_NOTES.md$' "$assets_list"; then
  pass "the contract names the 14 release assets, including SBOM, manifest and notes"
else
  die "the contract names $count assets or is missing release metadata"
fi

# A rename in one job only: the contract still demands the old name and nothing
# builds it, so both sides are reported.
sed 's/free-ai-studio-android\.aab/free-ai-studio-android-signed.aab/' \
  "$workflow" >"$tmp/renamed.yml"
if bash "$contract" "$tmp/renamed.yml" 9.9.9 >"$tmp/renamed.out" 2>&1; then
  die "the contract check accepted an asset only one side names"
else
  grep -q 'free-ai-studio-android\.aab' "$tmp/renamed.out" ||
    die "the contract failure did not name the unbuilt asset"
  grep -q 'free-ai-studio-android-signed\.aab' "$tmp/renamed.out" ||
    die "the contract failure did not name the ungoverned asset"
  pass "a renamed asset is refused with both names"
fi

# A job that stops promising a file it used to build.
sed -e '/windows-x86_64\.msi"/d' -e 's/\(windows-x86_64\.zip"\) \\$*/\1/' \
  "$workflow" >"$tmp/dropped.yml"
if bash "$contract" "$tmp/dropped.yml" 9.9.9 >"$tmp/dropped.out" 2>&1; then
  die "the contract check accepted a dropped MSI"
else
  grep -q 'windows-x86_64\.msi' "$tmp/dropped.out" ||
    die "the dropped artifact was not named"
  pass "a job that drops an asset is refused and named"
fi

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ] || exit 1
