#!/usr/bin/env bash
# Validate the finalized release directory before anything may be published.
#
# Checks that:
#   - every artifact the versioned contract promises exists and is non-empty;
#   - SHA256SUMS.txt covers exactly the files and verifies;
#   - the SBOM is CycloneDX 1.5 and its application version matches;
#   - the manifest version/commit match and every hash matches the file;
#   - release notes are for this version.
#
# Usage: scripts/ci/validate-release-metadata.sh <dist-dir> <version>

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
dist="${1:-}"
version="${2:-}"
[ -n "$dist" ] && [ -d "$dist" ] || { echo "usage: scripts/ci/validate-release-metadata.sh <dist> <version>" >&2; exit 2; }
[ -n "$version" ] || { echo "usage: scripts/ci/validate-release-metadata.sh <dist> <version>" >&2; exit 2; }
bash "$here/version.sh" --check-version "$version"

if command -v python3 >/dev/null 2>&1; then py=python3; else py=python; fi

while IFS= read -r name; do
  [ -s "$dist/$name" ] || { echo "ERROR: release asset missing or empty: $name" >&2; exit 1; }
done < <(bash "$here/release-assets.sh" "$version")

for name in sbom.cdx.json release-manifest.json RELEASE_NOTES.md SHA256SUMS.txt; do
  [ -s "$dist/$name" ] || { echo "ERROR: release metadata missing: $name" >&2; exit 1; }
done

bash "$here/verify-checksums.sh" "$dist" >/dev/null

"$py" - "$dist" "$version" <<'PY'
import hashlib
import json
import os
import sys

dist, version = sys.argv[1:3]

bom = json.load(open(os.path.join(dist, "sbom.cdx.json"), encoding="utf-8"))
assert bom.get("bomFormat") == "CycloneDX", "SBOM is not CycloneDX"
assert bom.get("specVersion") == "1.5", "SBOM must be CycloneDX 1.5"
assert bom["metadata"]["component"]["version"] == version, "SBOM version mismatch"
assert len(bom.get("components") or []) > 100, "SBOM has too few components"

manifest = json.load(open(os.path.join(dist, "release-manifest.json"), encoding="utf-8"))
assert manifest.get("schema") == "sarthi.release.manifest/v1", "manifest schema"
assert manifest.get("version") == version, "manifest version mismatch"
commit = manifest.get("source", {}).get("commit") or ""
assert len(commit) >= 7, "manifest has no source commit"
assert manifest.get("signing"), "manifest must state signing status"

for entry in manifest.get("artifacts") or []:
    name = entry.get("name")
    path = os.path.join(dist, name)
    assert os.path.isfile(path), f"manifest names missing file {name}"
    digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
    assert digest == entry.get("sha256"), f"manifest hash mismatch for {name}"
    assert os.path.getsize(path) == entry.get("size"), f"manifest size mismatch for {name}"

notes = open(os.path.join(dist, "RELEASE_NOTES.md"), encoding="utf-8").read()
assert f"v{version}" in notes, "release notes are for another version"
assert "Known limitations" in notes, "release notes omit known limitations"

print(
    f"metadata valid: {len(bom['components'])} SBOM components, "
    f"{len(manifest['artifacts'])} manifest artifacts, checksums verified"
)
PY
