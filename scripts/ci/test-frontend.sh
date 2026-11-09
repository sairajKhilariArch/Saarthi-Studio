#!/usr/bin/env bash
# Frontend unit tests for the static UI.
#
# `app/static/` has no build step and no package manager, so there is nothing to
# install: `app/static/tests/harness.js` evaluates the real `app.js` in a
# `node:vm` context with an inert DOM, and the tests assert the pure helpers
# (escaping, markdown, formatting, path handling) that the browser actually
# runs. Node's own test runner is the whole toolchain.
#
# Usage: scripts/ci/test-frontend.sh   (needs Node 18+; CI pins 20)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

if ! command -v node >/dev/null 2>&1; then
  echo "node is required to run the frontend tests" >&2
  exit 1
fi

node_major="$(node -p 'process.versions.node.split(".")[0]')"
if [ "$node_major" -lt 18 ]; then
  echo "node 18 or newer is required (found $(node --version))" >&2
  exit 1
fi

cd "$root"
# A glob rather than a directory: `node --test <dir>` resolves the directory as
# a module on some versions instead of walking it.
exec node --test "app/static/tests/*.test.js"
