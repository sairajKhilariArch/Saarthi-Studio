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
# A case a platform cannot enforce is skipped out loud, never counted as a pass.
skip() { printf 'skip %s\n' "$1"; }

# The state of one readiness check by name ("yes", "no" or "missing") from the
# body of the last /api/ready call.
ready_check() { # check name
  "$py" - "$work/body" "$1" <<'EOF'
import json, sys
try:
    payload = json.load(open(sys.argv[1], encoding="utf-8"))
except Exception:
    print("unknown")
    raise SystemExit
for check in payload.get("checks") or []:
    if check.get("name") == sys.argv[2]:
        print("yes" if check.get("ok") else "no")
        break
else:
    print("missing")
EOF
}

work="$(mktemp -d)"
studio_pid=""
router_pid=""
term_stream=""

cleanup() {
  stop_studio
  if [ -n "$router_pid" ]; then kill "$router_pid" 2>/dev/null || true; fi
  if [ -n "$term_stream" ]; then kill "$term_stream" 2>/dev/null || true; fi
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

# A version probe must print and exit before the app resolves a home or opens
# the database. It gets a scratch home and a deadline, so a regression cannot
# touch real data and cannot hang the suite.
expected_version="$(bash "$root/scripts/ci/version.sh" "$root/app/Cargo.toml")"
probe_home="$work/version-probe-home"
FREE_AI_STUDIO_HOME="$probe_home" "$binary" --version >"$work/version.txt" 2>/dev/null &
probe_pid=$!
probe_wait=0
while kill -0 "$probe_pid" 2>/dev/null && [ "$probe_wait" -lt 40 ]; do
  probe_wait=$((probe_wait + 1))
  sleep 0.05
done
if kill -0 "$probe_pid" 2>/dev/null; then
  kill "$probe_pid" 2>/dev/null || true
  wait "$probe_pid" 2>/dev/null || true
  die "--version did not exit; a probe must not start the server"
else
  wait "$probe_pid" 2>/dev/null
  probe_status=$?
  version_out="$(cat "$work/version.txt")"
  case "$version_out" in
    *"$expected_version"*)
      if [ "$probe_status" = "0" ] && [ ! -e "$probe_home" ]; then
        pass "--version reports $expected_version, exits, and creates no home"
      else
        die "--version status=$probe_status; created a home: $([ -e "$probe_home" ] && echo yes || echo no)"
      fi ;;
    *) die "--version printed '$version_out', expected $expected_version" ;;
  esac
fi

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
    if isinstance(value, list):
        # A missing index is an empty field, not a traceback: the checks above
        # use this to ask "is there a first step yet?".
        index = int(part)
        if index >= len(value):
            value = None
            break
        value = value[index]
    else:
        value = value.get(part)
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

# DNS rebinding makes an attacker's hostname resolve to this loopback listener.
# A foreign Host must not reach the core routes that are registered directly in
# main.rs, not only the module routers that carry their own guard.
code="$(curl -sS -o "$work/body" -w '%{http_code}' \
  -H "Host: evil.example:$port" "$base/api/config" 2>/dev/null)"
if [ "$code" = "403" ] && grep -q 'loopback' "$work/body"; then
  pass "a foreign Host cannot read the core config route"
else
  die "a foreign Host reached /api/config (HTTP $code: $(cat "$work/body"))"
fi

code="$(curl -sS -o "$work/body" -w '%{http_code}' \
  -H "Host: 127.0.0.1:$port" -H 'Origin: http://evil.example' \
  "$base/api/capabilities" 2>/dev/null)"
if [ "$code" = "403" ] && grep -q 'Cross-site' "$work/body"; then
  pass "a foreign Origin cannot read the core capabilities route"
else
  die "a foreign Origin reached /api/capabilities (HTTP $code: $(cat "$work/body"))"
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

