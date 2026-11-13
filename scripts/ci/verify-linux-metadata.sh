#!/usr/bin/env bash
# Offline check of the Linux package metadata: the AppStream component and the
# .desktop entry must agree with each other and with this build's version.
#
# `appstreamcli validate-tree` (which the AppImage job runs) checks schema and
# naming rules, but a <release> entry for an old version is perfectly valid XML,
# so a package can advertise a version the app no longer is. This check runs on
# every push and before packaging, needs only Python's standard library and does
# not require appstreamcli.
#
# Usage: scripts/ci/verify-linux-metadata.sh [appdata.xml] [desktop] [version]

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
id="io.github.sairajkhilariarch.SarthiStudio"
appdata="${1:-$root/app/packaging/$id.appdata.xml}"
desktop="${2:-$root/app/packaging/$id.desktop}"
version="${3:-}"

if [ -z "$version" ]; then
  version="$(bash "$here/version.sh" "$root/app/Cargo.toml")"
fi

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  printf 'ERROR: python is required to validate the Linux package metadata\n' >&2
  exit 1
fi

"$py" - "$appdata" "$desktop" "$version" <<'PY'
import os
import sys
import xml.etree.ElementTree as ET

appdata, desktop, version = sys.argv[1:4]
errors = []


def text(node, tag):
    found = node.find(tag)
    return (found.text or "").strip() if found is not None else ""


def check(condition, message):
    if not condition:
        errors.append(message)


if not os.path.isfile(appdata):
    print("ERROR: AppStream file not found: %s" % appdata, file=sys.stderr)
    sys.exit(1)
try:
    component = ET.parse(appdata).getroot()
except ET.ParseError as error:
    print("ERROR: AppStream file is not well-formed XML: %s" % error, file=sys.stderr)
    sys.exit(1)

check(component.tag == "component", "the AppStream root is <%s>, not <component>" % component.tag)
check(
    component.get("type") == "desktop-application",
    "the component type is not desktop-application",
)
component_id = text(component, "id")
# AppStream names the file after the component, with either of two suffixes.
stem = os.path.basename(appdata)
for suffix in (".appdata.xml", ".metainfo.xml", ".xml"):
    if stem.endswith(suffix):
        stem = stem[: -len(suffix)]
        break
check(
    component_id == stem,
    "the component id %r does not match the file name" % component_id,
)
check(bool(text(component, "name")), "the component has no <name>")
summary = text(component, "summary")
check(bool(summary), "the component has no <summary>")
check(not summary.endswith("."), "the <summary> ends with a period, which AppStream refuses")
check(
    text(component, "metadata_license") in ("MIT", "CC0-1.0", "FSFAP"),
    "the <metadata_license> is not one AppStream recommends",
)
check(bool(text(component, "project_license")), "the component has no <project_license>")
check(
    component.find("./description/p") is not None,
    "the <description> has no paragraph",
)
check(component.find("content_rating") is not None, "the component has no <content_rating>")

launchable = component.find("./launchable[@type='desktop-id']")
check(launchable is not None, "the component has no desktop-id <launchable>")
if launchable is not None:
    check(
        (launchable.text or "").strip() == os.path.basename(desktop),
        "the <launchable> names %r, not the desktop file %r"
        % ((launchable.text or "").strip(), os.path.basename(desktop)),
    )

releases = component.findall("./releases/release")
check(bool(releases), "the component lists no <release>")
if releases:
    newest = (releases[0].get("version") or "").strip()
    check(
        newest == version,
        "the newest <release> is %r but this build is %r" % (newest, version),
    )
    check(bool((releases[0].get("date") or "").strip()), "the newest <release> has no date")

if not os.path.isfile(desktop):
    print("ERROR: desktop file not found: %s" % desktop, file=sys.stderr)
    sys.exit(1)
entry = {}
group = None
seen_entry = False
with open(desktop, encoding="utf-8") as handle:
    for raw in handle:
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            group = line
            seen_entry = seen_entry or line == "[Desktop Entry]"
            continue
        if group == "[Desktop Entry]" and "=" in line:
            key, value = line.split("=", 1)
            entry[key.strip()] = value.strip()

check(seen_entry, "the desktop file has no [Desktop Entry] group")
check(entry.get("Type") == "Application", "the desktop entry Type is not Application")
check(bool(entry.get("Name")), "the desktop entry has no Name")
check(bool(entry.get("Exec")), "the desktop entry has no Exec")
check(bool(entry.get("Icon")), "the desktop entry has no Icon")
categories = entry.get("Categories", "")
check(bool(categories), "the desktop entry has no Categories")
check(
    all(part.endswith(";") for part in categories.split() if part),
    "every desktop Categories entry must end with a semicolon",
)

binary = text(component, "provides/binary")
if entry.get("Exec") and binary:
    program = os.path.basename(entry["Exec"].split()[0])
    check(
        program == binary,
        "the desktop Exec runs %r but the component provides %r" % (program, binary),
    )

icon = entry.get("Icon")
if icon:
    icon_file = os.path.join(os.path.dirname(desktop), "icons", icon + "-256.png")
    check(
        os.path.isfile(icon_file),
        "the desktop icon %r has no packaged %s-256.png beside it" % (icon, icon),
    )

if errors:
    for message in errors:
        print("ERROR: %s" % message, file=sys.stderr)
    sys.exit(1)

print("Linux package metadata OK: %s" % version)
PY
