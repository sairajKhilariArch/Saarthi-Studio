#!/usr/bin/env bash
# Generate release notes from the versioned CHANGELOG section.
#
# GitHub's generated notes are a commit list, not a product release note. This
# emits the human-written section for exactly this version, plus the supported
# platform and verification facts a downloader needs. It refuses when the
# version has no section, so a release cannot carry notes for another build.
#
# Usage: scripts/ci/release-notes.sh <version> [changelog]
#   RELEASE_DATE  optional YYYY-MM-DD; defaults to today (UTC)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
version="${1:-}"
changelog="${2:-$root/CHANGELOG.md}"
[ -n "$version" ] || { echo "usage: scripts/ci/release-notes.sh <version> [CHANGELOG]" >&2; exit 2; }
bash "$here/version.sh" --check-version "$version"
[ -f "$changelog" ] || { echo "no changelog: $changelog" >&2; exit 1; }

section="$(awk -v version="$version" '
  $0 ~ "^## " version "([[:space:]]|$)" { inside = 1; next }
  inside && /^## / { exit }
  inside { print }
' "$changelog")"

if [ -z "$(printf '%s' "$section" | tr -d '[:space:]')" ]; then
  echo "ERROR: CHANGELOG.md has no non-empty section for $version" >&2
  exit 1
fi

date="${RELEASE_DATE:-$(date -u +%Y-%m-%d)}"

cat <<EOF
# Sarthi Studio v$version

Released $date.

## Changes

$(printf '%s\n' "$section" | sed '/^[[:space:]]*$/d')

## Supported platforms

| Platform | Artifact | Validation |
|---|---|---|
| Linux x86_64 | \`.deb\`, portable tarball, AppImage | package payload booted and served HTTP; system install not run in CI |
| Windows x86_64 | portable zip, MSI | built and MSI payload tested on Windows CI; install/launch not run |
| macOS universal | \`.app.zip\`, universal tarball | universal binary built on macOS CI; **unsigned, not notarized** |
| Android | APK, AAB | built and emulator-tested; **release artifacts unsigned unless signed at distribution** |
| Web | served by the local backend | loopback Chromium and security-header checks |

## Verify the download

- Compare every file with \`SHA256SUMS.txt\`:
  \`sha256sum -c SHA256SUMS.txt\` (Linux/macOS).
- \`release-manifest.json\` records the version, source commit, workflow run and each artifact's SHA-256.
- \`sbom.cdx.json\` is a CycloneDX 1.5 dependency inventory.
- Windows Authenticode and macOS Developer ID/notarization are **not configured**; do not treat those artifacts as signed.

## Install and upgrade

- Linux: install the \`.deb\` or unpack the tarball. User data lives under the user's home and is not touched by package upgrades.
- Windows: run the MSI; it is a per-user install under \`%LOCALAPPDATA%\` and replaces prior versions. It is unsigned.
- macOS: unzip the app bundle. Gatekeeper will require an explicit Open because the bundle is unsigned.
- Android: install the APK after signing it with the project's release keystore.
- Conversations, settings, projects and credentials are preserved across upgrades; see \`INSTALLER_UPDATE_VALIDATION.md\`.

## Known limitations

- No production signing credentials are present for Windows or macOS.
- Android release artifacts are unsigned until the distributor signs them.
- Notifications, tray integration and deep links are not implemented.
- Physical Windows/macOS installation and upgrade validation is still blocked.
EOF
