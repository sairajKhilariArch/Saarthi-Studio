#!/usr/bin/env bash
# End-to-end smoke: a real headless binary, a real config file, a real SQLite
# database, a stand-in provider, and a restart in between.
#
# Everything below the HTTP surface is real. The only fixture is
# `scripts/mock-router.py`, which answers `/v1/chat/completions` the way a router
# does; the studio is launched exactly as a user's would be, pointed at a
# scratch `FREE_AI_STUDIO_HOME` so the run can never touch real settings.
#
# The flow asserts the properties a unit test cannot: that the binary boots,
# binds the port it was told to, answers a chat turn through a provider, writes
# the turn to disk, and still has it after the process is killed and restarted.
#
# Usage: scripts/ci/test-e2e.sh
#   E2E_BINARY=<path>   binary to run (default: the debug build)
#   E2E_NO_BUILD=1      fail instead of building a missing binary
#   E2E_KEEP=1          keep the scratch home and logs for inspection
#
# Needs: curl, python3 (or `python`), and a built binary.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

passed=0
failed=0
pass() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
die() {
  failed=$((failed + 1))
  printf 'FAIL %s\n' "$1" >&2
}

work="$(mktemp -d)"
studio_pid=""
router_pid=""

cleanup() {
  stop_studio
  if [ -n "$router_pid" ]; then kill "$router_pid" 2>/dev/null || true; fi
  if [ "${E2E_KEEP:-0}" = "1" ]; then
    printf '\nkept %s\n' "$work"
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

# -- prerequisites ----------------------------------------------------------

if command -v python3 >/dev/null 2>&1; then
  py=python3
elif command -v python >/dev/null 2>&1; then
  py=python
else
  echo "python is required to run the mock provider" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

binary="${E2E_BINARY:-$root/app/target/debug/free-ai-studio}"
[ -f "$binary.exe" ] && binary="$binary.exe"
if [ ! -f "$binary" ]; then
  if [ "${E2E_NO_BUILD:-0}" = "1" ]; then
    echo "no binary at $binary (unset E2E_NO_BUILD to build it)" >&2
    exit 1
  fi
  echo "building the debug binary (this is the slowest step of the run)"
  (cd "$root/app" && cargo build --locked --no-default-features) || {
    echo "build failed" >&2
    exit 1
  }
fi
[ -f "$binary" ] || {
  echo "no binary at $binary after the build" >&2
  exit 1
}

# A free port, chosen by binding one and letting go: the studio and the mock both
# need one, and hard-coded ports collide with whatever the machine already runs.
free_port() {
  "$py" - <<'EOF'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
EOF
}

port="$(free_port)"
mock_port="$(free_port)"
base="http://127.0.0.1:$port"
log="$work/studio.log"

# One scratch home holds everything the app writes: config, database, process
# log. `FREE_AI_STUDIO_HOME` is the documented sandbox root, so a bug here can
# never reach the developer's real install.
export FREE_AI_STUDIO_HOME="$work/home"
mkdir -p "$FREE_AI_STUDIO_HOME/.config/free-ai-studio"

cat >"$FREE_AI_STUDIO_HOME/.config/free-ai-studio/config.json" <<EOF
{
  "base_url": "http://127.0.0.1:$mock_port/v1",
  "model": "mock-model",
  "api_key": "e2e-not-a-real-key"
}
EOF

# -- helpers ----------------------------------------------------------------

# GET/POST a URL and leave the body in $work/body, echoing the status code.
http() { # method, url, [json]
  local method="$1" url="$2" data="${3:-}"
  if [ -n "$data" ]; then
    curl -sS -o "$work/body" -w '%{http_code}' -X "$method" \
      -H 'content-type: application/json' -d "$data" "$url" 2>/dev/null
  else
    curl -sS -o "$work/body" -w '%{http_code}' "$url" 2>/dev/null
  fi
}

# A field from the last response body, by dotted path ("meta.model").
field() { # dotted-path
  "$py" - "$work/body" "$1" <<'EOF'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
for part in sys.argv[2].split("."):
    value = value[int(part)] if isinstance(value, list) else value.get(part)
    if value is None:
        break
print("" if value is None else value)
EOF
}

# Block until the studio answers /api/ready, or give up. A port that accepts
# connections is not readiness: the database and data directory are checked
# there, and a failure in either is a real failure, not a slow boot.
wait_ready() {
  local tries=0
  while [ "$tries" -lt 60 ]; do
    if [ "$(http GET "$base/api/ready")" = "200" ]; then
      return 0
    fi
    tries=$((tries + 1))
    sleep 0.5
  done
  return 1
}

# The stored status of a run, read through the same public route the panel
# uses, so the poll below observes what a client would.
run_state() { # run id
  http GET "$base/api/runs/$1" >/dev/null
  field status
}

wait_run_state() { # run id, wanted, tries
  local id="$1" wanted="$2" limit="${3:-60}" tries=0 state
  while [ "$tries" -lt "$limit" ]; do
    state="$(run_state "$id")"
    [ "$state" = "$wanted" ] && return 0
    case "$state" in
      failed | cancelled | interrupted) return 2 ;;
    esac
    tries=$((tries + 1))
    sleep 0.25
  done
  return 1
}

