#!/usr/bin/env bash
# The last gate before a PUBLIC release. It re-validates the finalized bundle
# and refuses to publish when a mandatory trust property is missing.
#
# Exit codes:
#   0  every publication gate passed
#   1  the bundle is invalid (missing file, bad checksum, wrong version)
#   3  the bundle is valid but BLOCKED by an unsigned/unvalidated artifact
#
# Usage: scripts/ci/check-publish-gates.sh <dist-dir> <version>

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dist="${1:-}"
version="${2:-}"
[ -n "$dist" ] && [ -d "$dist" ] || { echo "usage: scripts/ci/check-publish-gates.sh <dist> <version>" >&2; exit 2; }
[ -n "$version" ] || { echo "usage: scripts/ci/check-publish-gates.sh <dist> <version>" >&2; exit 2; }

bash "$here/validate-release-metadata.sh" "$dist" "$version"

if command -v python3 >/dev/null 2>&1; then py=python3; else py=python; fi

"$py" - "$dist/release-manifest.json" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
signing = manifest.get("signing") or {}
required = {
    "windows_authenticode": "Windows Authenticode signature",
    "macos_developer_id": "macOS Developer ID signature",
    "macos_notarized": "macOS notarization and stapling",
    "android_release_keystore": "Android release keystore signature",
    "linux_manifest_signature": "signed Linux release manifest",
}
blocked = [label for key, label in required.items() if signing.get(key) is not True]

if blocked:
    print("BLOCKED: the bundle is complete and verified, but publication is refused because:")
    for label in blocked:
        print(f"  BLOCKED  {label}")
    print("Configure the signing credentials, sign the artifacts, record the verified")
    print("status in release-manifest.json, then run this gate again.")
    sys.exit(3)

print("PASS: every publication gate passed")
PY
