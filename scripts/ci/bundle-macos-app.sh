#!/usr/bin/env bash
# Build a real macOS application bundle (`.app`) and a distributable zip of it.
#
# A tarball hands a macOS user a binary to run by hand. A `.app` is what Finder,
# Launch Services, the Dock and `open` understand: one double-clickable product
# with a name, an identifier and a version the OS reports. It also fixes the
# asset layout — the binary lives in `Contents/MacOS`, the UI in
# `Contents/Resources` — which the app resolves as `<exe>/../Resources/static`.
#
# Usage: scripts/ci/bundle-macos-app.sh <stage-dir> <version> <out-dir> [app-name]
#   stage-dir  a tree with the binary and `static/` beside it (the release
#              staging step's output)
#   version    the version from scripts/ci/version.sh; re-validated here
#   out-dir    where the bundle and its zip are written
#   app-name   bundle name, default "Sarthi Studio"
#
# It refuses rather than guessing: a missing binary, a static tree without
# `index.html`, or an unusable version fails with the reason instead of
# producing a bundle that cannot start.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

stage="${1:-}"
version="${2:-}"
out="${3:-}"
app_name="${4:-Sarthi Studio}"
bundle_id="com.saarthi.freeaistudio"
minimum_macos="10.15"

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

[ -n "$stage" ] && [ -n "$version" ] && [ -n "$out" ] ||
  die "usage: scripts/ci/bundle-macos-app.sh <stage-dir> <version> <out-dir> [app-name]"
[ -d "$stage" ] || die "no such staged tree: $stage"
bash "$here/version.sh" --check-version "$version" ||
  die "unusable version '$version' (expected MAJOR.MINOR.PATCH)"

binary="$(find "$stage" -maxdepth 1 -type f \( -name 'free-ai-studio' -o -name 'free-ai-studio.exe' \) 2>/dev/null | head -n 1)"
[ -n "$binary" ] || die "$stage contains no free-ai-studio binary"
[ -f "$stage/static/index.html" ] ||
  die "$stage/static/index.html is missing, so the bundle would serve no UI"

app="$out/$app_name.app"
# A rebuild replaces the tree: a file left over from an earlier bundle must not
# ride along into the next one.
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

executable="$(basename "$binary")"
cp "$binary" "$app/Contents/MacOS/$executable"
chmod +x "$app/Contents/MacOS/$executable"
cp -R "$stage/static" "$app/Contents/Resources/static"

cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>$app_name</string>
  <key>CFBundleExecutable</key><string>$executable</string>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$app_name</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>$minimum_macos</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Classic bundle marker: package type plus four signature bytes.
printf 'APPL????' >"$app/Contents/PkgInfo"

# Check the result before it is zipped: a bundle whose pieces do not line up is
# not a release artifact, and an executable bit lost in the copy is exactly what
# makes a double-click fail with no message.
[ -f "$app/Contents/MacOS/$executable" ] || die "the bundle has no executable"
[ -x "$app/Contents/MacOS/$executable" ] || die "the bundled executable is not executable"
[ -f "$app/Contents/Resources/static/index.html" ] ||
  die "the bundle has no UI in Contents/Resources/static"
grep -q "<string>$version</string>" "$app/Contents/Info.plist" ||
  die "Info.plist does not carry version $version"
grep -q "<string>$executable</string>" "$app/Contents/Info.plist" ||
  die "Info.plist does not name the bundled executable"

mkdir -p "$out"
zip_path="$out/sarthi-studio_${version}_macos-universal.app.zip"
rm -f "$zip_path"

# Python's zipfile keeps the archive names portable (forward slashes) and exists
# everywhere this runs; `zip` is the fallback for a machine without python.
if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  py=""
fi
if [ -n "$py" ]; then
  "$py" - "$app" "$zip_path" <<'EOF'
import os
import sys
import zipfile

root, out = sys.argv[1], sys.argv[2]
base = os.path.dirname(root)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as archive:
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        for name in sorted(filenames):
            full = os.path.join(dirpath, name)
            arcname = os.path.relpath(full, base).replace(os.sep, "/")
            archive.write(full, arcname)
EOF
elif command -v zip >/dev/null 2>&1; then
  (cd "$out" && zip -qry "$(basename "$zip_path")" "$app_name.app")
else
  die "neither python nor zip is available to archive the bundle"
fi

[ -f "$zip_path" ] || die "the bundle zip was not written"
files="$(find "$app" -type f | wc -l | tr -d ' ')"
printf 'bundle %s\n' "$app"
printf 'binary %s (Contents/MacOS/%s)\n' "$executable" "$executable"
printf 'version %s, %s files\n' "$version" "$files"
printf 'zip    %s\n' "$zip_path"
