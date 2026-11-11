#!/usr/bin/env bash
# Audit every published release at or above a minimum version and refuse any
# release missing one of its promised assets.
#
# Why this exists: the release workflow's own verification only runs when a
# workflow is triggered. A release created through the API with no tag push and
# no usable release event could still sit there with zero assets. This runs on a
# schedule, checks only releases at or above MIN_RELEASE_VERSION (so the known
# v1.62.0-v1.71.0 failures are not re-reported every day), and fails the audit
# when any of them is incomplete.
#
# Usage: GH_TOKEN=... scripts/ci/audit-releases.sh
#   GH_REPOSITORY         owner/repo (default: sairajKhilariArch/Saarthi-Studio)
#   GH_API_URL            API base for a fixture server
#   MIN_RELEASE_VERSION   first version to audit (default: 3.4.0)
#
# The list request asks for the newest 100 releases, which is enough to cover
# every release at or above the current audit floor.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

repo="${GH_REPOSITORY:-sairajKhilariArch/Saarthi-Studio}"
api="${GH_API_URL:-https://api.github.com}"
api="${api%/}"
token="${GH_TOKEN:-}"
minimum="${MIN_RELEASE_VERSION:-3.4.0}"

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

[ -n "$token" ] || die "GH_TOKEN is required to read releases"
command -v curl >/dev/null 2>&1 || die "curl is required"

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  die "python is required to read the release list"
fi

json="$(mktemp)"
tags="$(mktemp)"
trap 'rm -f "$json" "$tags"' EXIT

# -L matters for the fixture server: a directory URL redirects to index.html.
curl -fsSL --max-time 30 --retry 3 --retry-delay 2 \
  -H "Authorization: Bearer $token" \
  -H "Accept: application/vnd.github+json" \
  "$api/repos/$repo/releases?per_page=100" -o "$json" ||
  die "could not list releases from $repo"

"$py" - "$json" "$minimum" >"$tags" <<'PY'
import json
import re
import sys

path, minimum = sys.argv[1:3]
module = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)")

def version_tuple(value):
    match = module.match(value or "")
    return tuple(int(part) for part in match.groups()) if match else None

minimum_tuple = version_tuple(minimum)
if minimum_tuple is None:
    print(f"ERROR: unusable MIN_RELEASE_VERSION: {minimum}", file=sys.stderr)
    sys.exit(1)

with open(path, encoding="utf-8") as handle:
    releases = json.load(handle)

if not isinstance(releases, list):
    print("ERROR: release list was not a JSON array", file=sys.stderr)
    sys.exit(1)

for release in releases:
    if release.get("draft"):
        continue
    tag = str(release.get("tag_name") or "")
    version = version_tuple(tag)
    if version is None or version < minimum_tuple:
        continue
    print(tag)
PY

if [ ! -s "$tags" ]; then
  printf 'release audit: no published releases at or above %s\n' "$minimum"
  exit 0
fi

failed=0
while IFS= read -r tag; do
  [ -n "$tag" ] || continue
  version="${tag#v}"
  printf 'release audit: checking %s\n' "$tag"
  if ! bash "$here/verify-release.sh" "$tag" "$version"; then
    failed=1
  fi
done <"$tags"

if [ "$failed" -ne 0 ]; then
  die "one or more releases at or above $minimum are incomplete"
fi

printf 'release audit passed: every release at or above %s is complete\n' "$minimum"