# The streaming route is a separate body machine from /api/chat and has its own
# partial-reply recorder. A router that answers plain JSON (the mock does) must
# still become one delta plus a done frame, and the reply must be stored once.
code="$(http POST "$base/api/chat/stream" "{\"conversation_id\":\"$cid\",\"message\":\"stream this turn\"}")"
if [ "$code" = "200" ] && grep -q '"delta"' "$work/body" && grep -q '"done":true' "$work/body"; then
  pass "streaming chat emits a delta and a done frame"
else
  die "streaming chat failed (HTTP $code: $(head -c 300 "$work/body"))"
fi

if [ "$(http GET "$base/api/conversations/$cid/messages")" = "200" ]; then
  count="$("$py" -c 'import json,sys;print(len(json.load(open(sys.argv[1]))))' "$work/body")"
  if [ "$count" = "4" ]; then
    pass "the streamed turn is stored exactly once"
  else
    die "expected 4 stored messages after streaming, found $count"
  fi
else
  die "messages endpoint failed after streaming for $cid"
fi

# The checks above use a router that ignores `stream` and answers one JSON body.
# A real event-stream takes a different branch: several deltas, each recorded as
# it arrives, and a client abort that must keep the part already received.
code="$(http POST "$base/api/conversations" '{"title":"real sse"}')"
sse_conv="$(field id)"
curl -sS -N --max-time 30 -o "$work/sse.body" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"conversation_id\":\"$sse_conv\",\"message\":\"stream real sse\",\"model\":\"mock-sse\"}" \
  "$base/api/chat/stream" >"$work/sse.code" 2>/dev/null
sse_code="$(cat "$work/sse.code")"
sse_deltas="$(grep -c '"delta"' "$work/sse.body" || true)"
if [ "$sse_code" = "200" ] && [ "${sse_deltas:-0}" -ge 2 ] &&
   grep -q '"done":true' "$work/sse.body" && ! grep -q '"error"' "$work/sse.body"; then
  pass "a real event-stream turn emits multiple deltas and one done frame ($sse_deltas deltas)"
else
  die "real event-stream turn failed (HTTP $sse_code, $sse_deltas deltas: $(head -c 300 "$work/sse.body"))"
fi

if [ "$(http GET "$base/api/conversations/$sse_conv/messages")" = "200" ]; then
  sse_counts="$("$py" - "$work/body" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1], encoding="utf-8"))
print(len(rows), len([r for r in rows if r.get("role") == "assistant"]))
PY
)"
  if [ "$sse_counts" = "2 1" ]; then
    pass "the real event-stream reply is stored exactly once"
  else
    die "expected 2 messages and 1 reply after a real event stream, found: $sse_counts"
  fi
else
  die "messages endpoint failed after a real event stream"
fi

# Stop generation: the fetch is aborted after the first delta while the provider
# is still holding the stream open. The recorder must save what the user saw,
# exactly once, and the studio must stay usable.
code="$(http POST "$base/api/conversations" '{"title":"stopped stream"}')"
stop_conv="$(field id)"
curl -sS -N --max-time 3 -o "$work/stopped.body" \
  -H 'content-type: application/json' \
  -d "{\"conversation_id\":\"$stop_conv\",\"message\":\"stop this stream\",\"model\":\"mock-slow-stream\"}" \
  "$base/api/chat/stream" >/dev/null 2>&1 || true
if grep -q '"delta"' "$work/stopped.body"; then
  pass "the stopped stream had already delivered a delta"
else
  die "the slow stream delivered no delta before the abort"
fi

stopped=""
tries=0
while [ "$tries" -lt 40 ]; do
  if [ "$(http GET "$base/api/conversations/$stop_conv/messages")" = "200" ]; then
    stopped="$("$py" - "$work/body" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1], encoding="utf-8"))
replies = [r for r in rows if r.get("role") == "assistant"]
print(len(replies), (replies[0].get("content") or "") if replies else "")
PY
)"
  fi
  # One assistant row with text is the success state: zero means the recorder
  # has not flushed yet, more than one means it saved the same turn twice.
  case "$stopped" in
    "1 "*) break ;;
  esac
  tries=$((tries + 1))
  sleep 0.25