step_actions() { # comma-joined action tags of the run in $work/body
  "$py" -c 'import json,sys;print(",".join(str(s.get("action","")) for s in json.load(open(sys.argv[1],encoding="utf-8")).get("stepList") or []))' "$work/body"
}

start_studio() {
  ( cd "${FREE_AI_STUDIO_HOME}" && exec "$binary" --headless ) \
    >>"$log" 2>&1 &
  studio_pid=$!
}

stop_studio() {
  [ -n "$studio_pid" ] || return 0
  kill "$studio_pid" 2>/dev/null || true
  # On Windows a native child can outlive the shell's kill; the port's owner is
  # the definitive PID there.
  if command -v netstat >/dev/null 2>&1; then
    local winpid
    winpid="$(netstat -ano 2>/dev/null | awk -v p=":$port" '$0 ~ p && /LISTENING/ {print $NF; exit}')"
    if [ -n "$winpid" ]; then taskkill //PID "$winpid" //F >/dev/null 2>&1 || true; fi
  fi
  # Wait for the port to go quiet, so the restart below binds the same one.
  local tries=0
  while [ "$tries" -lt 40 ]; do
    if ! http GET "$base/api/health" >/dev/null 2>&1; then break; fi
    tries=$((tries + 1))
    sleep 0.25
  done
  studio_pid=""
}

# -- run --------------------------------------------------------------------

"$py" "$root/scripts/mock-router.py" --port "$mock_port" >>"$work/mock.log" 2>&1 &
router_pid=$!

# The mock binds and answers within a few hundred milliseconds; polling is still
# the only honest check, because a started process is not a listening one.
mock_tries=0
until [ "$(http GET "http://127.0.0.1:$mock_port/api/ping")" = "200" ]; do
  mock_tries=$((mock_tries + 1))
  if [ "$mock_tries" -gt 40 ]; then break; fi
  sleep 0.25
done
if [ "$mock_tries" -le 40 ]; then
  pass "mock provider answers"
else
  die "mock provider did not start (see $work/mock.log)"
fi

export FREE_AI_STUDIO_PORT="$port"
export FREE_AI_STUDIO_SKIP_ROUTER=1
start_studio

if wait_ready; then
  pass "headless studio boots and reports ready on $port"
else
  die "studio never became ready (log follows)"
  tail -20 "$log" >&2
  tail -20 "$work/mock.log" >&2
  exit 1
fi

if [ "$(field ready)" = "True" ] && [ "$(field ok)" = "True" ]; then
  pass "readiness reports ready, with the database and data directory required"
else
  die "readiness did not report ready ($(cat "$work/body"))"
fi

# A conversation, then a turn through the mock provider.
code="$(http POST "$base/api/conversations" '{"title":"e2e run"}')"
cid="$(field id)"
if [ "$code" = "201" ] && [ -n "$cid" ]; then
  pass "conversation created ($cid)"
else
  die "could not create a conversation (HTTP $code: $(cat "$work/body"))"
  exit 1
fi

code="$(http POST "$base/api/chat" "{\"conversation_id\":\"$cid\",\"message\":\"hello from the e2e run\"}")"
answer="$(field answer)"
if [ "$code" = "200" ] && [ "$(field ok)" = "True" ]; then
  pass "chat turn answered through the provider (model $(field meta.model))"
else
  die "chat turn failed (HTTP $code: $(cat "$work/body"))"
fi
case "$answer" in
  *"Mock router reply"*) pass "the provider's answer is the stored reply" ;;
  *) die "unexpected answer: $answer" ;;
esac

if [ "$(http GET "$base/api/conversations/$cid/messages")" = "200" ]; then
  count="$("$py" -c 'import json,sys;print(len(json.load(open(sys.argv[1]))))' "$work/body")"
  if [ "$count" = "2" ]; then
    pass "the turn is stored as a user message and a reply"
  else
    die "expected 2 stored messages, found $count"
  fi
else
  die "messages endpoint failed for $cid"
fi

