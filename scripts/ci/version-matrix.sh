#!/usr/bin/env bash
# Report and validate every version line in the repository.
#
# Desktop and Android have independent release histories, and the website is
# explicitly on its own line. They must still be internally consistent:
#   - desktop Cargo.toml == newest AppStream release == a CHANGELOG section;
#   - Android versionName == README version, and versionCode == README code;
#   - VS Code package.json == package-lock root version;
#   - website package.json == package-lock root version.
#
# Usage: scripts/ci/version-matrix.sh [--check]

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
check=0
[ "${1:-}" = "--check" ] && check=1

fail() { printf 'FAIL %s\n' "$1" >&2; exit 1; }

app="$(bash "$here/version.sh" "$root/app/Cargo.toml")"
appdata="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' "$root/app/packaging/io.github.sairajkhilariarch.SarthiStudio.appdata.xml" | head -n1)"
grep -q "^## $app\$" "$root/CHANGELOG.md" || fail "CHANGELOG.md has no section for desktop $app"
[ "$app" = "$appdata" ] || fail "AppStream newest release is $appdata, desktop is $app"

android_name="$(sed -n 's/.*versionName = "\([^"]*\)".*/\1/p' "$root/android/app/build.gradle.kts" | head -n1)"
android_code="$(sed -n 's/.*versionCode = \([0-9]*\).*/\1/p' "$root/android/app/build.gradle.kts" | head -n1)"
bash "$here/version.sh" --check-version "$android_name" || fail "Android versionName is not semver"
grep -q "Version \`$android_name\` (version code \`$android_code\`)" "$root/README.md" ||
  fail "README does not document Android $android_name (code $android_code)"

if command -v python3 >/dev/null 2>&1; then py=python3; else py=python; fi
json_version() { "$py" - "$1" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
print(data.get("version", ""))
PY
}
json_lock_version() { "$py" - "$1" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
print((data.get("packages") or {}).get("", {}).get("version", data.get("version", "")))
PY
}

vscode="$(json_version "$root/vscode-extension/package.json")"
vscode_lock="$(json_lock_version "$root/vscode-extension/package-lock.json")"
bash "$here/version.sh" --check-version "$vscode" || fail "VS Code extension version is not semver"
[ "$vscode" = "$vscode_lock" ] || fail "VS Code package.json $vscode != package-lock $vscode_lock"

website="$(json_version "$root/website/package.json")"
website_lock="$(json_lock_version "$root/website/package-lock.json")"
bash "$here/version.sh" --check-version "$website" || fail "website version is not semver"
[ "$website" = "$website_lock" ] || fail "website package.json $website != package-lock $website_lock"

printf '%-24s %-10s %s\n' LINE VERSION STATE
printf '%-24s %-10s %s\n' desktop "$app" "authoritative (tag must match)"
printf '%-24s %-10s %s\n' "android" "$android_name" "versionCode $android_code (independent)"
printf '%-24s %-10s %s\n' "vscode-extension" "$vscode" "independent"
printf '%-24s %-10s %s\n' "website" "$website" "independent"
[ "$check" = "1" ] && echo "version matrix consistent"
