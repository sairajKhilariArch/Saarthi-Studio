#!/usr/bin/env bash
# Install smoke: prove a packaged Android app can be installed and will start.
#
# An APK is a zip whose parts an installer checks: a compiled
# AndroidManifest.xml, at least one dex, resources and a signature block.
# The app is native Java — there is no React Native runtime, so no
# JavaScript bundle and no required native libraries; instead the launch
# activity's class must be carried by the dex, or the app crashes at open.
# An AAB carries the same parts under `base/`. A release build that
# packages but ships no code or no signature crashes or refuses to install
# on a device, so this fails here instead.
#
# Usage: scripts/ci/smoke-apk.sh <artifact.apk|.aab> [another artifact...]
#   SMOKE_KEEP=1   keep the unpacked trees
#
# What this deliberately does not do: read `versionName` out of the APK. That
# lives in the binary manifest and needs aapt/apkanalyzer; where those are
# absent the version is reported from the project file, labelled as such, rather
# than guessed from the artifact name.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

[ "$#" -gt 0 ] || {
  echo "usage: scripts/ci/smoke-apk.sh <artifact.apk|.aab> [another artifact...]" >&2
  exit 2
}

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  echo "python is required to read an APK" >&2
  exit 1
fi

# The Android app keeps its own version line; report it beside the artifact so a
# mismatch is visible even though it is not read from the package. The namespace
# names the launch activity the dex must carry.
project_meta="$("$py" - "$root/android/app/build.gradle.kts" <<'PY'
import re, sys
try:
    text = open(sys.argv[1], encoding="utf-8").read()
except OSError:
    sys.exit()
version = re.search(r'versionName\s*=\s*"([^"]+)"', text)
namespace = re.search(r'namespace\s*=\s*"([^"]+)"', text)
print(version.group(1) if version else "")
print(namespace.group(1) if namespace else "")
PY
)"
project_version="$(printf '%s\n' "$project_meta" | sed -n 1p)"
project_namespace="$(printf '%s\n' "$project_meta" | sed -n 2p)"

work="$(mktemp -d)"
cleanup() {
  if [ "${SMOKE_KEEP:-0}" = "1" ]; then
    printf '\nkept %s\n' "$work"
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

status=0
for artifact in "$@"; do
  [ -f "$artifact" ] || {
    echo "no such artifact: $artifact" >&2
    status=1
    continue
  }
  printf '\n== %s ==\n' "$(basename "$artifact")"
  "$py" - "$artifact" "$work" "$project_version" "$project_namespace" <<'PY' || status=1
import os
import re
import sys
import zipfile

path, work, project_version, project_namespace = sys.argv[1:5]
label = path.rsplit("/", 1)[-1].rsplit("\\", 1)[-1]
bundle = label.lower().endswith(".aab")
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


try:
    archive = zipfile.ZipFile(path)
except (OSError, zipfile.BadZipFile) as error:
    die(f"{label} is not a readable zip: {error}")
    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1)

names = set(archive.namelist())
total = sum(info.file_size for info in archive.infolist())
pass_(f"opened the package ({len(names)} entries, {total} bytes before compression)")

root_prefix = "base/" if bundle else ""
manifest = f"{root_prefix}manifest/AndroidManifest.xml" if bundle else "AndroidManifest.xml"


def require(name, what):
    """True when the entry exists and is not empty."""
    if name not in names:
        die(f"no {name} ({what})")
        return False
    if archive.getinfo(name).file_size == 0:
        die(f"{name} is empty ({what})")
        return False
    pass_(f"{what}: {name} ({archive.getinfo(name).file_size} bytes)")
    return True


# 1. The manifest every installer parses before it will touch the app.
require(manifest, "compiled manifest")
if not bundle and manifest in names:
    head = archive.read(manifest)[:4]
    # RES_XML_TYPE (0x0003) with the standard 8-byte chunk header. A text
    # AndroidManifest.xml in an APK is a packaging mistake that fails at install.
    if head != b"\x03\x00\x08\x00":
        die("AndroidManifest.xml is not compiled binary XML")
    else:
        pass_("manifest is compiled binary XML")