done
stopped_count="${stopped%% *}"
stopped_text="${stopped#* }"
if [ "$stopped_count" = "1" ] && [ -n "$stopped_text" ]; then
  pass "stopping mid-stream stored the partial reply exactly once"
else
  die "the stopped stream stored '$stopped_count' replies ('$stopped_text')"
fi

code="$(http POST "$base/api/chat" "{\"conversation_id\":\"$stop_conv\",\"message\":\"after the stop\"}")"
if [ "$code" = "200" ] && grep -q 'Mock router reply' "$work/body"; then
  pass "the studio keeps answering after a stopped stream"
else
  die "chat failed after a stopped stream (HTTP $code: $(head -c 200 "$work/body"))"
fi

if [ "$(http POST "$base/api/conversations/$cid/update" '{"token_budget":5000}')" = "200" ] &&
   [ "$(field token_budget)" = "5000" ]; then
  pass "a conversation token budget is accepted and returned"
else
  die "conversation budget update failed ($(cat "$work/body"))"
fi

# Deleting a thread removes its messages. A second thread is used so the
# restart checks below still have the original history to find.
code="$(http POST "$base/api/conversations" '{"title":"delete me"}')"
doomed="$(field id)"
delete_code="$(http POST "$base/api/conversations/$doomed/delete" '{}')"
messages_code="$(http GET "$base/api/conversations/$doomed/messages")"
if [ "$code" = "201" ] && [ -n "$doomed" ] &&
   [ "$delete_code" = "200" ] && [ "$messages_code" = "404" ]; then
  pass "a deleted conversation and its messages are gone"
else
  die "conversation deletion did not cascade for $doomed (create=$code delete=$delete_code messages=$messages_code: $(cat "$work/body"))"
fi

# Config is read back without its secrets: the UI only needs to know whether a
# key exists, and a raw key must never be serialized to the browser again.
if [ "$(http GET "$base/api/config")" = "200" ] &&
   [ "$(field has_api_key)" = "True" ] &&
   ! grep -q 'e2e-not-a-real-key' "$work/body"; then
  pass "the config endpoint reports a saved key without returning it"
else
  die "config redaction failed ($(cat "$work/body"))"
fi

# Project CRUD is the scope every conversation and memory record depends on.
code="$(http POST "$base/api/projects" '{"name":"e2e project"}')"
project_id="$(field id)"
rename_code="$(http POST "$base/api/projects/$project_id/update" '{"name":"e2e renamed"}')"
list_code="$(http GET "$base/api/projects")"
if [ "$code" = "201" ] && [ -n "$project_id" ] && [ "$rename_code" = "200" ] &&
   [ "$list_code" = "200" ] && grep -q 'e2e renamed' "$work/body" &&
   [ "$(http POST "$base/api/projects/$project_id/delete" '{}')" = "200" ] &&
   [ "$(http GET "$base/api/projects")" = "200" ] && ! grep -q 'e2e renamed' "$work/body"; then
  pass "a project can be created, renamed, listed and deleted"
else
  die "project CRUD failed (create=$code rename=$rename_code list=$list_code: $(cat "$work/body"))"
fi

# Memory is real SQLite storage, not an in-memory hint list.
code="$(http POST "$base/api/memory" '{"kind":"fact","title":"e2e memory","body":"the audit marker"}')"
memory_id="$(field record.id)"
if [ "$code" = "200" ] && [ -n "$memory_id" ] &&
   [ "$(http GET "$base/api/memory")" = "200" ] && grep -q 'e2e memory' "$work/body"; then
  pass "a memory record is created and listed"
else
  die "memory create/list failed (create=$code id=$memory_id: $(cat "$work/body"))"
fi

