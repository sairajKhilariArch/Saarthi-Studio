# Master roadmap

Source of truth for ordering. Every phase ships only when its acceptance
criteria in `docs/IMPLEMENTATION-SPECIFICATION.md` are demonstrated end to end.
Current status of each area is in `docs/FEATURE-STATUS.md`; the running
checklist is `docs/PROGRESS.md`.

## Dependency order

```
Phase 1  CI/CD + release reliability        (no deps)
Phase 10 Permission engine foundation       (no deps, runs alongside 1)
Phase 17 Observability foundation           (no deps, runs alongside 1)
Phase 16 Test harness growth                (continuous)
Phase 2  Router manager registry            (needs Phase 1 + OS detection helpers)
Phase 3  Model/provider manager             (needs provider abstraction)
Phase 4  Thinking + OpenAI Platform         (needs Phase 3)
Phase 5  Agent execution engine parity      (needs 2 + 3)
Phase 6  Skills management (install/update) (needs 5)
Phase 7  MCP manager                        (needs 10)
Phase 8  Memory + codebase indexing         (needs DB + workspace APIs)
Phase 9  Developer environment              (needs workspace APIs)
Phase 11 Token/context/cost                 (needs 3 + 5)
Phase 12 Live agent monitoring              (needs 5 lifecycle)
Phase 13 Workflow builder                   (needs 5 + 12)
Phase 14 Cross-platform parity              (needs stable APIs)
Phase 15 Extension ecosystem                (needs 10 + 15 manifest)
Phase 18 Installers + updates               (needs verified builds everywhere)
```

## Remaining work at 3.20.0

This is the ordered backlog for the current tree. It supersedes older
"remaining" notes inside individual phase logs; the evidence tables are in
`docs/FEATURE-STATUS.md`, and the defect ledger is `docs/KNOWN-ISSUES.md`.

### Phase 1 — release reliability (blocker)

1. Run the current tree through Desktop Release and get a tagged run where all
   six build jobs pass. No tag has passed with the current 11-asset set; the last
   fully green run was `v1.26.6`, which had nine assets.
2. Confirm the deb job reaches and passes `cargo test --locked
   --no-default-features`. 3.7.0 adds `libdbus-1-dev` and `pkg-config`; the
   command and every assertion are unchanged.
3. Pass the existing deb/AppImage/tarball/zip/macOS install smokes in that run,
   with the VSIX and APK/AAB smokes added in 3.11.0.
4. Resolve macOS runner availability, then decide the historical tags:
   `v1.26.0/1/2` (failed) and `v1.62.0`–`v1.71.0` (published with zero assets) —
   backfill or document them as abandoned.

### Missing product work

