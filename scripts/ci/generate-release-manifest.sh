#!/usr/bin/env bash
# Write release-manifest.json: the machine-readable identity of a release.
#
# It binds the artifact hashes to the source commit, version and workflow run,
# and states the signing status explicitly. It never includes environment
# values beyond the public CI identifiers.
#
# Usage: scripts/ci/generate-release-manifest.sh <dist-dir> <version> [tag]
#   GITHUB_SHA, GITHUB_RUN_ID, GITHUB_REPOSITORY, GITHUB_WORKFLOW_REF optional

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
dist="${1:-}"
version="${2:-}"
tag="${3-v$version}"
[ -n "$dist" ] && [ -d "$dist" ] || { echo "usage: scripts/ci/generate-release-manifest.sh <dist-dir> <version> [tag]" >&2; exit 2; }
bash "$here/version.sh" --check-version "$version"

if command -v python3 >/dev/null 2>&1; then py=python3; else py=python; fi

commit="${GITHUB_SHA:-$(git -C "$root" rev-parse HEAD 2>/dev/null || echo unknown)}"
run="${GITHUB_RUN_ID:-local}"
repo="${GITHUB_REPOSITORY:-sairajKhilariArch/Saarthi-Studio}"
workflow="${GITHUB_WORKFLOW_REF:-local}"

"$py" - "$dist" "$version" "$tag" "$commit" "$run" "$repo" "$workflow" <<'PY'
import datetime
import hashlib
import json
import os
import sys

dist, version, tag, commit, run, repo, workflow = sys.argv[1:8]
assets = []
for name in sorted(os.listdir(dist)):
    path = os.path.join(dist, name)
    if not os.path.isfile(path) or name in {"release-manifest.json", "SHA256SUMS.txt"}:
        continue
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    assets.append({"name": name, "size": os.path.getsize(path), "sha256": digest.hexdigest()})

manifest = {
    "schema": "sarthi.release.manifest/v1",
    "version": version,
    "tag": tag,
    "source": {"commit": commit, "repository": repo, "workflow": workflow, "run_id": run},
    "generated_at": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
    "signing": {
        "windows_authenticode": False,
        "macos_developer_id": False,
        "macos_notarized": False,
        "android_release_keystore": False,
        "linux_manifest_signature": False,
    },
    "artifacts": assets,
}
with open(os.path.join(dist, "release-manifest.json"), "w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2, sort_keys=True)
    handle.write("\n")
print(f"manifest: {len(assets)} artifacts for {version} ({commit[:12]})")
PY