# Every user action leaves a process-log line; the boot is one of them, so this
# also proves the log file was created beside the database in the scratch home.
if [ "$(http GET "$base/api/diagnostics/log")" = "200" ]; then
  entries="$(field count)"
  if [ "${entries:-0}" -gt 0 ]; then
    pass "process log recorded the run ($entries entries)"
  else
    die "process log is empty after boot and a turn"
  fi
else
  die "process log endpoint failed"
fi

# -- agent run --------------------------------------------------------------
# The half a chat turn cannot prove: a run that reads its goal, asks for a
# write, waits at the approval gate, executes it, is verified, and leaves the
# file on disk. The provider's answers are scripted in mock-router.py; every
# step below goes through the real routes and the real run loop.
ws="$work/ws"
mkdir -p "$ws"
# The binary is native, so a Git Bash `/tmp/...` path is not an absolute path to
# it. `cygpath -m` keeps forward slashes, which also need no JSON escaping.
ws_native="$(cygpath -m "$ws" 2>/dev/null || printf '%s' "$ws")"

code="$(http POST "$base/api/workspace/open" "{\"path\":\"$ws_native\"}")"
if [ "$code" = "200" ] && [ "$(field ok)" = "True" ]; then
  pass "workspace folder opened for the agent"
else
  die "could not open a workspace folder (HTTP $code: $(cat "$work/body"))"
  exit 1
fi

code="$(http POST "$base/api/runs" '{"goal":"e2e-agent: write the marker file","allowWrites":true,"profile":"coding","maxSteps":6}')"
run_id="$(field run.id)"
if [ "$code" = "200" ] && [ -n "$run_id" ]; then
  pass "agent run created ($run_id)"
else
  die "could not create an agent run (HTTP $code: $(cat "$work/body"))"
  exit 1
fi

if wait_run_state "$run_id" awaiting_approval 80; then
  pass "the run stops at the write approval gate"
else
  die "run never asked for approval (state $(run_state "$run_id")): $(cat "$work/body")"
fi

code="$(http POST "$base/api/runs/$run_id/approve" '{"approve":true}')"
if [ "$code" = "200" ]; then
  pass "the waiting write was approved"
else
  die "approving the write failed (HTTP $code: $(cat "$work/body"))"
fi

tries=0
state=""
while [ "$tries" -lt 120 ]; do
  state="$(run_state "$run_id")"
  case "$state" in
    succeeded | failed | cancelled | interrupted) break ;;
  esac
  tries=$((tries + 1))
  sleep 0.25
done
if [ "$state" = "succeeded" ]; then
  pass "the agent run finished after verification"
else
  die "run ended $state: $(cat "$work/body")"
fi

if [ -f "$ws/e2e-agent.txt" ] && grep -q 'written by the agent' "$ws/e2e-agent.txt"; then
  pass "the approved write landed in the workspace on disk"
else
  die "the agent's file is missing or wrong"
fi

http GET "$base/api/runs/$run_id" >/dev/null
actions="$(step_actions)"
case "$actions" in
  *write*verify*done*) pass "the run log keeps write, verify and done steps" ;;
  *) die "run steps are not write, verify, done: $actions" ;;
esac

# -- restart ----------------------------------------------------------------
# The point of the restart: state must come from disk, not from memory.
stop_studio
start_studio

if wait_ready; then
  pass "studio restarts on the same port and home"
else
  die "studio did not come back up (log follows)"
  tail -20 "$log" >&2
  exit 1
fi

if [ "$(http GET "$base/api/conversations/$cid/messages")" = "200" ]; then
  count="$("$py" -c 'import json,sys;print(len(json.load(open(sys.argv[1]))))' "$work/body")"
  if [ "$count" = "2" ]; then
    pass "the conversation survived the restart"
  else
    die "expected 2 messages after restart, found $count"
  fi
else
  die "the conversation is gone after the restart"
fi

# The database file itself must exist where the app documents it, or a user
# backing up that path would silently lose their history.
if [ -f "$FREE_AI_STUDIO_HOME/.local/share/free-ai-studio/history.sqlite3" ]; then
  pass "history is in the documented database path"
else
  die "no database at the documented path"
fi

# The run is the harder half of persistence: its row, its steps and its
# approval outcome all have to come back from the database, not from memory.
if [ "$(http GET "$base/api/runs/$run_id")" = "200" ] && [ "$(field status)" = "succeeded" ]; then
  if [ -n "$(field stepList.0.action)" ]; then
    pass "the agent run and its steps survived the restart"
  else
    die "the run came back with no steps after the restart"
  fi
else
  die "the agent run did not survive the restart"
fi

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ] || exit 1
