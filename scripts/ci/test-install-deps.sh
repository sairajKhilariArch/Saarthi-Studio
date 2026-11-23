#!/usr/bin/env bash
# The source installer builds Tauri, so a missing mold/pkg-config/GTK/WebKit
# package used to surface as a compiler or linker error that never named the
# package. This test runs the real install.sh against a fake PATH: every
# dependency is present, then mold is removed, then --deps-only resolves it.
#
# Usage: scripts/ci/test-install-deps.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

bin="$tmp/bin"
mkdir -p "$bin"

stub() { # name body
  printf '#!/usr/bin/env bash\n%s\n' "$2" >"$bin/$1"
  chmod +x "$bin/$1"
}

for command in git curl cc mold pkg-config openssl xdg-open docker; do
  stub "$command" "exit 0"
done
stub rustc 'echo "rustc 1.90.0 (fake)"'
stub cargo 'echo "cargo 1.90.0 (fake)"'
stub pkg-config 'if [[ "${1:-}" == "--exists" ]]; then exit 0; fi; echo 1.0'
stub docker 'if [[ "${1:-}" == "compose" ]]; then exit 0; fi; exit 0'
# The resolution path normally uses sudo. Returning root from id -u exercises
# the same apt code without touching the real package manager.
stub id 'if [[ "${1:-}" == "-u" ]]; then echo 0; else /usr/bin/id "$@"; fi'
stub apt-get 'if [[ "${1:-}" == "install" ]]; then printf "#!/usr/bin/env bash\nexit 0\n" >"$(dirname "$0")/mold"; chmod +x "$(dirname "$0")/mold"; fi; exit 0'

run_installer() { # logfile args...
  local log="$1"
  shift
  HOME="$tmp/home" PATH="$bin:/usr/bin:/bin" bash "$root/install.sh" "$@" >"$log" 2>&1
}

fail() {
  printf 'FAIL %s\n' "$1" >&2
  exit 1
}

run_installer "$tmp/present.log" --check \
  || fail "a complete fake PATH was reported as missing dependencies"
grep -q "All dependencies are present." "$tmp/present.log" \
  || fail "the present case did not report success"

rm -f "$bin/mold"
if run_installer "$tmp/missing.log" --check; then
  fail "a missing mold was accepted"
fi
grep -q "mold — linking the app" "$tmp/missing.log" \
  || fail "the missing case did not name mold"
grep -q "apt-get install -y mold" "$tmp/missing.log" \
  || fail "the missing case did not name the package that provides mold"

run_installer "$tmp/resolve.log" --deps-only \
  || fail "--deps-only could not resolve the missing mold"
test -x "$bin/mold" || fail "--deps-only did not install the missing command"
grep -q "Dependencies are installed." "$tmp/resolve.log" \
  || fail "--deps-only did not report success after resolving the dependency"

printf 'installer dependency checks passed\n'
