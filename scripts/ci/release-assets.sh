#!/usr/bin/env bash
# The files a release promises, one per line, for a given version.
#
# The asset set used to exist twice: each build job passed its own expected names
# to scripts/ci/verify-artifacts.sh, and the final gate in verify-release.sh held
# a second copy of the same list. A rename applied to one side only was invisible
# until a whole tagged run had built every platform and failed at the last job.
# Both sides read this file now, and scripts/ci/verify-release-contract.sh proves
# on every push that the workflow's job lists add up to exactly this set.
#
# Usage: scripts/ci/release-assets.sh <version>

set -euo pipefail

version="${1:-}"
if [ -z "$version" ]; then
  echo "usage: scripts/ci/release-assets.sh <version>" >&2
  exit 2
fi

# Every line becomes a filename. A space, a path separator or a control character
# in the version would make an unsafe name (the carriage-return class of bug), so
# the version is refused here rather than downstream at upload time.
case "$version" in
  *[[:space:]]* | */* | *\\*)
    echo "version '$version' cannot be used in an artifact name" >&2
    exit 2
    ;;
esac
if [ "$(printf '%s' "$version" | tr -d '[:cntrl:]')" != "$version" ]; then
  echo "version '$version' contains a control character" >&2
  exit 2
fi

cat <<EOF
SAARTHI-STUDIO-android.aab
SAARTHI-STUDIO-android.apk
SAARTHI-STUDIO-vscode.vsix
SAARTHI-STUDIO_${version}-1_amd64.deb
SAARTHI-STUDIO_${version}_linux-x86_64.tar.gz
SAARTHI-STUDIO_${version}_macos-universal.app.zip
SAARTHI-STUDIO_${version}_macos-universal.tar.gz
SAARTHI-STUDIO_${version}_windows-x86_64.msi
SAARTHI-STUDIO_${version}_windows-x86_64.zip
SAARTHI-STUDIO_${version}_x86_64.AppImage
SHA256SUMS.txt
EOF