# Recall reaches the streaming meta even when no skill auto-matched: the two
# fields are independent, and the reply must say it used the memory.
code="$(http POST "$base/api/conversations" '{"title":"memory stream"}')"
memory_conv="$(field id)"
stream_code="$(http POST "$base/api/chat/stream" "{\"conversation_id\":\"$memory_conv\",\"message\":\"what does the e2e memory say?\"}")"
if [ "$code" = "201" ] && [ -n "$memory_conv" ] && [ "$stream_code" = "200" ] &&
   grep -q '"memory_used"' "$work/body"; then
  pass "streaming metadata reports the memory that was used"
else
  die "streaming memory metadata missing (conversation=$code stream=$stream_code: $(head -c 400 "$work/body"))"
fi
http POST "$base/api/conversations/$memory_conv/delete" '{}' >/dev/null

if [ "$(http POST "$base/api/memory/$memory_id/delete" '{}')" = "200" ] &&
   [ "$(http GET "$base/api/memory")" = "200" ] && ! grep -q 'e2e memory' "$work/body"; then
  pass "a deleted memory record leaves the list"
else
  die "memory deletion failed (id=$memory_id: $(cat "$work/body"))"
fi

# A workflow is stored in a file, exported as the documented document, and
# removed without leaving a schedule claim or a ghost entry behind.
code="$(http POST "$base/api/workflows" '{"name":"e2e workflow","prompt":"say hello"}')"
workflow_id="$(field id)"
export_code="$(http GET "$base/api/workflows/$workflow_id/export")"
if [ "$code" = "200" ] && [ -n "$workflow_id" ] && [ "$export_code" = "200" ] &&
   grep -q 'sarthi.workflow' "$work/body" &&
   [ "$(http POST "$base/api/workflows/$workflow_id/delete" '{}')" = "200" ] &&
   [ "$(http GET "$base/api/workflows")" = "200" ] && ! grep -q 'e2e workflow' "$work/body"; then
  pass "a workflow is saved, exported and deleted"
else
  die "workflow CRUD failed (create=$code id=$workflow_id export=$export_code: $(cat "$work/body"))"
fi

# The built-in skill catalogue is served by the same router as user skills.
if [ "$(http GET "$base/api/skills")" = "200" ] &&
   "$py" - "$work/body" <<'PY'
import json, sys
payload = json.load(open(sys.argv[1], encoding="utf-8"))
raise SystemExit(0 if isinstance(payload.get("skills"), list) else 1)
PY
then
  pass "the skill catalogue is readable"
else
  die "skills endpoint failed ($(head -c 300 "$work/body"))"
fi

# A remote MCP server must use TLS. The panel can warn, but the server is the
# authority: plain http is accepted only for a loopback endpoint, and an old
# mcp.json is refused when a connection is opened.
code="$(http POST "$base/api/mcp/servers" '{"name":"e2e-remote","transport":"http","url":"http://example.com/mcp"}')"
if [ "$code" = "400" ] && grep -q 'Use https://' "$work/body"; then
  pass "a remote MCP server without TLS is refused"
else
  die "remote cleartext MCP was not refused (HTTP $code: $(cat "$work/body"))"
fi

code="$(http POST "$base/api/mcp/servers" "{\"name\":\"e2e-loopback\",\"transport\":\"http\",\"url\":\"http://127.0.0.1:$mock_port/mcp\"}")"
remove_code="$(http POST "$base/api/mcp/servers/remove" '{"name":"e2e-loopback"}')"
if [ "$code" = "200" ] && [ "$remove_code" = "200" ] && [ "$(field removed)" = "1" ]; then
  pass "loopback MCP over http is allowed and removable"
else
  die "loopback MCP save/remove failed (save=$code remove=$remove_code: $(cat "$work/body"))"
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

