#!/usr/bin/env bash
# Every release job must read the version through the one parser.
#
# v1.26.0 and v1.26.1 shipped no usable asset because a job extracted the
# version with its own `sed`: a carriage return from a CRLF checkout reached the
# artifact filename, and the macOS job's copy of the same expression was
# over-escaped. `scripts/ci/version.sh` fixed the parsing (strips CR/LF/tab/space,
# refuses an empty or malformed value), but nothing stopped a job from growing a
# second reader again. This check reads the release workflow and fails, naming
# the step, when one does.
#
# Usage: scripts/ci/verify-version-steps.sh [workflow]
#   workflow  default: .github/workflows/desktop-release.yml

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

workflow="${1:-$root/.github/workflows/desktop-release.yml}"
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

"$py" - "$workflow" <<'PY'
import re
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    text = handle.read()

# Split on step boundaries, then keep the steps that publish the version.
steps = re.split(r"\n(?=\s*- name:)", text)
meta = [step for step in steps if re.search(r"^\s*id:\s*meta\s*$", step, re.M)]

problems = []
if not meta:
    problems.append("no step produces the version (expected a step with `id: meta`)")
for step in meta:
    name = re.sub(
        r"^-\s*name:\s*", "", step.strip().splitlines()[0].strip()
    )
    if "scripts/ci/version.sh" not in step:
        problems.append(f"step \"{name}\" does not call scripts/ci/version.sh")

# A step that reads the manifest itself is the v1.26.1 class of bug: the parser
# has to be the one place that knows about CRLF, whitespace and semver.
inline = [
    (r"sed\s+-n\s+'s/\^version", "an inline `sed` version reader"),
    (r"grep\s+-[^\n]*\^version\s*=", "an inline `grep` version reader"),
    (r"Select-String[^\n]*version\s*=", "an inline PowerShell version reader"),
]
for pattern, label in inline:
    for match in re.finditer(pattern, text):
        line = text.count("\n", 0, match.start()) + 1
        problems.append(f"line {line} has {label}")

# The release has four jobs that each need the version (preflight, deb, Windows,
# macOS); fewer than that means one of them stopped publishing it through the
# shared parser rather than that a job was removed deliberately.
if len(meta) < 4:
    problems.append(
        f"only {len(meta)} version step(s); the release has four jobs that need one"
    )

if problems:
    print(
        f"ERROR: {path} would read the release version outside scripts/ci/version.sh:",
        file=sys.stderr,
    )
    for problem in problems:
        print(f"  {problem}", file=sys.stderr)
    sys.exit(1)

print(f"release version steps use one parser: {len(meta)} steps")
PY
