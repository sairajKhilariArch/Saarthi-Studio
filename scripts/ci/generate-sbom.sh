#!/usr/bin/env bash
# Generate a CycloneDX 1.5 SBOM from the locked Rust dependency graph.
#
# This intentionally uses the committed Cargo.lock, not `cargo metadata`: the
# SBOM must describe what a release actually built, including the exact crate
# versions and registry checksums, without needing a network or a toolchain.
# A deterministic serial number makes repeated runs on the same version/commit
# comparable.
#
# Usage: scripts/ci/generate-sbom.sh <output.json> [app/Cargo.lock]
#   SOURCE_DATE_EPOCH  optional, for a reproducible timestamp
#   GITHUB_SHA         recorded as the source commit when present

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="${1:-}"
lock="${2:-$root/app/Cargo.lock}"
[ -n "$out" ] || { echo "usage: scripts/ci/generate-sbom.sh <output.json> [Cargo.lock]" >&2; exit 2; }
[ -f "$lock" ] || { echo "no such lockfile: $lock" >&2; exit 1; }

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  echo "python is required to generate the SBOM" >&2
  exit 1
fi

version="$(bash "$here/version.sh" "$root/app/Cargo.toml")"
commit="${GITHUB_SHA:-$(git -C "$root" rev-parse HEAD 2>/dev/null || echo unknown)}"
mkdir -p "$(dirname "$out")"

"$py" - "$lock" "$out" "$version" "$commit" <<'PY'
import datetime
import hashlib
import json
import os
import sys
import uuid

lock_path, out_path, version, commit = sys.argv[1:5]

try:
    import tomllib
except ImportError:  # Python < 3.11
    tomllib = None

if tomllib:
    with open(lock_path, "rb") as handle:
        lock = tomllib.load(handle)
    packages = lock.get("package", [])
else:
    # Minimal fallback for the two fields this SBOM needs.
    packages = []
    current = None
    with open(lock_path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line == "[[package]]":
                if current:
                    packages.append(current)
                current = {}
            elif current is not None and "=" in line:
                key, _, value = line.partition("=")
                key = key.strip()
                if key in {"name", "version", "checksum"}:
                    current[key] = value.strip().strip('"')
    if current:
        packages.append(current)

components = []
for package in sorted(packages, key=lambda item: (item.get("name", ""), item.get("version", ""))):
    name = str(package.get("name") or "").strip()
    crate_version = str(package.get("version") or "").strip()
    if not name or not crate_version:
        continue
    component = {
        "type": "library",
        "bom-ref": f"pkg:cargo/{name}@{crate_version}",
        "name": name,
        "version": crate_version,
        "purl": f"pkg:cargo/{name}@{crate_version}",
    }
    checksum = str(package.get("checksum") or "").strip()
    if checksum:
        component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
    components.append(component)

if not components:
    print("ERROR: Cargo.lock produced no SBOM components", file=sys.stderr)
    sys.exit(1)

serial_seed = f"sarthi-studio|{version}|{commit}"
serial = uuid.uuid5(uuid.NAMESPACE_URL, serial_seed)
epoch = os.environ.get("SOURCE_DATE_EPOCH")
if epoch and epoch.isdigit():
    timestamp = datetime.datetime.fromtimestamp(int(epoch), datetime.timezone.utc)
else:
    timestamp = datetime.datetime.now(datetime.timezone.utc)
timestamp = timestamp.replace(microsecond=0).isoformat().replace("+00:00", "Z")

bom = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "serialNumber": f"urn:uuid:{serial}",
    "version": 1,
    "metadata": {
        "timestamp": timestamp,
        "tools": [{"vendor": "Sarthi Studio", "name": "generate-sbom.sh", "version": "1"}],
        "component": {
            "type": "application",
            "bom-ref": f"pkg:cargo/free-ai-studio@{version}",
            "name": "free-ai-studio",
            "version": version,
            "purl": f"pkg:cargo/free-ai-studio@{version}",
        },
        "properties": [{"name": "source.commit", "value": commit}],
    },
    "components": components,
}

encoded = json.dumps(bom, indent=2, sort_keys=True) + "\n"
if any(secret in encoded for secret in ("sk-live-", "ghp_", "BEGIN PRIVATE KEY")):
    print("ERROR: SBOM contains a secret-shaped value", file=sys.stderr)
    sys.exit(1)
with open(out_path, "w", encoding="utf-8") as handle:
    handle.write(encoded)
print(f"SBOM: {len(components)} Rust components for {version} -> {out_path}")
PY
