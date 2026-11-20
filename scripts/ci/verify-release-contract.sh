#!/usr/bin/env bash
# Prove that the release workflow builds exactly the assets the release promises.
#
# Each build job tells scripts/ci/verify-artifacts.sh which files it must produce,
# and scripts/ci/verify-release.sh demands the whole set from the published
# release. Those two lists are independent copies of one contract, so a rename
# applied to one side (or a job that simply stops promising a file) only surfaced
# after a tagged run had built every platform and reached the final job. This
# check reads the workflow's job lists and the contract in
# scripts/ci/release-assets.sh and fails, naming each side's difference, before
# anything is built.
#
# Usage: scripts/ci/verify-release-contract.sh [workflow] [version]
#   workflow  default: .github/workflows/desktop-release.yml
#   version   default: the version in app/Cargo.toml

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

workflow="${1:-$root/.github/workflows/desktop-release.yml}"
version="${2:-$(bash "$here/version.sh" "$root/app/Cargo.toml")}"

[ -f "$workflow" ] || {
  echo "no such workflow: $workflow" >&2
  exit 2
}

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  echo "python is required to read the workflow" >&2
  exit 2
fi

assets="$(mktemp)"
trap 'rm -f "$assets"' EXIT
bash "$here/release-assets.sh" "$version" >"$assets"

"$py" - "$workflow" "$version" "$assets" <<'PY'
import re
import sys

workflow, version, assets_path = sys.argv[1:4]
with open(workflow, encoding="utf-8") as handle:
    text = handle.read()

# A step's command may be split over several lines with a trailing backslash; join
# those first so a name on a continuation line is read as part of its step.
joined = []
buffer = ""
for raw in text.splitlines():
    line = raw.strip()
    if buffer:
        buffer = f"{buffer} {line}"
    elif "verify-artifacts.sh" in line:
        buffer = line
    else:
        continue
    if not buffer.endswith("\\"):
        joined.append(buffer)
        buffer = ""
if buffer:
    joined.append(buffer)

promised = set()
for line in joined:
    match = re.search(r"verify-artifacts\.sh\s+dist\b(.*)$", line)
    if not match:
        continue
    # The version is a workflow expression in the real file and a literal here.
    expression = match.group(1).replace("${{ steps.meta.outputs.version }}", version)
    expression = expression.replace("${{ needs.preflight.outputs.version }}", version)
    for quoted, bare in re.findall(r'"([^"]*)"|(\S+)', expression):
        name = (quoted or bare).strip().strip("\\").strip()
        if name:
            promised.add(name)

with open(assets_path, encoding="utf-8") as handle:
    contract = {line.strip() for line in handle if line.strip()}

# A job that reads steps.meta.outputs must define the meta step itself. The deb
# job once referenced it without one, the expression resolved to nothing, and
# the artifact gate demanded a versionless filename after a successful build.
jobs_section = text.split("\njobs:", 1)[1]
for block in re.split(r"\n  (?=[A-Za-z][\w-]*:)", jobs_section):
    if not block.strip():
        continue
    job_name = block.strip().splitlines()[0].rstrip(":")
    if "steps.meta.outputs" in block and "id: meta" not in block:
        print(
            f"ERROR: job '{job_name}' reads steps.meta.outputs but defines no 'id: meta' step",
            file=sys.stderr,
        )
        sys.exit(1)
    # Under a pwsh default shell a backslash is not a line continuation: a
    # multi-line bash command must declare shell: bash, or the command's own
    # arguments arrive broken (the Windows gate received a literal backslash
    # as an expected artifact name and failed after a successful build).
    if "shell: pwsh" in block:
        for step in re.split(r"\n      - name:", block):
            if "run: |" not in step or "shell: bash" in step:
                continue
            if any(line.rstrip().endswith("\\") for line in step.splitlines()):
                step_name = step.strip().splitlines()[0]
                print(
                    f"ERROR: job '{job_name}' step '{step_name}' uses bash line "
                    "continuations under a pwsh default shell without 'shell: bash'",
                    file=sys.stderr,
                )
                sys.exit(1)

unbuilt = sorted(contract - promised)  # promised by the release, no job makes it
ungated = sorted(promised - contract)  # a job makes it, the release gate ignores it

if unbuilt:
    print(
        f"ERROR: {len(unbuilt)} release asset(s) have no build job:",
        file=sys.stderr,
    )
    for name in unbuilt:
        print(f"  {name}", file=sys.stderr)
if ungated:
    print(
        f"ERROR: {len(ungated)} job artifact(s) are not in the release contract:",
        file=sys.stderr,
    )
    for name in ungated:
        print(f"  {name}", file=sys.stderr)
if unbuilt or ungated:
    print(
        f"the workflow promises {len(promised)} artifact(s), "
        f"the contract names {len(contract)}",
        file=sys.stderr,
    )
    sys.exit(1)

print(
    f"release contract holds for {version}: {len(contract)} assets, "
    f"every one built by a job and demanded by the release gate"
)
PY
