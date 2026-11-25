#!/usr/bin/env bash
# Offline regression test for SBOM, manifest, release notes and checksums.
#
# Usage: scripts/ci/test-release-metadata.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
version="$(bash "$here/version.sh" "$root/app/Cargo.toml")"

if command -v python3 >/dev/null 2>&1; then py=python3; else py=python; fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/dist"

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() { failed=$((failed + 1)); printf 'FAIL %s\n' "$1" >&2; }

# Release notes must come from this version's changelog section.
if bash "$here/release-notes.sh" "$version" >"$tmp/dist/RELEASE_NOTES.md"; then
  grep -q "Sarthi Studio v$version" "$tmp/dist/RELEASE_NOTES.md" &&
    grep -q "Known limitations" "$tmp/dist/RELEASE_NOTES.md" &&
    pass "release notes name the version and limitations" ||
    die "release notes are missing required sections"
else
  die "release notes refused version $version"
fi
if bash "$here/release-notes.sh" 0.0.0-does-not-exist >/dev/null 2>&1; then
  die "release notes accepted a version with no changelog section"
else
  pass "release notes refuse a version with no changelog section"
fi

# SBOM: standard CycloneDX shape, real components, deterministic identity.
bash "$here/generate-sbom.sh" "$tmp/dist/sbom.cdx.json" >/dev/null
"$py" - "$tmp/dist/sbom.cdx.json" "$version" <<'PY' || exit 1
import json, sys
path, version = sys.argv[1:3]
bom = json.load(open(path, encoding="utf-8"))
assert bom["bomFormat"] == "CycloneDX"
assert bom["specVersion"] == "1.5"
assert bom["metadata"]["component"]["version"] == version
assert len(bom["components"]) > 100, len(bom["components"])
assert all(c.get("purl") and c.get("version") for c in bom["components"])
assert not any("BEGIN PRIVATE KEY" in json.dumps(c) for c in bom["components"])
PY
pass "SBOM is valid CycloneDX 1.5 with the locked components"

# Manifest + checksums over a realistic set of files.
printf 'linux artifact\n' >"$tmp/dist/sarthi-studio_${version}_linux-x86_64.tar.gz"
printf 'windows artifact\n' >"$tmp/dist/sarthi-studio_${version}_windows-x86_64.zip"
bash "$here/generate-release-manifest.sh" "$tmp/dist" "$version" "v$version" >/dev/null
"$py" - "$tmp/dist/release-manifest.json" "$version" <<'PY' || exit 1
import json, sys
path, version = sys.argv[1:3]
manifest = json.load(open(path, encoding="utf-8"))
assert manifest["schema"] == "sarthi.release.manifest/v1"
assert manifest["version"] == version
assert manifest["source"]["commit"]
assert len(manifest["artifacts"]) == 4, len(manifest["artifacts"])
assert all(len(a["sha256"]) == 64 for a in manifest["artifacts"])
assert manifest["signing"]["windows_authenticode"] is False
PY
pass "release manifest binds artifacts to version and commit"

( cd "$tmp/dist" && sha256sum ./* >SHA256SUMS.txt )
bash "$here/verify-checksums.sh" "$tmp/dist" >/dev/null
pass "checksum file covers and verifies every artifact"

printf 'tampered\n' >>"$tmp/dist/sarthi-studio_${version}_windows-x86_64.zip"
if bash "$here/verify-checksums.sh" "$tmp/dist" >/dev/null 2>&1; then
  die "checksum verification accepted a modified artifact"
else
  pass "checksum verification rejects a modified artifact"
fi

# Contract versioning: metadata is required from 3.88.15, not from historical tags.
bash "$here/release-assets.sh" 3.88.14 >"$tmp/old.txt"
bash "$here/release-assets.sh" 3.88.15 >"$tmp/new.txt"
grep -q '^sbom.cdx.json$' "$tmp/old.txt" && die "historical contract requires SBOM" || pass "historical releases do not require new metadata"
grep -q '^sbom.cdx.json$' "$tmp/new.txt" && pass "current contract requires SBOM" || die "current contract omits SBOM"

printf '\n%d passed, %d failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