| Phase | Remaining |
|---|---|
| 4 — Thinking + OpenAI Platform | Closed in 3.12.0/3.13.0 (structured output, then native tool calling); only a live platform call is unverified. |
| 9 — Developer environment | Editor engine (syntax highlighting, completion, formatting), debugger, watch mode. |
| 14 — Cross-platform | Android router management and local execution; VS Code parity. The local contract shipped in 3.20.0 (20 ids plus limits, the extension's subset and a status read); what remains is making every screen read it instead of gating by hand. |
| 15 — Extensions | Provider adapters and UI extensions. (Dependency cycles since 3.9.0; a dependency's own compatibility and re-judging a folder after an upgrade since 3.14.0.) |
| 18 — Installers | Code signing/notarization, a macOS `.dmg`, and a per-machine MSI choice. |

### Smaller named gaps

- Phase 3: native OpenAI picks carry no price; context is shown only when the
  provider supplies it.
- Phase 6: per-skill permissions remain absent; a skill is prompt text and is not
  executed, so this only matters if that changes.
- Phase 8: semantic search uses the router; there is no local embedding index.
- Phase 12: no workstream remains open; Move up/Move down covers one-step queue
  reordering, but drag-and-drop is not promised.
- Phase 13: add an automated browser test for the canvas layout. Rendering and
  the drag gesture are harness-tested since 3.16.0, and a workflow cancel stops
  the child agent runs it started (3.15.0). Free positioning and cross-process
  schedule claims shipped in 2.47.0/2.48.0.

### Quality and repository hygiene

- Browser-level layout checks for the IDE and workflow canvas, and a privileged
  ENOSPC test.
- Clippy on Windows/macOS as well as Linux (the whole tree is rustfmt-gated since 3.19.0).
- AppStream validation for the renamed Linux package; real-device checks for the
  macOS/Windows/Android icons.
- Reconcile the diverged `website` branch. (The stale workflow names in
  `AGENTS.md` were fixed in 3.10.0.)
- Run restore remains narrower than a workspace snapshot; diagnostics and
  redaction have the limits recorded in `KNOWN-ISSUES.md`.

### Already closed

Router management, the agent engine, MCP, permissions, token/cost, observability,
the workflow engine, and the testing layers are functionally complete. Their
remaining notes are caveats, not missing subsystems.

## Phase 1 — CI/CD and release reliability (in progress)

Delivered in 3.19.0:

- The whole Rust tree is formatted, and both Rust workflows check all of it:
  `desktop-ci.yml` runs `cargo fmt --check` before the build, and
  `rust-host-api-ci.yml` no longer checks only four host modules. This closes
  KNOWN-ISSUES #10; clippy on Windows/macOS (#11) is what remains.

Delivered in 1.26.4:

- `scripts/ci/version.sh` — one version parser for every OS, strips CR/LF/tab/
  space, rejects empty and non-semver values.
- `scripts/ci/verify-artifacts.sh` — refuses artifact basenames containing
  control characters or weird characters, requires at least one artifact, and
  verifies `SHA256SUMS.txt` when present.
- `scripts/ci/test-version.sh` — regression suite for LF/CRLF/empty/malformed
  manifests and bad artifact names; wired into `desktop-ci.yml`.
- All three release jobs call the helper instead of inline `sed`; each job
  validates names before upload; `concurrency` prevents overlapping releases.
- AppStream metadata (`app/packaging/sarthi-studio.appdata.xml`) shipped into
  the AppImage.

Delivered in 3.6.0:

- `scripts/ci/audit-releases.sh` audits every published release at or above
  `MIN_RELEASE_VERSION` (default 3.4.0) and fails if one is missing an expected
  asset. `release-audit.yml` runs it daily and on manual dispatch, so a release
  created without a usable event is still caught. The known `v1.62.0`–`v1.71.0`
  tags remain below the floor; `test-release.sh` covers the audit pass/failure.

Delivered in 3.5.0:

- `release: types: [published]` is now a third trigger, so a release created
  through the API/UI without a tag-push event still builds and attaches its
  artifacts. The event reuses the tag-scoped preflight, build and verification
  path; the workflow's own `GITHUB_TOKEN` release creation does not loop.

Delivered in 3.4.0:

- `workflow_dispatch` accepts an existing `tag`. `preflight` checks out that
  tag, refuses a tag/manifest version mismatch, and gives every job the version,
  tag and release flag. `tag_name` and the concurrency group are scoped to the
  target tag; a dispatch without one only uploads artifacts.
- `scripts/ci/verify-release.sh` requires all 11 promised assets (Linux
  tarball/AppImage/deb, Windows zip/MSI, macOS tarball/app zip, VSIX, APK/AAB,
  `SHA256SUMS.txt`) with non-zero size. The final `verify-release` job fails a
  tag run when one is missing; `scripts/ci/test-release.sh` proves the pass and
  missing-asset failure offline.
- `smoke-package.sh` recognises the installed AppImage/deb UI layout under
  `<exe>/../share/sarthi-studio/static` and the legacy `free-ai-suite` path,
  matching `static_dir_candidates`. This removes a false failure in the
  AppImage smoke.

Remaining for Phase 1 sign-off:

- [ ] A tagged run where all six jobs succeed with the 11-asset set. The last
      fully green run was `v1.26.6`, with nine assets.
- [ ] Confirm the deb job reaches and passes
      `cargo test --locked --no-default-features` after 3.7.0 adds
      `libdbus-1-dev` and `pkg-config`; the command and assertions are unchanged.
- [ ] macOS runner availability resolved (queue starvation/cancellation).
- [ ] The deb/AppImage/tarball/zip/macOS smokes pass in that run; add the missing
      VSIX and APK smokes (added in 3.11.0).
- [ ] Decide the historical tags: `v1.26.0/1/2` (failed) and
      `v1.62.0`–`v1.71.0` (zero assets) — backfill or document as abandoned.

## Phase 2 — Router manager

Delivered through 1.43.0: registry, capability gating, live probes, a real
configuration editor behind a whitelist + `fs_write`, measured host and
package-manager probing, approval-gated install/update/uninstall with pinned
digests and verified downloads, and an auto-start sweep that never adopts an
external router. The panel drives install, update, uninstall, logs, the
configuration editor and the connection tester end to end.

Remote-service routers (3.1.0): `POST /api/routers/:id/remote` stores a
validated base URL per router (https anywhere, http only on loopback; no
credentials, query, fragment or path) behind the `settings` rule, and an empty
string clears it. The list and the connection tester probe that endpoint with
the router's own health path; a remote router is `installed`/`running` only
when it answers, is never owned, and the start/stop/restart/install/update/
uninstall routes refuse with `409` naming the endpoint. The startup sweep never
starts one, and the panel hides the local-only controls while an endpoint is
set. No workstream remains open here; open ends that are not missing features:
the endpoint is a base URL only (no per-router credentials or OAuth), the probe
is an unauthenticated GET, and a real remote router across a network has not
been exercised.

1. Replace the hard-coded FreeLLMAPI/OmniRoute paths in `router.rs` with a
   registry of descriptors (id, name, OS/arch support, install methods, min
   version, config location, health probe, start/stop/update/uninstall).
   **Done (1.26.9)** — `router_registry.rs`; update/uninstall shipped in 1.26.23.
2. Add detection: installed? version? runtime? healthy? owned?
   **Done (1.26.9)** — `/api/routers`, rebuilt from the existing probes;
   `healthy` is "the probe answered".
3. Add an approval-gated install flow with checksum verification. **Mostly done
   (1.26.18–1.26.21)** — plan preview with measured prerequisites, a digest-bound
   approval, recorded installs, registry-digest-pinned images for both gateways
   and verified release downloads (streamed through sha256, unpacked only on a
   match). All three routers have a real path, plus an update check, a gated
   update that pulls before replacing, and an uninstall that keeps data unless
   told to purge it (1.26.23), plus the whitelisted config editor (1.43.0).
4. Add a startup sweep honouring per-router auto-start preferences, and keep
   the ownership rule (never stop what the app did not start). **Done (1.26.17)**
   — preferences in `router-prefs.json`, sweep starts only an enabled, installed,
   not-already-answering router, so nothing is ever adopted.
5. UI: list, status, logs, connection tester, update/uninstall. **Done
   (1.26.13–1.26.16, 1.43.0)** — list, health, start/stop, restart, tester,
   container logs, update/uninstall and the configuration editor.

## Phase 3 — Model and provider manager

1. Capability table per model (context, modalities, tools, reasoning,
   structured output) sourced from provider metadata, never invented.
   **Done (1.28.0)** — `capabilities.rs`, unknown stays `null`.
2. Model selection at global/agent/project/task scope with documented
   precedence. **Done (1.46.0)** — `model_scope::resolve` with a reported
   origin.
3. Health checks, timeouts, retry policy with idempotency guard, circuit
   breaker, explicit fallback chain. **Done (1.50.0)** — `breaker.rs` plus a
   provider health card built from the breaker and 24 h of `request_log`.

Open: native OpenAI Platform picks carry no price (`cost.rs` mirrors no price
table), and a single message cannot persist a scope of its own beyond the
composer's task pick.

## Phase 4 — Thinking + OpenAI Platform

Delivered 1.29.0 (backend) and 1.30.0 (composer and settings):

1. Native OpenAI Platform provider using the Responses API behind the existing
   provider abstraction; keys in the OS credential store, never client-side.
2. Thinking control (`off|low|medium|high|model default`) mapped only to
   parameters the selected model supports; an unsupported model is refused with
   its reason and a rejected parameter is dropped once and recorded.
3. `@Thinking` composer token: activates the control, shows level + model,
   preserves scope, and `\@Thinking` stays literal.
4. Reasoning summaries rendered from the provider's own summary frames; hidden
   chain-of-thought is never claimed or displayed.

Structured output (3.12.0): an agent step on a platform pick now sends
`text.format` with a `json_schema` ("one action object, `action` required, every
field an action may carry described, nothing else required"), so the reply is a
JSON action instead of prose the parser has to repair. A model that refuses the
block answers 400; the request is retried once without it and that model is
remembered for the process, exactly like the reasoning parameter. Router picks
and local mode are untouched — a router has no schema for a step. The earlier
claim that "no agent tool loop uses `openai:` yet" was wrong: agent steps have
routed to the platform since 1.29.0 (`attempt_router_once` handles the prefix),
and this change is what makes them ask for a structured reply.

Native tool calling (3.13.0): the same request also declares the agent's action
vocabulary as functions — `list_files`, `read_file`, `write_file`, `run_command`,
`report_think`, `finish`, `give_up`, plus `mcp_call` and `extension_call` only
when that run's prompt advertised them — with `tool_choice: "auto"`.
`function_call_action` turns a call back into the identical action object the
text protocol uses, so gating, approval, conflict detection, the audit trail and
the step log are unchanged; a call wins over a preamble beside it, and an unknown
function name becomes a `think` step saying nothing ran.

Each step is a fresh request (the app sends its own `input` list, not a previous
response id), so a call is answered through the step history text rather than a
`function_call_output` item: the platform never sees a half-finished tool loop.
Phase 4 is complete in the tree; what it still lacks is a live call against the
real platform, which needs an owner key.

## Phase 5 — Agent execution engine

1. Full task lifecycle (`Created → Queued → Planning → WaitingForApproval →
   Executing → Verifying → Completed/Failed/Cancelled`) with persistence and
   resume, building on `agent_runs`/`agent_steps`.
2. Task queue with priority, dependencies, retry, cancellation, checkpoints.
   *Delivered 1.53.0 for the queue (`queued` state, priority, dependencies and
   in-order admission behind `max_concurrent_runs`) and 1.55.0 for retry as its
   own control alongside resume; checkpoints and cancellation already existed,
   and 1.70.0 added run-level restore across every file a finished run wrote.*
3. Role-specialised agents with explicit tools/permissions/budgets.
4. Parallel execution guarded by worktree isolation or file locks; conflict
   detection before write.
   *Delivered 1.54.0 (conflict detection and path leases), 2.1.0 (a worktree and
   command claim per writing run) and 2.45.0 (the editor asks the same lease).*
5. Verification step before a run is marked complete.
   *Delivered 1.56.0: a run that changed the workspace goes to the verifier and
   is only marked complete on `APPROVE`; ISSUES allows one repair round and then
   fails the run with the findings kept.*

## Phase 6 — Skills management

Install from local dir / git, version tracking, update preserving config,
permission constraints, usage history, automatic selection — on top of the
existing discovery/registry.
*Delivered 1.57.0: a message that names no skill is matched against the enabled
skills' names and descriptions, with a floor, a two-skill cap and a token
budget; the reply reports what was attached. Per-skill permissions remain open,
and only matter if a skill ever executes.*

## Phase 7 — MCP manager

Delivered 1.33.0–1.45.0: registry, stdio + streamable-HTTP + SSE transports,
pooled sessions with an idle supervisor, tool discovery, required-argument and
scalar-type validation, `mcp_tool` permission binding with an audit trail,
health/logs from real handshakes, OAuth sign-in for remote servers, and agent
use of MCP tools behind a per-run flag.

*Delivered 2.7.0: schema validation* — `mcp.rs` checks every call against the
tool's own `inputSchema` beyond presence: type lists, `enum`/`const`, nested
`properties` and `items`, `additionalProperties`, numeric/string/array/object
bounds, `allOf`/`anyOf`/`oneOf`/`not`, boolean schemas and local `$ref`s, with a
depth limit for recursive schemas. Keywords it does not model (`pattern`,
`format`, `propertyNames`, remote refs) accept the value so the server remains
the authority, and an unreadable schema is refused at call time. Phase 7 is
closed; nothing in the specification remains open here.

## Phase 8 — Project knowledge and memory

File discovery, language detection, symbol extraction, incremental index,
search + relevance ranking, exclusions (secrets, build output, deps), and
traceable retrieval. Memory records for decisions/preferences/tasks with a
management UI.
*Delivered 1.58.0: memory records — four kinds, project or global scope, scored
recall capped by count and tokens, usage history, and a sidebar pane to add and
forget them. Delivered 1.59.0: the code index — bounded discovery with
exclusions, language detection, line-based symbol extraction, incremental
refresh by content digest, and ranked search returning a path and a line.
Delivered 1.60.0: retrieval — a chat turn carries the matched declarations with
their surrounding lines, each labelled with path and line. Delivered 1.61.0: agent
runs open with the same background — the goal's matched memory (read without
touching its usage counters) and its matched code, as one labelled system message
ahead of the goal, and 1.62.0 stored that block on the run itself so the card can
say what it was given. Delivered 1.63.0: the semantic layer - indexed symbols are
embedded through the configured router, cached by model and file digest, and
ranked by cosine similarity, with the word search unchanged when no embedding
model is set, and a sidebar Index tab to search both ways (1.64.0).*

## Phase 9 — Developer environment

Editor niceties that do not require a bundled editor engine, real git
operations (status/branch/diff/stage/commit/worktree checkpoints), build/test
runner with a problems panel, and diagnostics tied to real file locations.

Status at 2.1.0: the git and build/test halves are delivered (1.66.0–1.91.0,
including merge, rebase, line-level staging, remotes and worktrees), a run that
may write executes in its own worktree and can hand it back (1.92.0–2.1.0), and a
repository sanity check runs when a folder opens (1.89.0–1.90.0). Open: an
editor engine (syntax highlighting, completion, formatting), a debugger, and
watch mode; only the detected build/test presets can be run.

*Delivered 3.2.0: `git rebase --skip` — the Rebase form can drop the commit the
replay is stuck on and continue with the next one (`POST
/api/workspace/git/rebase/skip`, gated on `git`), returning the dropped subject.
Unmerged markers are allowed here because that is git's own skip case, and the
browser confirms first. In-pane reflog undo arrived in 3.18.0.*

*Delivered 1.66.0: the Git half — `app/src/git.rs` (diff, stage, unstage,
discard, commit, branches, log) behind the `git` permission action with paths
validated before argv, plus the Source Control pane that drives all of it
(inline diff, stage/unstage/discard, commit box, branch create/switch, history,
conflict/staged/changed/untracked groups, ahead/behind). Remaining: editor engine
(syntax highlighting, completion, formatting), debugger and watch mode.
Checkpoints, worktrees, merge/rebase, per-hunk staging and commit
detail all shipped through 3.2.0.*

*Delivered 1.67.0: the build/test loop — detected `cargo build`/`cargo test` and
`package.json` build/test scripts run from the Problems pane, stdout/stderr
captured with a timeout and bounded output, and Rust/TypeScript/generic
diagnostics parsed into clickable `path:line:col` problems. Still open: a
debugger and watch mode. Saved command presets arrived in 3.17.0.*

*Delivered 3.17.0: saved commands. The runner no longer stops at what `detect`
recognises — `Config::command_presets` holds up to 20 `name :: command` entries,
validated on save (name and line non-empty and bounded, no control characters, no
two names that slug to one id), and `runner::all` lists them after the detected
presets with `kind: "custom"` and ids `custom-<slug>`. `/api/workspace/run` still
resolves the posted id against that list, so a command line never comes from the
request, and the same `command` gate is asked with the whole line as its target
before the platform shell (`sh -c` / `cmd /C`) starts it. Settings has the
`name :: command` box; the Problems pane has a picker and Run beside Build/Test,
and one function runs either kind. At 3.17.0 the editor engine, a debugger and
watch mode remained.*

*Delivered 3.18.0: reflog undo. `GET /api/workspace/git/reflog` lists the newest
`HEAD@{n}` entries (selector, hash, action, date) and says whether a merge or
rebase is in progress; `POST /api/workspace/git/undo` resets the branch to an
entry the server itself listed — never a revision expression from the request —
behind the `git` gate, refusing while that history work is unfinished or tracked
files are modified because the reset takes the working tree with it. The reset is
its own reflog entry, so the same list is the way forward again. The Source
Control pane drives it from `Undo…`. Phase 9 is left with the editor engine, a
debugger and watch mode.*

*Delivered 1.68.0: the remote half — fetch with a resolved remote,
fast-forward-only pull that refuses a diverged history with a 409, and push that
never forces and sets the upstream once. Remote commands have a 90 s deadline,
no terminal credential prompt, drained and bounded streams, and the `git`
permission action. Still open: merge/rebase, adding remotes from the UI, and
force push (deliberately).*

*Delivered 1.69.0: per-hunk staging — the diff is split into its real hunks and
one hunk is applied to the index through `git apply --cached` (reversed to
unstage), with the patch built server-side from git's own preamble so the client
never supplies patch text — plus a commit-detail diff from history. Still open:
merge/rebase, staging individual lines, and worktree checkpoints before an agent
writes.*

## Phase 10 — Safety and permissions

Delivered 1.31.0–1.42.0, extended by every route added since: the policy store
(once/session/project/always, longest-target then deny/ask/allow) gates agent
steps, router installs, MCP calls, terminal open, editor saves, workspace
create/rename/delete, settings and key saves, and the Git routes; the OS
credential store holds provider keys and MCP OAuth material; `permission_audit`
records grants, revokes, refusals and approved installs; and the emergency stop
cancels every live run and closes every terminal session.

No workstream remains open here. An application update has no in-app route, so
there is nothing yet to gate.

## Phase 11 — Token/context/cost

Delivered: token-budgeted packing (1 k–32 k clamp), `request_log` with
aggregates/series/by-model, cost from the provider's own price row times its
reported tokens with unpriced rows counted rather than zeroed, a rolling 24 h
budget that refuses before the chain runs and before a stream, structural
compression that reports every condensed and dropped message, and a context
meter with an over-budget state. Native OpenAI Platform picks remain unpriced by
design; nothing is inferred.

## Phase 12 — Live monitoring

Backend-truth dashboard: agents, task, model, router, tokens, duration, tool,
files touched, approvals, errors, with start/pause/resume/cancel/retry/logs.

Delivered: the cross-run board and per-run timing/token facts (1.51.0),
pause/resume of a live run at a step boundary (1.52.0), and the task queue
(1.53.0) — a queued pill, `queue #N` on the card, a Queued count on the board, a
waiting note with Cancel only, and priority / depends-on on the run form — which
completes the control set against real state, queued runs included.

Reordering (3.3.0): `POST /api/runs/:id/queue` moves a queued run one place
(`move: "up"|"down"`), rewriting every queued priority into one explicit
descending sequence inside a transaction, so the card's order is the
dispatcher's order. Only queued runs move, either end refuses with 409, and the
card offers Move up / Move down.

Live step view (2.29.0): `agent_steps` carries a `status`, and the loop writes a
`thinking` placeholder with `status: "running"` before each model call, so the
panel shows the wait during a slow reply instead of a frozen log. The step that
resolves it deletes the placeholder and takes its `seq` (one row per action, no
gap), a run that stops without a step clears it, and the panel renders the row
with a pulsing tag that respects `prefers-reduced-motion`.

No workstream remains open; Move up/Move down covers one-step queue reordering,
but drag-and-drop is not promised.

## Phase 13 — Workflow builder

Started in 2.9.0. `workflows.json` now carries an optional `nodes` array, so a
saved prompt is still valid and a graph is a first-class workflow.

Delivered in 2.50.0: a P0 repair to the IDE shell. The chat view lives in
the AI dock's grid column, which the legacy "right panel closed" default collapsed
to zero width — a fresh install opened threads into a blank 0 px panel with an 8 px
composer. The panel default is now one testable rule (the docked shell opens the
dock), panels are applied again after the dock is built, opening a thread reveals
it, the open thread is remembered across reloads, and the status bar's connection
label follows its dot. `/api/usage`, which every fresh home answered with 500
because its SQL continuations swallowed their spaces, is fixed and tested.

Delivered in 2.49.0: the canvas verified in a browser. A headless build on
a scratch home, driven in Chromium at 1440×900 through the app's own pointer
listeners. The check found a real defect — an unguarded `setPointerCapture` threw
and aborted the drag handler, so a card could not move — now guarded. It confirmed
the drag itself, live edge redraw, clamping to the canvas width with the canvas
growing, and a position that survives save-time validation and a full reload.

Delivered in 2.48.0: cross-process schedule arbitration. A due time is
claimed in SQLite with one conditional upsert that only moves forward, so a desktop
app and the headless server sharing a home start one run per window instead of one
each. A failed start releases its window for the next tick, a stale release is
ignored, deleting a workflow drops its claim, and a process that cannot open the
database schedules nothing.

Delivered in 2.47.0: free positioning on the canvas. A node may carry
`x`/`y`; the layout honours it and falls back to dependency columns for a node that
was never moved. Cards drag with the pointer — edges follow and the canvas grows to
fit — and the position is written back through the editor's normal path, so the
synced graph JSON and the saved workflow carry it. The backend validates it like
every other field: the pair is stored, a half or non-numeric coordinate is absent,
out-of-range values are clamped, and a graph saved before positioning keeps its
canonical form.

Delivered in 2.9.0:

- Node kinds that execute against real state: `agent` (a real agent run, waited
  on, including its approval gate), `approval` (parks the workflow), `condition`
  (asserts dependency outcomes) and `task` (a recorded note).
- Dependency edges as ids or `{id, when: "failed"}`, with unmet branches marked
  `skipped`; a failure counts as handled only when its repair branch succeeded.
- Validation before a run row exists: ids, kinds, profiles, dependencies,
  self references, cycles, per-kind requirements, a 50-node cap — each error
  names the node.
- Persisted `workflow_runs`/`workflow_steps`, one spawned execution task,
  per-node retries, cancel, and a boot sweep that marks live runs `interrupted`.
- Routes for save/list/delete/run, run history, detail, approve and cancel; an
  Automation editor for the node JSON and a live run list.

Scheduling also shipped (2.10.0): `everyMinutes` on the workflow, a 30-second
tick, `lastRunAt`/`nextRunAt` in the list, and a window that advances on start so
a restart cannot fire early or queue missed intervals.

Resume also shipped (2.11.0): an interrupted run keeps its finished steps, reopens
the ones that never ended, and refuses to resume a graph that changed or was
deleted.

A canvas shipped in 2.12.0: cards in dependency columns, SVG edges (dashed for
a `failed` edge), add/rename/delete with dependency rewriting, an inspector for
every node field, and the JSON field kept as a synced advanced view. Layout is
automatic, not free positioning.

Export/import shipped in 2.13.0: a versioned `sarthi.workflow` document, the
same validation on import, a new id and unique name, and a fresh schedule
window.

Remaining: an automated browser test (the canvas was driven manually in 2.49.0),
and cancellation still does not forcibly stop a child agent run already in
flight. Free positioning shipped in 2.47.0 and schedule windows are claimed
across processes in SQLite since 2.48.0.

## Phase 14 — Cross-platform

Capability negotiation driving feature gates; Android remote execution and
router management; VS Code agent parity; consistent API contracts.

Since 3.20.0 the local API answers `GET /api/capabilities` with a versioned
contract, and the VS Code extension's embedded server answers the same shape
with its own subset; its status command reports a match, a mismatch or an older
backend. Android keeps its separate v1 handshake over the Host API. The feature
ids are a promise about this build, not a wish list, and the limits are read
from the constants the routes enforce. The next step is every local screen
reading the contract instead of hiding a control by hand.

## Phase 15 — Extension ecosystem

Started in 2.15.0 with the manifest, kept read-only on purpose: no code is
loaded, so there is no sandbox to claim.

Delivered in 2.15.0:

- `extensions/<id>/manifest.json` under the data directory, discovered by
  `GET /api/extensions` with per-folder validity and the reason for a failure.
- Fields: id (must match the folder), name, version (`1.2.3`), description,
  author, optional `entry` (relative, inside the folder, must exist) and
  permissions from a closed list (`workspace.read`, `workspace.write`,
  `terminal.run`, `network`, `mcp.call`).
- Unknown permissions, a missing manifest or entry file, a path escape and a
  64 KiB size cap all fail validation with a stated reason; one bad folder does
  not hide the valid ones.
- Settings → Extensions lists the directory and every folder's status, with a
  Refresh action (2.16.0).
- Install and uninstall (2.17.0): a folder is copied through staging with the
  manifest validated first, symlinks refused and a 500-file/16 MiB cap; removal
  checks canonical containment and moves the folder to `.removed/`.

Sandbox decision (2.18.0): `docs/adr/0001-extension-execution-sandbox.md` —
WASM-first, capability-brokered, no in-process execution. JS entries stay
installable but not runnable until a sandboxed JS runtime exists.

Runtime gating (2.19.0): `runtime: "wasm"` with a `.wasm` entry, per-extension
`runnable`/`blockedReason`, and `POST /api/extensions/:id/run` refusing before
any code is loaded, naming the unsupported permission or the missing runner.

WASM runner, first slice (2.20.0): a permissionless wasm extension runs in a
wasmi 0.31.2 interpreter with an empty linker and no WASI — `run() -> i32`, no
imports, a 50,000,000 fuel budget, 64 MiB memory, a 100,000-element table limit
and an 8 MiB module cap. Fuel makes cancellation deterministic. The run route
answers `{exitCode, fuelUsed, elapsedMs}` and records the outcome in the oplog.
Because no host function exists yet, any manifest requesting a capability is
refused at run with the permission named.

Workspace host functions (2.21.0): `env.read`/`env.write` plus
`env.last_error`, linked only when the manifest declares `workspace.read` or
`workspace.write`. Paths are resolved with the editor's own containment rules,
writes reuse the atomic `write_text_file`, and each call is checked against the
saved permission rules for the active project. A `deny` refuses the call and a
rule that would `ask` also refuses, because unattended code cannot answer a
prompt — reads default to allow and writes need a saved allow rule. Every call
is audited with the extension id (process log, plus the permission audit table
for refusals), and I/O is capped at 64 calls and 1 MiB per call.

Per-extension data (2.22.0): `env.data_read`/`env.data_write` are rooted at
`<extensions>/<id>/data`, created on the first approved write, contained like
workspace paths and gated by the same declared permissions. The module's code
folder and every other extension stay unreachable.

Run control (2.23.0): one run per extension, `POST /api/extensions/:id/cancel`
stops it at the next host call, a 5-second wall-clock budget is checked at every
host boundary, and uninstall cancels a run in flight before moving the folder.
Settings shows `Running…` with `Cancel` while the request is open.

Run history (2.24.0): `extension_runs` keeps one row per run that reached the
sandbox — including loader failures, traps, fuel stops and cancellations — with
the project, duration, exit code, fuel, host calls and error. `GET
/api/extension-runs` serves the newest 50 and Settings lists them.

Compatibility metadata (2.25.0): `requiresStudio` names the oldest app version
the extension works on, so a manifest written for a newer build stays `valid`
but is `compatible: false` and is neither run nor installed (the reason names
both versions); `apiVersion` names the manifest schema and a number above the
supported 1 is refused at parse rather than half-read; `dependencies` lists
extension ids that must be installed, reported as `missingDependencies` and
blocking a run (not an install, since the dependency can follow). Discovery
reads every manifest before judging any, so a dependency is checked against what
really is installed, and Settings badges an incompatible folder `needs newer
app` with the requirement in its tooltip.

Custom tools (2.26.0): a manifest declares `tools` (name, required description,
optional `.wasm` entry and `inputSchema`; the wasm runtime is required), and the
sandbox gained the matching ABI — `env.arguments` delivers the call's JSON
arguments, `env.result` takes exactly one answer. `POST
/api/extensions/:id/tools/:tool` runs one in the same sandbox as a plain run,
behind a new `extension_tool` permission action whose `ask` default refuses
because the route has no prompt, and records the call in `extension_runs`.

Remaining: no provider adapters and no UI extensions. Installing grants no
capability by itself; the reachable surface is the two workspace pairs, the
extension's own data folder, the tool ABI and `last_error`. A dependency is
checked for presence, not its own compatibility, and no dependency cycle is
detected; an app upgrade does not re-check installed folders.

Custom tools in the agent loop (2.27.0): building a run's prompt reads every
valid, runnable folder's declared tools from disk and pushes them as a labelled
`system` catalogue (nothing is advertised when none is installed), so the model
can emit `{"action":"extension","extension":...,"tool":...,"arguments":...}`.
The step is gated by `extension_tool` on the same `extension/tool` target the HTTP
route uses — approval unless the run auto-approves, a saved `deny` refuses — and
runs through the same sandbox with the same budgets, grants, one-run rule, cancel
boundary, `extension_runs` row and host-call audit; `env.result` becomes the step
output.

Tool arguments (2.28.0): a call is checked against the tool's declared
`inputSchema` before the module is loaded — the same bounded JSON Schema subset
MCP calls use (`type`, `properties`, `required`, `enum`/`const`, bounds, array
items, nested objects, draft-4 tuples) — with `true`/`false` schemas honoured, an
unreadable schema refused rather than ignored, and the refusal naming the tool and
the failing field. The prompt lists each tool's required argument names, and the
route checks the saved permission rule before the arguments, so a denied call
stays denied.

What remains in this phase: there are no provider adapters and no UI extensions,
and a dependency cycle or an update path that re-checks a folder after the app is
upgraded is still missing.

## Phase 16 — Testing

Frontend unit tests, end-to-end runs (launch → chat → agent → approve →
restart), failure injection matrix, platform matrices, and release-artifact
install smokes in CI.

Delivered in 2.5.0: the frontend unit layer — `app/static/tests/` loads the real
`app.js` in a browser-free `node:vm` harness, 12 cases over pure helpers, run by
`scripts/ci/test-frontend.sh` and gated in `desktop-ci.yml`.

Delivered in 2.6.0: the end-to-end flow up to the agent step —
`scripts/ci/test-e2e.sh` launches the real binary against a scratch home and the
mock provider, runs a chat turn, reads the stored conversation, then restarts the
process and confirms the state came from disk (11 checks, ~3 s, gated in CI).

Delivered in 2.8.0: the same flow through a real agent run — a workspace folder
opens, the run parks at its write approval gate, the approved write lands on
disk, the step log is `write, verify, done`, and the run with its steps is
re-read after the restart (19 checks, gated in CI). The first attempt failed on
a Git Bash `/tmp` path the native binary cannot resolve; the workspace call now
passes a `cygpath -m` path.

Delivered in 2.30.0: the first failure-injection cases, both process-level and
both asserted through the public routes. The mock provider is killed and a run
started anyway must reach `failed` with a recorded error while `/api/ready` keeps
answering; the mock answers slowly (`--chat-delay`) so the harness can wait for a
recorded step and the next model call in flight, kill the process, and require
the restarted app to report that run `interrupted` with a readable step log. The
harness is 28 checks, up from 19.

Delivered in 2.31.0: install smokes. `scripts/ci/smoke-package.sh` unpacks a
release artifact the way a user would (tarball, zip, `.deb`, AppImage, or a bare
binary), boots the packaged binary headless against a scratch home, waits for
`/api/ready`, serves `/`, and compares the version the binary reports with
`app/Cargo.toml`. The release workflow runs it on the `.deb`, the Linux tarball
and AppImage, the Windows zip and the macOS tarball.

Delivered in 2.32.0: Windows in CI. `desktop-ci.yml` builds the default `gui`
feature and runs the full Rust suite on `windows-latest`, so a Windows-only
source break fails on the push instead of on a tag; the release pipeline keeps
building the shipped Windows zip. Linux keeps clippy and the end-to-end smoke.

Delivered in 2.33.0: the render smoke. The harness now returns stable element
stubs and records creations, so `renderWorkflowCanvas` can be executed and its
cards, dependency edges and empty-graph note inspected, and `ide.js` is evaluated
on top of `app.js` to prove the shell builds its chrome and wraps the app rather
than replacing it. The frontend suite is 21 cases. This is still not a layout or
pixel check — that needs a browser.

Delivered in 2.34.0: two more injections. A corrupt database must stop the
process with the failure named in its log and nothing on the port, and restoring
a saved copy must bring it back ready; a read-only data directory must make the
`data directory` readiness check go false and recover (POSIX only, skipped where
`chmod` is not enforced).

Remaining: a privileged ENOSPC case (a real full disk needs a loop/tmpfs mount)
and browser-level layout checks for the canvas and IDE. The VSIX and the
APK/AAB got their install smokes in 3.11.0 (`smoke-vsix.sh`, `smoke-apk.sh`),
beside the desktop archives, deb and AppImage that were already wired.

## Phase 17 — Observability

Delivered in 1.72.0: one read-only timeline over `request_log`,
`permission_audit`, `agent_runs` and failing `agent_steps` with a level and a
correlation key per row (`GET /api/diagnostics`, area and limit filtered), counts
from the same rows, and `GET /api/diagnostics/bundle` — version, platform,
non-secret settings, credential booleans and the timeline, with run goals omitted
and every secret-shaped value redacted. Settings → Diagnostics shows it and saves
the bundle.

Delivered in 2.2.0: a persistent structured log file and console ring buffer
(`oplog.rs` → `studio.log`, rotated at 1 MiB, 200-entry ring behind
`GET /api/diagnostics/log`, shown in Settings → Diagnostics), written at boot,
on router lifecycle changes, permission ask/deny, provider failures and every run
ending.

Delivered in 2.3.0: router installs and answering MCP servers are timeline rows
with their own timestamps and `router`/`mcp` area filters.

Delivered in 2.4.0: a request id per local API call (`trace.rs`, echoed as
`x-request-id`, stored on `permission_audit` by in-place migration, shown on
timeline and log rows) and `GET /api/ready` — cheap local checks with only the
database and the data directory marked required, and a Readiness block above the
timeline.

**Phase 17 is complete.** What remains in the roadmap is 13 (workflow graph), 15
(extension manifest) and the 16/18 quality and packaging work.

## Phase 18 — Installers and updates

Delivered: Linux `.deb`, AppImage and tarball, a Windows portable zip, an
unsigned macOS universal tarball, an Android APK/AAB and a VS Code VSIX.

Delivered in 2.35.0: the data half of an update. `app/src/schema.rs` records the
schema version in `PRAGMA user_version`; an existing database is copied with
`VACUUM INTO` and the copy is opened, `quick_check`ed and version-checked before
a migration runs; a migration that fails restores that copy and leaves the
version unadvanced so the next boot retries; and a database written by a newer
build is refused without being touched. Boot logs the outcome to `studio.log`
and `/api/ready` reports the schema version as a required check.

Delivered in 2.36.0: the update check. `GET /api/update/check` reads the newest
published release, compares versions numerically, and names the asset this
platform would download (or says there is none for this target). It writes and
installs nothing, so it is not permission-gated; `/api/health` now reports the
running version and Settings → Privacy & about shows it with a Check button.

Delivered in 2.37.0: the verified download. `POST /api/update/stage` resolves the
published sha256 from the release's asset digest or `SHA256SUMS.txt` (refusing a
release that publishes neither), downloads through `installer::download_verified`
so only a matching file is ever renamed into place, records it in
`updates/staged.json`, and is gated on `update` with an explicit approval.
`GET /api/update/staged` and `POST /api/update/staged/discard` manage what is
waiting, and the About card can download and discard it.

Delivered in 2.38.0: the prepare step, and gzip for the extractor. `POST
/api/update/prepare` re-checks the staged sha256 (the staged view compares sizes
only), unpacks `.zip`/`.tar.gz`/`.tar.zst` through escape-proof extraction into
`updates/prepared/<version>/`, refuses a tree with no runnable binary, and records
what it unpacked. `GET /api/update/prepared` and its discard route manage the
result; both the download and the prepare are gated on `update` through one
helper.

Delivered in 2.39.0: the swap. `POST /api/update/swap` copies the prepared binary
beside the target, hashes the copy, renames the installed binary to
`<name>.old-<version>` and restores it if the new one cannot take its place, so a
copy that lands wrong never reaches the rename. Renaming is what makes it possible
while the app runs; the reply and the panel say a restart is required rather than
claiming the upgrade is already live.

Delivered in 2.40.0: the cleanup after a swap. At startup the app removes
`<name>.old-<version>` files beside its own binary when their version is older than
the running build, never touching the file being executed or a newer backup, and
logs the outcome instead of failing to start.

Delivered in 2.41.0: the way back. `POST /api/update/rollback` copies the backup
`applied.json` records back over the installed binary through the same hashed copy
path the swap uses, and keeps the build it displaces at `<name>.old-<version>` so a
rollback can be rolled forward by hand. It refuses with a reason and no change when
nothing is recorded, the backup is gone, the record names another binary, or the
backup name is not one this app wrote; the panel says which build is back and where
the newer one went.

Delivered in 2.42.0: the macOS application bundle. `scripts/ci/bundle-macos-app.sh`
builds `Sarthi Studio.app` from the staged universal tree — binary in
`Contents/MacOS`, UI in `Contents/Resources/static`, an `Info.plist` with the
version, identifier and executable — and `static_dir_candidates` resolves that
layout, so the bundle runs as an app rather than a binary with no UI. The release
job smokes the `.app.zip` the same way as a tarball and attaches both.

Delivered in 2.43.0: the Windows installer. `app/packaging/windows/installer.wxs`
is a per-user WiX package that harvests the staged tree, adds a Start Menu
shortcut and registers a major upgrade; `scripts/ci/build-windows-msi.sh` builds it
and proves the payload with an administrative install, `smoke-package.sh` boots the
installed binary, and the release job attaches the MSI beside the portable zip.
The toolchain is WiX 5 (MIT) because WiX 7 gates builds behind a paid fee.

Delivered in 2.46.0: worktree paths compared the way the filesystem does. macOS
reaches its temporary directory as `/var/folders/...` while git prints
`/private/var/folders/...`, so a worktree of the repository was refused as
another's and macOS CI failed. Both sides are resolved with `fs::canonicalize`
(falling back to the normalised spelling for a path that does not exist), and git
is handed its own spelling of the validated match.

Delivered in 2.45.0: the editor saves through the run lease. A save, create,
rename or delete asks `filelock` which live run holds the path — or the folder,
while that run is running a command there — and answers 409 naming the run rather
than racing it. The agent's write already refused the other direction, so the
conflict is now loud on both sides instead of silent on one.

Delivered in 2.44.0: the way off a computer for a portable install. Settings →
About names the program folder, the settings folder and the data folder with
their counts, offers to keep chats/projects and settings, and removes the rest
only after `approve: true` and a confirmation that is the data folder's path,
under its own `uninstall` permission action. A source checkout is refused, the
program folder is removed first so a refusal cannot orphan a working install, and
a Windows binary is renamed aside with the folder reported as pending, because a
running program cannot be deleted.

Open: code signing/notarization, a macOS `.dmg`, and a per-machine MSI choice.
The portable uninstall shipped in 2.44.0.

## Identity (2.51.0)

The product name is **Sarthi Studio** across the UI, documentation, installers and artifacts. The repository-wide audit, the full old→new map, and every identifier that was deliberately kept (with its migration path) are in `docs/BRANDING-AUDIT.md`. The GitHub repository keeps its own name; the two are not the same thing.

## Mascot (2.52.0)

The product mark is a tardigrade face — orange, black and white, no gradients — generated from one geometry and shipped as the window icon, installer icon, favicon, Linux theme icon, macOS bundle icon and Android adaptive launcher. `docs/BRAND-ASSETS.md` is the inventory.

## 3.8.0 phase matrix

Measured against the implementation, not the plan. Detail and evidence per row:
`docs/FEATURE-STATUS.md`; defects: `docs/KNOWN-ISSUES.md`.

| Phase | State | What is still open |
|---|---|---|
| 1 CI/CD and release | Partial | no fresh green tagged run with the 11-asset set; 3.7.0 deb dependency awaits CI |
| 2 Router manager | Working | — (remote routers are unauthenticated base URLs; no live network test) |
| 3 Model and provider | Working | native OpenAI picks carry no price; context only when the provider supplies it |
| 4 Thinking / OpenAI | Working | no live platform call has been made (the request shapes are proven against a mock) |
| 5 Agent execution | Working | — |
| 6 Skills | Working | per-skill permissions only matter if skills execute |
| 7 MCP | Working | — |
| 8 Memory and RAG | Working | semantic search is a router proxy, not a local embedding index |
| 9 Developer environment | Partial | no editor engine, debugger or watch mode |
| 10 Safety and permissions | Working | — |
| 11 Token, context, cost | Working | cost only where the provider reports usage |
| 12 Agent monitoring | Working | — |
| 13 Workflows | Working | automated browser coverage; child agent cancellation |
| 14 Cross-platform | Partial | Android/VS Code subsets; local contract in 3.20.0, screens still gate by hand |
| 15 Extensions | Partial | provider adapters and UI extensions |
| 16 Testing and quality | Working | browser layout, privileged ENOSPC |
| 17 Observability | Working | — |
| 18 Install and update | Partial | signing/notarization, macOS `.dmg`, per-machine MSI |