# The editor's file lifecycle: create, save, read, rename, delete. Paths are
# resolved inside the open folder, so an escape is refused before I/O.
if [ "$(http POST "$base/api/workspace/create" '{"path":"notes.txt"}')" = "200" ] &&
   [ "$(http PUT "$base/api/workspace/file" '{"path":"notes.txt","content":"first line\\n"}')" = "200" ] &&
   [ "$(http GET "$base/api/workspace/file?path=notes.txt")" = "200" ] &&
   grep -q 'first line' "$work/body"; then
  pass "a workspace file is created, saved and read back"
else
  die "workspace file lifecycle failed ($(cat "$work/body"))"
fi

if [ "$(http POST "$base/api/workspace/rename" '{"from":"notes.txt","to":"renamed.txt"}')" = "200" ] &&
   [ "$(http GET "$base/api/workspace/file?path=notes.txt")" = "404" ] &&
   [ "$(http GET "$base/api/workspace/file?path=renamed.txt")" = "200" ] &&
   [ "$(http POST "$base/api/workspace/delete" '{"path":"renamed.txt"}')" = "200" ] &&
   [ "$(http GET "$base/api/workspace/file?path=renamed.txt")" = "404" ]; then
  pass "a workspace file is renamed and deleted"
else
  die "workspace rename/delete failed ($(cat "$work/body"))"
fi

code="$(http GET "$base/api/workspace/file?path=..%2Foutside.txt")"
if [ "$code" = "400" ] && ! grep -q 'outside.txt' "$work/body"; then
  pass "a path outside the open folder is refused"
else
  die "path traversal was not refused (HTTP $code: $(cat "$work/body"))"
fi

# Interactive terminal: a real PTY, a command entered over HTTP, and a file on
# disk proving the command ran rather than merely being echoed by the shell.
case "$(uname -s 2>/dev/null || echo unknown)" in
  MINGW* | MSYS* | CYGWIN*)
    skip "the interactive terminal smoke is POSIX-only (PowerShell redirection differs)"
    ;;
  *)
    code="$(http POST "$base/api/terminal/open" "{\"cwd\":\"$ws_native\",\"cols\":80,\"rows\":24}")"
    term_id="$(field id)"
    if [ "$code" = "200" ] && [ -n "$term_id" ]; then
      curl -sS -N --max-time 8 "$base/api/terminal/$term_id/stream" >"$work/terminal.sse" 2>/dev/null &
      term_stream=$!
      sleep 0.4
      payload="$("$py" -c 'import json,sys; print(json.dumps({"data": sys.argv[1] + "\r"}))' 'echo e2e-terminal > terminal-proof.txt')"
      input_code="$(http POST "$base/api/terminal/$term_id/input" "$payload")"
      seen=0
      for _ in $(seq 1 40); do
        if [ -f "$ws/terminal-proof.txt" ] && grep -q 'e2e-terminal' "$ws/terminal-proof.txt"; then
          seen=1
          break
        fi
        sleep 0.25
      done
      http POST "$base/api/terminal/$term_id/close" '{}' >/dev/null
      kill "$term_stream" 2>/dev/null || true
      wait "$term_stream" 2>/dev/null || true
      term_stream=""
      if [ "$input_code" = "200" ] && [ "$seen" = "1" ]; then
        pass "a terminal command runs through a real PTY"
      else
        die "terminal did not run the command (open=$code input=$input_code seen=$seen)"
      fi
    else
      die "terminal session did not open (HTTP $code: $(cat "$work/body"))"
    fi
    ;;
esac

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

# A setting saved through the route the Settings panel calls must come back
# from disk after the restart below, not from an in-memory copy.
code="$(http POST "$base/api/config" '{"history_messages":12,"approve":true}')"
if [ "$code" = "200" ]; then
  pass "a setting can be saved through the config route"
else
  die "saving a setting failed (HTTP $code: $(head -c 200 "$work/body"))"
fi

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

if [ "$(http GET "$base/api/config")" = "200" ] && [ "$(field history_messages)" = "12" ]; then
  pass "the saved setting survived the restart"
else
  die "the saved setting did not survive the restart ($(head -c 200 "$work/body"))"