# 2. Code: at least one dex, all of which the runtime loads.
dex = sorted(
    name
    for name in names
    if re.fullmatch(rf"{re.escape(root_prefix)}(dex/)?classes\d*\.dex", name)
)
if not dex:
    die("no classes.dex, so the app has no code")
else:
    empty = [name for name in dex if archive.getinfo(name).file_size == 0]
    if empty:
        die("empty dex entries: " + ", ".join(empty))
    else:
        pass_(f"{len(dex)} dex file(s), all non-empty")

# 3. Resources: an APK carries resources.arsc, an AAB a proto resources table.
resources = "base/resources.pb" if bundle else "resources.arsc"
require(resources, "resources table")

# 4. The launch activity must be packaged. The manifest names .MainActivity
# and the runtime resolves it from the dex; a release that minified it away
# crashes at open. The app is native Java, so there is no JavaScript bundle.
if project_namespace:
    descriptor = f"L{project_namespace.replace('.', '/')}/MainActivity;".encode()
    if any(descriptor in archive.read(name) for name in dex):
        pass_(f"launch activity {project_namespace}.MainActivity is carried by the dex")
    else:
        die(f"no dex carries {project_namespace}.MainActivity, so the app cannot start")
else:
    die("cannot read the namespace from android/app/build.gradle.kts")

# 5. Native libraries are optional for a native Java app; any that ship must
# be non-empty, since an empty .so is a packaging corruption.
libs = sorted(name for name in names if re.fullmatch(rf"{re.escape(root_prefix)}lib/[^/]+/.+\.so", name))
abis = sorted({name.split("/")[1 if not bundle else 2] for name in libs})
if libs:
    empty = [name for name in libs if archive.getinfo(name).file_size == 0]
    if empty:
        die("empty native libraries: " + ", ".join(empty[:5]))
    else:
        pass_(f"{len(libs)} native libraries across {', '.join(abis)}")
else:
    pass_("no native libraries shipped (none required by the native Java app)")

# 6. An APK must be signed or `adb install` refuses it. CI builds an unsigned
# diagnostic release because no production keystore exists; SMOKE_ALLOW_UNSIGNED=1
# validates its structure while explicitly reporting that it is not installable
# as a production artifact. Signed builds still require a v1 or v2/v3 block.
if not bundle:
    signatures = [
        name
        for name in names
        if re.fullmatch(r"META-INF/[^/]+\.(RSA|DSA|EC)", name, re.IGNORECASE)
    ]
    if signatures:
        pass_(f"signed (v1: {', '.join(sorted(signatures)[:2])})")
    else:
        raw = open(path, "rb").read()
        eocd = raw.rfind(b"PK\x05\x06")
        schemes = []
        if eocd >= 0:
            cd = int.from_bytes(raw[eocd + 16:eocd + 20], "little")
            if raw[cd - 16:cd] == b"APK Sig Block 42":
                size = int.from_bytes(raw[cd - 24:cd - 16], "little")
                pos = cd - size
                while pos < cd - 24:
                    plen = int.from_bytes(raw[pos:pos + 8], "little")
                    pid = int.from_bytes(raw[pos + 8:pos + 12], "little")
                    if pid in (0x7109871A, 0xF05368C0, 0x1B93AD61):
                        schemes.append({0x7109871A: "v2", 0xF05368C0: "v3", 0x1B93AD61: "v3.1"}[pid])
                    pos += 8 + plen
        if schemes:
            pass_(f"signed (APK Signature Scheme {', '.join(schemes)})")
        elif os.environ.get("SMOKE_ALLOW_UNSIGNED") == "1":
            pass_("UNSIGNED diagnostic build (allowed by SMOKE_ALLOW_UNSIGNED; not installable as production)")
        else:
            die("no META-INF signature block and no v2/v3 signing block, so the APK is unsigned and will not install")
else:
    require("BundleConfig.pb", "bundle config")

if project_version:
    print(f"info {label} is for Android versionName {project_version} (read from the project, not the package)")

print(f"\n{passed} passed, {failed} failed")
sys.exit(0 if failed == 0 else 1)
PY
done

exit "$status"
