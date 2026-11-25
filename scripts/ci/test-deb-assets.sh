#!/usr/bin/env bash
# Every runtime file under app/static must be listed in the cargo-deb assets.
#
# The 3.88.x .deb listed only index.html, app.js and styles.css. The UI also
# loads ide.js, highlight.js, replace.js and ide.css, so an installed package
# booted into a page whose workbench, syntax highlighting and buttons were
# missing. Static tests did not read the package manifest; this one does.
#
# app/static/tests is excluded because tests are not runtime assets.
#
# Usage: scripts/ci/test-deb-assets.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
manifest="$root/app/Cargo.toml"

[ -f "$manifest" ] || { echo "missing $manifest" >&2; exit 1; }

missing=0
checked=0
while IFS= read -r file; do
  relative="${file#"$root/app/"}"
  checked=$((checked + 1))
  if ! grep -qF "[\"$relative\"" "$manifest"; then
    echo "FAIL .deb assets omit $relative" >&2
    missing=$((missing + 1))
  fi
done < <(find "$root/app/static" -type f ! -path '*/tests/*' | sort)

if [ "$missing" -ne 0 ]; then
  echo "Add the missing files to [package.metadata.deb].assets in app/Cargo.toml." >&2
  exit 1
fi

# The files the browser loads directly are the ones whose absence bricked the
# installed UI; name them so a future rewrite of the find expression cannot
# hide the regression.
for required in index.html app.js ide.js highlight.js replace.js styles.css ide.css; do
  grep -qF "[\"static/$required\"" "$manifest" || {
    echo "FAIL .deb assets omit static/$required" >&2
    exit 1
  }
done

echo "ok   all $checked runtime static files are packaged in the .deb"
