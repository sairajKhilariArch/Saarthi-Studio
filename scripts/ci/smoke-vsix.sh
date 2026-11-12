#!/usr/bin/env bash
# Install smoke: prove a packaged VS Code extension can be installed and
# activated by VS Code.
#
# A .vsix is a zip, so this reads it the way the marketplace client does: the
# manifest that carries the identity and version, the package.json VS Code
# activates from, the `main` file that file points at, and the engine floor that
# decides whether an install is even offered. A package that builds but ships no
# activation entry, ships a version the manifest disagrees with, or carries a
# secret file fails here instead of failing for a user.
#
# Usage: scripts/ci/smoke-vsix.sh <artifact.vsix> [expected-version]
#   SMOKE_KEEP=1   keep the unpacked tree

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

artifact="${1:-}"
expected="${2:-}"
[ -n "$artifact" ] || {
  echo "usage: scripts/ci/smoke-vsix.sh <artifact.vsix> [expected-version]" >&2
  exit 2
}
[ -f "$artifact" ] || {
  echo "no such artifact: $artifact" >&2
  exit 1
}

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  echo "python is required to read a .vsix" >&2
  exit 1
fi

# The extension carries its own version line (2.6.0), separate from the desktop
# app's, so the default expectation is read from its package.json.
if [ -z "$expected" ]; then
  expected="$("$py" - "$root/vscode-extension/package.json" <<'PY'
import json, sys
try:
    print(json.load(open(sys.argv[1], encoding="utf-8")).get("version", ""))
except OSError:
    print("")
PY
)"
fi

work="$(mktemp -d)"
cleanup() {
  if [ "${SMOKE_KEEP:-0}" = "1" ]; then
    printf '\nkept %s\n' "$work"
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

"$py" - "$artifact" "$expected" "$work" <<'PY'
import json
import re
import sys
import zipfile

path, expected, work = sys.argv[1:4]
passed = 0
failed = 0


def pass_(what):
    global passed
    passed += 1
    print(f"ok   {what}")


def die(what):
    global failed
    failed += 1
    print(f"FAIL {what}", file=sys.stderr)


def summary():
    print(f"\n{passed} passed, {failed} failed")
    sys.exit(0 if failed == 0 else 1)


try:
    archive = zipfile.ZipFile(path)
except (OSError, zipfile.BadZipFile) as error:
    die(f"{path} is not a readable zip: {error}")
    summary()

names = set(archive.namelist())
size = sum(info.file_size for info in archive.infolist())
pass_(f"opened the package ({len(names)} entries, {size} bytes before compression)")

# 1. The manifest the marketplace and `code --install-extension` read.
manifest_name = "extension.vsixmanifest"
if manifest_name not in names:
    die(f"the package has no {manifest_name}, so VS Code cannot install it")
else:
    manifest = archive.read(manifest_name).decode("utf-8", "replace")
    identity = re.search(r'<Identity\b[^>]*\bVersion="([^"]*)"', manifest)
    if not identity:
        die(f"{manifest_name} has no Identity version")
    else:
        pass_(f"manifest identity version is {identity.group(1)}")

# 2. The extension's own package.json, which VS Code activates from.
package_name = "extension/package.json"
if package_name not in names:
    die(f"the package has no {package_name}")
    summary()
try:
    package = json.loads(archive.read(package_name).decode("utf-8"))
except (UnicodeDecodeError, json.JSONDecodeError) as error:
    die(f"{package_name} is not readable JSON: {error}")
    summary()

for field in ("name", "publisher", "version"):
    if not str(package.get(field) or "").strip():
        die(f"{package_name} has no {field}")
if not re.fullmatch(r"\d+\.\d+\.\d+([-+][0-9A-Za-z.]+)?", str(package.get("version", ""))):
    die(f"{package_name} version {package.get('version')!r} is not MAJOR.MINOR.PATCH")
else:
    pass_(f"{package['publisher']}.{package['name']} declares version {package['version']}")

if expected and str(package.get("version")) != expected:
    die(f"{package_name} says {package.get('version')}, expected {expected}")
elif expected:
    pass_(f"version matches the source package.json ({expected})")

engine = (package.get("engines") or {}).get("vscode")
if not engine:
    die(f"{package_name} declares no engines.vscode, so no install is offered")
else:
    pass_(f"declares its VS Code floor ({engine})")

# 3. The activation entry has to be inside the package. `main` is relative to the
# extension folder, and a missing file means the extension never activates.
main = str(package.get("main") or "").strip()
if not main:
    die(f"{package_name} declares no main entry, so the extension cannot activate")
else:
    entry = "extension/" + main.lstrip("./")
    if entry not in names:
        die(f"the declared main entry {main} is not in the package")
    elif archive.getinfo(entry).file_size == 0:
        die(f"the declared main entry {main} is empty")
    else:
        pass_(f"activation entry {main} is present ({archive.getinfo(entry).file_size} bytes)")

# The manifest and package.json must name the same version: the marketplace
# shows one and activates the other.
if manifest_name in names:
    identity_version = re.search(
        r'<Identity\b[^>]*\bVersion="([^"]*)"',
        archive.read(manifest_name).decode("utf-8", "replace"),
    )
    if identity_version and identity_version.group(1) != str(package.get("version")):
        die(
            f"{manifest_name} says {identity_version.group(1)} but {package_name} "
            f"says {package.get('version')}"
        )
    elif identity_version:
        pass_("manifest and package.json agree on the version")

# 4. A published package must not carry local secrets.
secret_patterns = (
    re.compile(r"(^|/)\.env($|\.)"),
    re.compile(r"(^|/)\.dev\.vars$"),
    re.compile(r"(^|/)id_(rsa|ed25519|ecdsa)"),
    re.compile(r"\.(pem|p12|pfx)$"),
)
leaked = [name for name in names if any(p.search(name) for p in secret_patterns)]
if leaked:
    die("the package carries secret-shaped files: " + ", ".join(sorted(leaked)[:5]))
else:
    pass_("no secret-shaped files are packaged")

# 5. Unpack it the way an install does, so a path the zip lists but cannot write
# is caught here rather than at install time.
try:
    archive.extractall(work)
    pass_("unpacks cleanly")
except (OSError, zipfile.BadZipFile) as error:
    die(f"could not unpack: {error}")

summary()
PY
