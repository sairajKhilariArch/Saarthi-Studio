#!/usr/bin/env bash
# Verify that a published GitHub release actually carries every artifact the
# release workflow promises.
#
# Why this exists: releases v1.62.0-v1.71.0 were created with zero assets. The
# build-and-attach jobs had never run, and nothing checked the release itself,
# so a release page could look successful while there was nothing to install.
# This gate runs after every build job and fails the workflow when an expected
# asset is missing or empty.
#
# Usage: GH_TOKEN=... scripts/ci/verify-release.sh <tag> [version]
#   GH_REPOSITORY   owner/repo (default: sairajKhilariArch/Saarthi-Studio)
#   GH_API_URL      API base for a local/fixture server
#
# Needs: curl and python3 (or python).

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

tag="${1:-}"
version="${2:-}"
[ -n "$tag" ] || {
  echo "usage: scripts/ci/verify-release.sh <tag> [version]" >&2
  exit 2
}
[ -n "$version" ] || version="$(bash "$here/version.sh" "$root/app/Cargo.toml")"

repo="${GH_REPOSITORY:-sairajKhilariArch/Saarthi-Studio}"
api="${GH_API_URL:-https://api.github.com}"
api="${api%/}"
token="${GH_TOKEN:-}"

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

[ -n "$token" ] || die "GH_TOKEN is required to read the release"
command -v curl >/dev/null 2>&1 || die "curl is required"

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  die "python is required to read the release JSON"
fi

json="$(mktemp)"
trap 'rm -f "$json"' EXIT

curl -fsS --max-time 30 --retry 3 --retry-delay 2 \
  -H "Authorization: Bearer $token" \
  -H "Accept: application/vnd.github+json" \
  "$api/repos/$repo/releases/tags/$tag" -o "$json" ||
  die "could not read release $tag from $repo"

"$py" - "$json" "$tag" "$version" <<'PY'
import json
import sys

path, tag, version = sys.argv[1:4]
with open(path, encoding="utf-8") as handle:
    release = json.load(handle)

if release.get("draft"):
    print(f"ERROR: release {tag} is still a draft", file=sys.stderr)
    sys.exit(1)

expected = {
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
}

assets = release.get("assets") or []
sizes = {str(a.get("name")): int(a.get("size") or 0) for a in assets}
missing = sorted(expected - set(sizes))
empty = sorted(name for name in expected if sizes.get(name, 0) <= 0)

if missing:
    print(f"ERROR: release {tag} is missing {len(missing)} expected asset(s):", file=sys.stderr)
    for name in missing:
        print(f"  {name}", file=sys.stderr)
if empty:
    print(f"ERROR: release {tag} has {len(empty)} empty expected asset(s):", file=sys.stderr)
    for name in empty:
        print(f"  {name}", file=sys.stderr)
if missing or empty:
    print(
        f"release has {len(assets)} asset(s): "
        + (", ".join(sorted(sizes)) if sizes else "none"),
        file=sys.stderr,
    )
    sys.exit(1)

print(f"release {tag} carries all {len(expected)} expected assets ({len(assets)} total)")
PY