fi

if [ "$(http GET "$base/api/conversations/$cid/messages")" = "200" ]; then
  count="$("$py" -c 'import json,sys;print(len(json.load(open(sys.argv[1]))))' "$work/body")"
  if [ "$count" = "4" ]; then
    pass "the conversation survived the restart"
  else
    die "expected 4 messages after restart, found $count"
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

# -- failure injection ------------------------------------------------------
# The happy path above proves the app works when everything does. These two
# cases prove it ends honestly when it does not: a provider that is not
# answering, and a process killed with a run genuinely in flight.

# 1. Provider outage. The mock is killed, so the run's model call cannot
#    connect; the run must fail with a reason and the studio must stay up.
kill "$router_pid" 2>/dev/null || true
wait "$router_pid" 2>/dev/null || true
router_pid=""
tries=0
while [ "$tries" -lt 20 ]; do
  if [ "$(http GET "http://127.0.0.1:$mock_port/api/ping")" != "200" ]; then break; fi
  tries=$((tries + 1))
  sleep 0.1
done

code="$(http POST "$base/api/runs" '{"goal":"e2e-outage: say hello","maxSteps":3}')"
outage_id="$(field run.id)"
if [ "$code" = "200" ] && [ -n "$outage_id" ]; then
  pass "a run can be started while the provider is down"
else
  die "could not start a run during the outage (HTTP $code: $(cat "$work/body"))"
fi

outage_state=""
tries=0
while [ "$tries" -lt 120 ]; do
  outage_state="$(run_state "$outage_id")"
  case "$outage_state" in
    failed | cancelled | interrupted | succeeded) break ;;
  esac
  tries=$((tries + 1))
  sleep 0.25
done
if [ "$outage_state" = "failed" ]; then
  pass "a run with no provider fails instead of hanging"
else
  die "the run during the outage ended $outage_state"
fi
http GET "$base/api/runs/$outage_id" >/dev/null
case "$(field error)" in
  "") die "the failed run records no error message" ;;
  *) pass "the failed run records the provider error" ;;
esac
if [ "$(http GET "$base/api/ready")" = "200" ]; then
  pass "the studio still answers after a provider outage"
else
  die "the studio stopped answering after the provider went down"
fi

# 2. Killed mid-run. A slow provider keeps the model call in flight, so the
#    kill lands while the run really is working; after the restart the boot
#    sweep must mark it interrupted rather than leave it "running" forever.
"$py" "$root/scripts/mock-router.py" --port "$mock_port" --chat-delay 1.2 >>"$work/mock.log" 2>&1 &
router_pid=$!
tries=0
until [ "$(http GET "http://127.0.0.1:$mock_port/api/ping")" = "200" ]; do
  tries=$((tries + 1))
  if [ "$tries" -gt 40 ]; then break; fi
  sleep 0.25
done

code="$(http POST "$base/api/runs" '{"goal":"e2e-kill: write the marker","allowWrites":true,"autoApprove":true,"maxSteps":6}')"
kill_id="$(field run.id)"
if [ "$code" = "200" ] && [ -n "$kill_id" ]; then
  pass "a slow-provider run was started for the kill case"
else
  die "could not start the kill-case run (HTTP $code: $(cat "$work/body"))"
fi

# Wait for one real step to land and for the next model call to be in flight:
# killing before the first step would leave an empty log, which proves less.
killed_in_flight="no"
tries=0
while [ "$tries" -lt 80 ]; do
  state="$(run_state "$kill_id")"
  case "$state" in
    failed | cancelled | interrupted | succeeded) break ;;
  esac
  if [ "$state" = "running" ]; then
    http GET "$base/api/runs/$kill_id" >/dev/null
    if [ -n "$(field stepList.0.action)" ]; then
      killed_in_flight="yes"
      break
    fi
  fi
  tries=$((tries + 1))
  sleep 0.2
done
if [ "$killed_in_flight" = "yes" ]; then
  pass "the kill lands while the run is working, after its first step"
else
  die "the kill-case run never recorded a step (state $state)"
fi

stop_studio
start_studio
if wait_ready; then
  pass "the studio restarts after being killed mid-run"
else
  die "the studio did not come back after the mid-run kill"
  tail -20 "$log" >&2
  exit 1
fi

killed_state=""
tries=0
while [ "$tries" -lt 80 ]; do
  killed_state="$(run_state "$kill_id")"
  case "$killed_state" in
    interrupted) break ;;
    failed | cancelled | succeeded) break ;;
  esac
  tries=$((tries + 1))
  sleep 0.25
done
if [ "$killed_state" = "interrupted" ]; then
  pass "a run killed mid-flight comes back marked interrupted"
else
  die "the killed run came back as $killed_state"
fi
http GET "$base/api/runs/$kill_id" >/dev/null
if [ -n "$(field stepList.0.action)" ]; then
  pass "the interrupted run's step log is readable after the restart"
else
  die "the interrupted run came back with no steps"
fi

# 3. Corrupt database. A broken file must stop the process with a reason, not
#    boot a half-working server; restoring a saved copy must bring it back.
stop_studio
data_dir="$FREE_AI_STUDIO_HOME/.local/share/free-ai-studio"
db_file="$data_dir/history.sqlite3"
cp "$db_file" "$work/history-good.sqlite3"
printf 'this is not a sqlite database' >"$db_file"
start_studio
exited="no"
tries=0
while [ "$tries" -lt 40 ]; do
  if ! kill -0 "$studio_pid" 2>/dev/null; then
    exited="yes"
    break
  fi
  tries=$((tries + 1))
  sleep 0.25
done
if [ "$exited" = "yes" ]; then
  pass "a corrupt database stops the process instead of booting a half-working server"
else
  die "the studio kept running on a corrupt database"
fi
if grep -qi 'not a database' "$log"; then
  pass "the boot log names the corrupt database"
else
  die "the boot log does not name the database failure"
fi
if [ "$(http GET "$base/api/health")" != "200" ]; then
  pass "nothing answers on the port while the database is corrupt"
else
  die "a server answered on a corrupt database"
fi
studio_pid=""
cp "$work/history-good.sqlite3" "$db_file"
start_studio
if wait_ready; then
  pass "restoring the database brings the studio back ready"
else
  die "the studio did not recover after the database was restored"
  tail -20 "$log" >&2
fi

# 4. Unwritable data directory. This is the write failure a full disk produces,
#    made portable: readiness has to say the directory is not usable, then
#    recover when the permission is put back. Windows Git Bash ignores chmod, so
#    there the case is skipped rather than reported as a pass.
chmod -w "$data_dir" 2>/dev/null || true
enforced="no"
if ! : >"$data_dir/.write-probe" 2>/dev/null; then
  enforced="yes"
else
  rm -f "$data_dir/.write-probe"
fi
if [ "$enforced" = "yes" ]; then
  http GET "$base/api/ready" >/dev/null
  state_of_dir="$(ready_check "data directory")"
  if [ "$state_of_dir" = "no" ]; then
    pass "readiness reports an unwritable data directory"
  else
    die "readiness said '$state_of_dir' about an unwritable data directory"
  fi
  chmod +w "$data_dir" 2>/dev/null || true
  http GET "$base/api/ready" >/dev/null
  if [ "$(ready_check "data directory")" = "yes" ]; then
    pass "readiness recovers when the directory is writable again"
  else
    die "readiness did not recover after the directory was made writable"
  fi
else
  chmod +w "$data_dir" 2>/dev/null || true
  skip "an unwritable data directory (chmod is not enforced on this platform)"
fi

# Not injected, and named so the gap is visible: a genuinely full disk. ENOSPC
# needs a privileged loop or tmpfs mount; the read-only directory above exercises
# the same write-failure path, but that is not the same proof.

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ] || exit 1
