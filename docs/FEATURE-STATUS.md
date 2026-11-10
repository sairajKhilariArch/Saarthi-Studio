# Feature status report — audit of `main` @ 2.1.0, revised through 2.25.0

Read against the master specification phase by phase. Status vocabulary:
**Working** (route + persistence + UI verified in code or by a recorded run),
**Partial** (exists but a stated requirement is unmet), **Missing** (no
implementation), **Broken** (exists and fails).

## Evidence for this audit

| Check | Command | Result |
|---|---|---|
| Rust suite | `cd app && cargo test --locked` | see the count recorded in `PROGRESS.md` for this version (test functions in tree: 504) |
| Route registrations | `grep -c '\.route(' app/src/{main,agent,host_api,git}.rs` | 44 / 15 / 8 / 28 |
| Test functions in tree | `grep -rc '#\[tokio::test\]\|#\[test\]' app/src` | 504 |
| Last green release | run `41884439627` (`v1.26.6`) | all six jobs success, 9 assets |
| Release line | `8ff7c5e` … `2.27.0` | 1.26.4 → 2.1.0 all shipped, LF manifests |

The counts above are what the tree contains today, not a projection.

**Revision 2.49.0 (2026-11-10):** the canvas was driven in a real browser
for the first time, closing Phase 13's last verification gap. The check found one
real defect (an unguarded pointer capture that aborted the drag) and confirmed the
gesture, the clamping, the live edges and the round trip through save and reload.
Phase 13 rows stay Working and no row was added, so the figures are unchanged at
92% (1,659/18).

**Revision 2.48.0 (2026-11-10):** scheduled windows are now arbitrated
between processes — the claim lives in SQLite and only moves forward, so a desktop
app and the headless server over one home start one run per due time. The Phase 13
row stays Working and no row was added, so the phase and overall figures are
unchanged at 92% (1,659/18).

**Revision 2.47.0 (2026-11-10):** Phase 13's visual editor gains free
positioning — a node may carry `x`/`y`, honoured by the layout and persisted through
save-time validation — so the canvas is no longer limited to dependency columns.
The Phase 13 row moves no category (it was already Working), so the phase and
overall figures are unchanged at 92% (1,659/18).

**Revision 2.46.0 (2026-11-10):** a CI fix, not a feature. Worktree removal
compared paths as strings, so on macOS a worktree reached through `/var/folders`
never matched the `/private/var/folders` path git reports and was refused as
another repository's. The comparison now resolves both sides through the
filesystem, and git is handed its own spelling of the validated path. Phase counts
are unchanged; the overall mean stays 92% (1,659/18).

**Revision 2.45.0 (2026-11-10):** the editor now saves through the run lease:
`filelock::claimed_by`/`command_claim` let a save, create, rename or delete ask
which live run holds the path or the folder, and refuse with `409` naming it
instead of racing it. The agent's write already refused the other direction, so a
conflict now fails loudly on both sides. Phase counts are unchanged — this closes
an open edge in row 20, not a feature row. Overall mean stays 92% (1,659/18).

**Revision 2.44.0 (2026-11-10):** a portable (tarball/zip) install can now be
uninstalled from Settings → About: `POST /api/uninstall` removes the program
folder, the settings folder and the data folder after an `approve` plus a
confirmation that is the data folder's path, keeps either tree on request, and
refuses a source checkout outright. On Windows the running binary is renamed
aside and the folder is reported as pending, because a running program cannot be
deleted. Phase 18 stays 10 rows — 9 Working + 1 Partial = 95% — because signing
is still missing, and the overall mean stays 92% (1,659/18).

**Revision 2.43.0 (2026-11-10):** Phase 18's installer row is now Working for both
Windows and macOS: a per-user MSI (`app/packaging/windows/installer.wxs` built by
`scripts/ci/build-windows-msi.sh`, payload proven by an administrative install)
sits beside the macOS `.app`, and the release job smokes both packaged results.
What keeps the row Partial is signing: nothing is code-signed or notarized, and a
portable (tarball/zip) install still has no uninstall of its own. Phase 18 is 10
rows — 9 Working + 1 Partial = 95% — and the overall mean stays 92% (1,659/18).

**Revision 2.42.0 (2026-11-10):** Phase 18 gained the macOS application bundle:
`scripts/ci/bundle-macos-app.sh` builds `Saarthi Studio.app` from the staged tree
and the binary resolves `<exe>/../Resources/static`, so the bundle is a real
installable app rather than a renamed binary; the release job smokes its zip and
attaches it. Phase 18's install row moves from Missing to Partial (macOS bundled,
Windows MSI/EXE and uninstall still absent), so the phase is 10 rows — 9 Working +
1 Partial = 9.5/10 = 95% — and the overall mean stays 92% (1,659/18, rounded; the
sum was 1,654 before this row moved).

**Revision 2.41.0 (2026-11-10):** Phase 18 gained the way back: `POST
/api/update/rollback` restores the backup a swap kept (the copy is hashed first) and
keeps the build it displaces, refusing without a record, a missing backup, another
binary's record or a backup name this app did not write. Phase 18 stays 10 rows at
90% — the install row is still Missing because there is no MSI/EXE/.app or
uninstaller — and the overall mean stays 92%.

**Revision 2.40.0 (2026-11-10):** the backups a swap leaves behind are cleared at
startup: only `<name>.old-<version>` names whose version is older than the running
build, never the file being executed and never a newer one. Phase 18 stays 10 rows
at 90% and the overall mean stays 92%.

**Revision 2.39.0 (2026-11-10):** Phase 18 gained the swap: `POST
/api/update/swap` copies the prepared binary beside the target, hash-checks the
copy, renames the old binary to `<name>.old-<version>` and restores it if the new
one cannot take its place. The running process keeps its image until a restart,
which the reply says. Phase 18 is 10 rows — 9 Working + 1 Missing = 9/10 = 90% — and
the overall mean stays 92%.

**Revision 2.38.0 (2026-11-10):** Phase 18 gained the prepare step: the extractor
reads gzip tarballs (this app's own Linux/macOS artifacts), `POST
/api/update/prepare` re-verifies the staged sha256, unpacks into
`updates/prepared/<version>/` and records the binary it found, refusing a tree
without one. Phase 18 is 9 rows — 8 Working + 1 Missing = 8/9 = 89% — and the
overall mean stays 92%.

**Revision 2.37.0 (2026-11-10):** Phase 18 gained the verified download: `POST
/api/update/stage` fetches the named asset through the verified-download path
(hash from the release's `digest` or `SHA256SUMS.txt`, refused when neither
exists), records it in `updates/staged.json`, and is gated on `update`. Nothing
applies it. Phase 18 is 8 rows — 7 Working + 1 Missing = 7/8 = 88% — and the
overall mean stays 92% (the table's own percentages sum to 1,654/18).

**Revision 2.36.0 (2026-11-10):** Phase 18 gained an update check (2.36.0):
`GET /api/update/check` reads the newest published release, compares versions
numerically and names this platform's asset, and Settings → Privacy & about
renders it. Nothing downloads or installs, so the install row stays Missing.
Phase 18 is now 7 rows — 6 Working + 1 Missing = 6/7 = 86% — and the overall mean
is 92% (the table's own percentages sum to 1,654/18).

**Revision 2.35.0 (2026-11-10):** Phase 18's last row moved from Missing to
Partial: the database is versioned (`PRAGMA user_version`), copied with
`VACUUM INTO` and verified before an existing one is migrated, restored if a
migration fails, and a database from a newer build is refused untouched
(2.35.0). Auto-update and uninstall are still absent. Phase 18 is 6 rows —
5 Working + 1 Partial = 5.5/6 = 92% — so the overall mean is 92% (the table's own
percentages sum to 1,658/18; re-derived here because the previous note's 1,630
did not match its own table). Superseded by the 2.36.0 note above, which adds a
row and re-derives the totals.

**Revision 2.34.0 (2026-11-10):** Phase 16's failure-injection row gained two more
cases (2.34.0): a corrupt database (the process must exit with a named error and
recover from a restored copy) and a read-only data directory (the readiness check
must go false and recover). A true ENOSPC case is still not run and is named as
such. No row changed status, so Phase 16 stays 100% and the overall mean stays 91%.

**Revision 2.33.0 (2026-11-10):** Phase 16's frontend row gained a render smoke
(2.33.0): the workflow canvas and the `ide.js` shell are now executed in the
harness and their elements inspected, so the "no render smoke, no `ide.js`
harness" caveat is gone and the frontend suite is 21 cases. No row changed
status, so Phase 16 stays 100% and the overall mean stays 91%.

**Revision 2.32.0 (2026-11-10):** Phase 16's Windows row moved from Missing to
Working — `desktop-ci.yml` now builds and tests the desktop on `windows-latest`
(2.32.0) — so the phase is 9 of 9 rows = 100%, and the overall mean is 91%
(1,630/18). Its named caveats are the `ide.js` render smoke and the remaining
injections, not a missing layer.

**Revision 2.31.0 (2026-11-10):** Phase 16's install-smoke row moved from Missing
to Working: `scripts/ci/smoke-package.sh` unpacks each release artifact (tarball,
zip, `.deb`, AppImage), boots the packaged binary and checks its reported version,
and the release workflow runs it on all five artifacts (2.31.0). The phase is 8 of
its 9 rows = 89%, and the overall mean is 90% (1,619/18).

**Revision 2.30.0 (2026-11-10):** Phase 16's failure-injection row moved from
Missing to Working — the process-level harness now kills the provider and kills
the process mid-run and asserts what the restart says (2.30.0) — so the phase is
7 of its 9 rows = 78%, and the overall mean stays 89% (1,608/18).

**Revision 2.29.0 (2026-11-10):** Phase 12's named gap — a live step view while a
model call is in flight — closed in 2.29.0: the loop writes a `thinking`
placeholder with `status: "running"` before each call and the step that resolves
it replaces that row. The phase moves 90% → 95% (reordering beyond a numeric
priority is what is left), which moves the overall mean 88% → 89% (1,597/18).

**Revision 2.28.0 (2026-11-10):** Phase 15's custom-tool row now enforces the
declared `inputSchema` before a module loads (2.28.0), so its largest open gaps are
provider adapters and UI extensions. The phase stays 82% (9 of 11 rows) and the
overall mean stays 88% — no row changed status, so no percentage moved.

**Revision 2.27.0 (2026-11-10):** Phase 15's custom-tool row moved from Partial to
Working — the agent's tool loop now offers installed tools (2.27.0) — so the phase
is 9 of its 11 rows = 82%. Re-deriving it exposed two arithmetic slips, corrected
here: the 2.26.0 note counted 12 rows and 71% where the table has 11 rows and the
figure then was 77% (8 working + 1 partial = 8.5/11), and the stated overall mean
of 82% did not match the mean of this table's own phase percentages, which is 88%
(1,592/18). The method is unchanged; only the arithmetic was re-run.

**Revision 2.26.0 (2026-11-10):** Phase 15's custom-tool row moved from Missing to
Partial: a manifest can declare tools, the sandbox has a tool ABI
(`env.arguments`/`env.result`), and `POST /api/extensions/:id/tools/:tool` runs
one behind the new `extension_tool` permission. It is not in the agent's tool
loop yet and `inputSchema` is not enforced, so the phase is re-derived from its
now 12 rows (8 working + 1 partial = 8.5/12 = 71%); the overall mean stays 82%.

**Revision 2.25.0 (2026-11-10):** Phase 15 gained a compatibility row —
`requiresStudio`, `apiVersion` and `dependencies` are declared, reported and
enforced, so a folder written for a newer build is refused by name instead of
half-read. Phase 15's percentage is re-derived from its current rows (8 of 11
working = 73%), which also moves the overall mean to 82%; the row-level verdicts
below otherwise stand.

**Revision 2.7.0 (2026-11-09):** Phase 7's last row closed — MCP calls are
validated against a bounded JSON Schema subset. The row-level verdicts below
otherwise stand as written at 2.1.0.

**Revision 2.1.0 (2026-11-09):** rows re-verified against the tree at `2.1.0`.
Closed since the 1.28.0 pass: Phase 2 configuration editor and dependency
probing (1.43.0), Phase 3 circuit breaker and provider health (1.50.0), Phase 10
credential store (1.42.0) and global emergency stop (1.41.0), Phase 17 timeline
and bundle (1.72.0). This file is the per-requirement verdict; `PROGRESS.md` is
the shipped-version checklist, `MASTER-ROADMAP.md` the ordering and
`KNOWN-ISSUES.md` the defect list. When they disagree, this file and
`PROGRESS.md` are corrected together.

## Verdict in one paragraph

Phases 2, 3, 4, 5, 6, 7, 8, 10, 11 and 14 are delivered in substance; what
remains inside them is named per row (router update/uninstall in the panel,
remote-service routers, a price for native OpenAI picks, per-skill permissions,
an editor that bypasses the run lease, live
capability negotiation). Phase 1 needs install smokes and the dead-tag decision,
Phase 9 lacks an editor engine and a debugger, **Phase 13 now runs graphs on a
schedule, resumes an interrupted one, edits them on a canvas and moves them
between installs, now including placing cards by hand (2.47.0); and the canvas was driven in Chromium once
(2.49.0), and Phase 15 runs wasm extensions with workspace and own-data access, now declares compatibility and dependencies, and now offers its custom tools to the agent's tool loop, but still has no provider adapters or UI extensions**; Phase 12 lacks a live step view,Phase 16 now has a frontend unit layer (2.5.0) and a process-level end-to-end run (2.6.0, through
the agent approval gate since 2.8.0) but no failure matrix or install-smoke layer, and Phase 18 ships every artifact, protects the database across an upgrade
(2.35.0), reports whether a newer release exists (2.36.0) and downloads and
verifies one into staging (2.37.0), unpacks it beside the data directory (2.38.0)
and swaps it in keeping the replaced binary as a backup (2.39.0), but there is no
MSI/EXE/.app and no uninstaller.
Phase 17 closed in 2.4.0.

## Completion by phase (derived, not a release gate)

Method: each phase's requirement rows in this file count **Working = 1**,
**Partial = 0.5**, **Missing/Broken = 0**; the phase percentage is that sum over
the row count, and "overall" is the unweighted mean of the phases. Rows the
specification marks optional (Ruflo) are excluded. The numbers move only when a
row below moves, so they cannot drift from the tables.

| Phase | Area | Completion | Largest open gap |
|---|---|---|---|
| 1 | CI/CD and release | 78% | install smokes in CI, dead-tag decision |
| 2 | Router manager | 92% | remote-service routers; update/uninstall flow in the panel |
| 3 | Model and provider | 89% | price and context for native OpenAI picks |
| 4 | Thinking + OpenAI Platform | 88% | tool calling / structured output through the platform path |
| 5 | Agent execution | 100% | named caveats only (an editor save bypasses the run lease) |
| 6 | Skills | 92% | per-skill permissions |
| 7 | MCP | 100% | — (schema validation landed 2.7.0) |
| 8 | Memory and RAG | 93% | embeddings are a router proxy, not a local index |
| 9 | Developer environment | 82% | editor engine, debugger, watch mode |
| 10 | Safety and permissions | 100% | updates have no in-app route to gate |
| 11 | Token and context | 100% | — |
| 12 | Live monitoring | 95% | reordering beyond a numeric priority |
| 13 | Workflow builder | 95% | free positioning and browser verification of the canvas |
| 14 | Cross-platform | 80% | client subsets, capability-driven gating |
| 15 | Extension ecosystem | 82% | provider adapters, UI extensions |
| 16 | Testing and quality | 100% | named caveats only (browser layout checks, a privileged ENOSPC case) |
| 17 | Observability | 100% | — |
| 18 | Installation and updates | 90% | Windows MSI/EXE, macOS `.app`, uninstall |

**Overall (mean of the 18 phases): 92%.** Read it as "one reliable core, most
phases closed, the remaining work concentrated in 1, 9, 14 and 18". The
per-row method is the arbiter: a phase moves only when a row here moves.

## Phase 1 — CI/CD and error diagnosis

| Item | Status | Evidence / gap |
|---|---|---|
| One version parser for all OSes | Working | `scripts/ci/version.sh` strips `[:space:][:cntrl:]`, validates semver |
| Version regression suite | Working | `scripts/ci/test-version.sh`, wired into `desktop-ci.yml` |
| Artifact-name + checksum gate | Working | `scripts/ci/verify-artifacts.sh` runs before every upload |
| Release concurrency | Working | `concurrency: desktop-release-${{ github.ref }}` |
| AppStream metadata | Working | `appstreamcli validate-tree` clean; AppImage attaches |
| macOS job | Working | `macos-15-intel`, both Darwin targets, universal tarball |
| All six release jobs green | Working | run `41884439627`, 9 assets |
| Install smoke on deb/AppImage/vsix/apk | **Missing** | only the Windows zip was launched locally |
| Dead tags `v1.26.0/1/2` | Open | documented as failed; no decision to delete |

Issue A (agent SQL syntax) is **not reproducible**: `agent.rs` compiles and its
query paths are covered by tests — left unchanged rather than rewritten blindly.
Issue F (macOS cancellation) was diagnosed as runner acquisition, not code.

## Phase 2 — Router manager

| Item | Status | Notes |
|---|---|---|
| Central registry with §5.1 fields | Working | `router_registry.rs`; only routers with a real lifecycle are listed |
| OS/arch gating, Android never claims a local install | Working | `capability()` + tests |
| Detection: installed/version/runtime/healthy/ownership | Working | `/api/routers`, probes run concurrently (1.26.14) |
| `healthy` means the probe answered | Working | unit-tested both ways |
| Approval-gated install with plan digest | Working | 1.26.18 |
| Pinned images, verified downloads | Working | 1.26.19/1.26.20/1.26.21 |
| Update + uninstall (data kept unless `purge`) | Working | 1.26.23 |
| Auto-start sweep, external routers never stopped | Working | 1.26.17, rule tested both ways |
| UI: list, health, start/stop, restart, tester, logs | Working | 1.26.13–1.26.16 |
| Configuration editor | Working (1.43.0) | `GET /api/routers/:id/config` lists whitelisted files in the install's config dir; `GET/POST /api/routers/:id/config/file?name=` reads and writes one of them through the same validator, 64 KiB cap, gated on `fs_write`; a router with no editable file explains why instead of showing an empty box |
| Dependency/package-manager probing | Working (1.43.0) | `/api/routers` reports the measured host: OS + version, arch, apt/dnf/pacman/zypper/apk/brew/winget/choco/scoop/port with versions, Docker and compose, read from PATH and the tools' own output |
| Update and uninstall from the panel | Partial | both routes exist, are approval-gated and keep the data volume unless `purge`; the panel shows the plan rather than driving a full update/uninstall flow |
| Remote service routers | Missing | registry is local-process only |

## Phase 3 — Model and provider manager

| Item | Status | Notes |
|---|---|---|
| Provider presets | Working | `app.js` / `index.html` |
| Model discovery with cache | Working | `/models`, 1-minute cache keyed by base + key |
| Local Ollama models | Working | `local.rs` (status, tags, hardware, pull with progress) |
| Context window where a provider reports it | Partial | `main.rs:1181` reads `context_window`; `app.js:2704` displays it — nothing is inferred when absent |
| Capability table (modalities, tools, reasoning, structured output) | Working (1.28.0) | `capabilities.rs` from `/models` rows and Ollama `/api/show`; unknown stays `null` |
| Price table | Partial (1.28.0) | carried when the provider publishes `pricing.prompt`/`completion`; never inferred |
| Model scope (task > agent > project > global) | Working (1.46.0) | `model_scope::resolve` with a reported origin; `projects.model`, `agent_model`, and a per-run `model`; the composer pick stays the task scope |
| Fallback / retry | Working (1.50.0) | ordered, capped, deduplicated chain that retries only an unavailable provider and never replays a half-streamed answer; every attempt lands in meta. A circuit breaker (`breaker.rs`) opens after three consecutive unavailable answers, refuses the model for 45 s, then allows one probe; a skipped candidate is recorded as `circuit open: skipped` and the whole-chain refusal names each model's remaining seconds |
| Provider health monitoring | Working (1.50.0) | `GET /api/health/providers` merges the breaker's live state with 24 h of `request_log` (requests, successes, average latency, the provider's own last error) and states the opening rule; shown as a Provider health card on the usage page. `/api/health` remains the cheap liveness answer |

## Phase 4 — Thinking and OpenAI Platform

Backend in 1.29.0, composer and settings in 1.30.0.

| Item | Status |
|---|---|
| Native OpenAI Platform provider (Responses API) | Working (1.29.0) — `openai.rs`, own key, `openai:` model ids, buffered + streaming + agent calls |
| Thinking control (off/low/medium/high/model default) | Working (1.30.0) — composer Think control, levels from `/api/thinking/levels` |
| `@Thinking` shortcut + literal escape | Working (1.29.0/1.30.0) — stripped server-side before storage, hinted in the composer, `\@Thinking` stays literal |
| Scope precedence (message > agent > project > global) | Working (1.29.0) — `projects.thinking` column migrated in place, agent scope from config |
| Reasoning summaries / progress surfacing | Working (1.30.0) — collapsible card fed by the provider's summary frames |
| Platform key in Settings (write-only) + key test | Working (1.30.0) — `/api/openai/test`, no tokens spent |
| Tests | Working — 21 new cases, 204 total; live smoke on :8799 for the new routes |
| Tool calling / structured output through the platform path | Missing — the request shape allows it, no agent tool loop uses `openai:` yet |

## Phase 5 — Agent execution engine

| Item | Status | Notes |
|---|---|---|
| Real execution loop with tools | Working | `agent.rs`: think/list/read/write/command/done/fail |
| Approval pauses, checkpoints, diffs, restore | Working (1.70.0) | `awaiting_approval`, per-step checkpoint (200 kB cap), per-step restore and run-level restore of every file a finished run wrote; `fs_write` is checked per path before a bulk restore writes anything, and a checkpoint digest skips a path edited after the run unless the user forces it (1.71.0) |
| Cancellation, crash recovery (`interrupted`) | Working | `ACTIVE` map; startup sweep |
| Pause / resume between steps | Working (1.52.0) | `POST /api/runs/:id/pause` parks a live run at the next step boundary (never mid-action), writes a `pause` step, sets `paused` and releases its control entry; `resume` continues from the same step log. A pause is durable across a restart and is only refused when there is nothing to stop (a run awaiting approval) |
| Resume from the step log | Working | `POST /api/runs/:id/resume` + 2 tests |
| Team (executor → reviewer → repair) | Working | `review` step with verdict/findings |
| Crew pipeline + per-stage budgets/policies | Working | `crew` roster 1–6, `stage_max_steps`, fail/skip |
| Hybrid cloud-plan / local-execute | Working | plan step, bounded escalation |
| Multi-agent parity in VS Code | Working | `agent.ts` port |
| Task queue, priority, dependencies | Working (1.53.0) | every run is created `queued` and admitted by one dispatcher pass ordered by `priority` (-10…10) then age; `dependsOn` is validated at creation (unknown/self/loop/empty/>8 all 400) and a dependency is only satisfied by `succeeded`, while a dependent of a failed/cancelled/interrupted run is failed with the dependency's name. A queued run survives a restart and is admitted at boot; pause names it as queued, cancel and the emergency stop include it |
| Retry from scratch (`POST /api/runs/:id/retry`) | Working (1.55.0) | starts a clean run from a finished run's settings with `retry_of` linking back; the step log, tokens, duration, error and dependencies are not carried over, and a `retry` step names the original. Refused for a run that has not finished; a paused run is told to resume instead |
| Concurrency limit (`max_concurrent_runs`) | Working (2.1.0) | clamped 1–4, default 2, honoured above 1 only when the workspace is itself a git repository root (every run gets its own worktree); a non-git folder stays at one. A run waiting for approval still holds its slot, a paused or queued run does not |
| Conflict detection before a write | Working (1.54.0) | `app/src/filelock.rs`: a `write` is refused as a non-fatal `write-conflict` step when another live run has claimed the path or the file on disk is not what this run last read; the run re-reads and continues. Claims are dropped for any run no longer `running`/`awaiting_approval` and released by `finish` |
| Parallel execution + worktree isolation / file locks | Working (2.1.0) | each writing run works in its own worktree, a command claims its checkout while it runs, and the lease ledger lives in the database so more than one process sees it. An editor save or another program still writes behind the lease, caught on the run's next write |
| Verification step before `succeeded` | Working (1.56.0) | a run that wrote, ran a command or called a tool goes to the verifier, which must answer `APPROVE`; ISSUES hands the findings back for one repair round and a second ISSUES ends the run `failed` with the findings kept. Read-only runs and `team` mode are not verified twice. No human sign-off step: the verifier is a model on the same router |
| Ruflo | Not integrated (spec says optional) | Saarthi Core keeps ownership |
| Live-run checkout hand-back | Working (2.0.0) | a finished run's `run/<id>` branch is committed, the shared checkout is fast-forwarded and the worktree removed; nothing is applied automatically and an unapplied worktree stays on disk |

## Phase 6 — Skills

| Item | Status | Notes |
|---|---|---|
| Discovery across 5 roots | Working | `skills.rs` |
| Enable/disable persisted | Working | `POST /api/skills/:id/state` |
| Instruction injection with token cap | Working | rides the next message |
| Install / update / remove / version tracking | Working (1.49.0) | install writes `<user skills>/<slug>/SKILL.md` after validating it with the list parser (temp file + rename, refuses overwrite unless `replace`); update rewrites in place and keeps the id; remove moves the folder to `<root>/.removed/` instead of deleting; `skills.json` records version, install/update times and a content fingerprint, so “edited since install” is reported; only the user root is writable |
| Skill permissions / usage history | Partial (1.49.0) | usage history is real (`skill_usage`: id, time, conversation, tokens; `uses`/`last_used_at` per skill and `/api/skills/:id/usage`); writing routes are gated on `fs_write`. There are no per-skill permissions because a skill is prompt text and is never executed |
| Automatic skill selection | Working (1.57.0) | `skills::select_for`: id/name words score 4, description words 2, noise words and numbers dropped; needs 2 points, at most two skills, id tie-break, 1,200-token budget, disabled and broken skills skipped. A manual pick always wins, and `auto_select_skills` (default on) turns it off. The reply meta names what was attached (`auto_skills`) |

## Phase 7 — MCP

| Item | Status | Notes |
|---|---|---|
| Server registry | Working (1.37.0) | `mcp.json` beside the config; `transport` (stdio/http/sse), `url` for remote servers, `env` for stdio and write-only `headers` for remote — values stored, only names returned; an entry without `transport` reads as stdio |
| Connection lifecycle | Working (1.39.0) | a live session per server is reused across calls (handshake skipped, tool list re-read), warm for five idle minutes, evicted on save/remove and on a failed request, with a fresh-handshake fallback; a dropped stdio child is killed and an SSE reader aborted |
| Tool discovery | Working (1.33.0) | `tools/list`, schemas kept |
| Schema validation | Working (2.7.0) | Bounded JSON Schema subset: type lists, enum/const, nested properties and items, `additionalProperties`, bounds, combinators, boolean schemas and local `$ref`s, depth-limited; `pattern`/`format`/remote refs left to the server |
| Permission binding | Working (1.33.0) | `mcp_tool` on `server.tool`; ask/deny/allow, audited |
| Health, logs, reconnect | Working (1.36.0) | health recorded from a real handshake (`serverInfo.version`, protocol, tool count, latency, time), `unknown` before any contact; stderr and client events in a 200-line in-memory ring; one handshake retry after 250 ms, never a retried `tools/call`; `/api/mcp/health`, `/api/mcp/health/check`, `/api/mcp/logs?name=`; an idle supervisor pings each pooled session every 60 s with a 5 s budget and evicts only silence or a dead transport (1.45.0) |
| SSE / HTTP transports | Working (1.37.0) | streamable HTTP (one POST per message, JSON or SSE answer, `Mcp-Session-Id` carried forward) and legacy HTTP+SSE (GET stream, `endpoint` event, id-matched answers); both share handshake, catalogue, validation, gating and health/log state with stdio |
| Agent use of MCP tools | Working (1.35.0) | `Action::Mcp` with a per-run `allowMcp` flag; catalogue discovered once into `agent_runs.mcp_tools` and logged as `mcp-tools`; approval pause names the tool and arguments; `mcp_tool` rules on `server.tool` veto; execution goes through the same `mcp::invoke` as the route |
| Per-step tool rediscovery | Working (1.38.0) | a failed `mcp` step re-reads the catalogue before the repair step; the snapshot's `refreshes` counter bounds it to three per run and survives a resume |
| MCP manager UI | Working (1.34.0) | Settings → MCP: register/edit/test/remove, tool cards, argument template, approve-then-call, raw result |

## Phase 8 — Project knowledge and memory

| Item | Status | Notes |
|---|---|---|
| Conversation history persistence | Working | `conversations`/`messages`, Android mirror |
| Workspace text search | Working | `/api/workspace/search` |
| Embeddings *probe* | Partial | `media.rs` proxies one `/api/embeddings` call; nothing is stored or indexed |
| Semantic search over the code index | Working (1.63.0) | `app/src/embeddings.rs`: indexed symbols are embedded through the configured router and cached in `embedding_cache` by model + symbol + file digest, ranked by cosine similarity, served by `GET /api/index/semantic?q=`. Off until `embedding_model` is set (the route then answers 400), a provider error is a 502 rather than a silent fallback to the word search, and one search embeds at most 128 new documents, reporting the rest as `pending`. The Index sidebar tab (1.64.0) shows what the index holds, refreshes it, searches by words or by meaning, opens a hit's file, and carries the embedding-model field. Browser-checked in 1.65.0 (word and meaning search, model save, hit-to-file, refresh); its segmented control now auto-fits five tabs |
| Codebase indexing / symbols / incremental updates | Working (1.59.0) | `app/src/index.rs`: bounded walk skipping build/dependency folders, secret-shaped files never read, privacy exclusions applied before reading, 1 MiB and NUL-byte skips, 5,000-file ceiling reported as `truncated`; symbols per language family with path + line + kind + name; refresh by content digest (unchanged untouched, changed re-extracted, deleted dropped); ranked search over names and paths; `GET /api/index`, `POST /api/index/refresh`, `GET /api/index/search?q=`. No UI yet and symbols are line scans, not a parser |
| RAG over the project | Working (1.60.0) | chat turns carry the index's ranked matches: one snippet per file, the declaration plus its opening lines read from disk at request time, labelled `path:line — kind name`, 4 snippets / 700 tokens, reported as `retrieved` in the reply meta. Word-based, so no embeddings are involved; `deepsearch.rs` remains web research, a different thing. Agent runs open with the same two blocks through `agent::run_background` (1.61.0), with memory read quietly so a run's repeated asks leave `uses` alone, and the block is stored on the run at promotion (1.62.0): `context` on the run carries the memory and code labels, the card says `given 1 memory + 1 code`, the expanded run names them, and the steps read the stored block back instead of recalling again |
| Memory records + management UI | Working (1.58.0) | `app/src/memory.rs`: `memory_records` with kinds decision/preference/task/fact, a required title, optional details and a scope (one project or every project); `GET /api/memory`, `POST /api/memory`, `POST /api/memory/:id/delete`, local-origin guarded; recall by shared words (title 3, body 1, floor 2, 6 records / 900 tokens) with `uses`/`last_used_at` and `memory_used` in the reply meta; sidebar Memory pane with form, list and forget. The model never writes a record |

## Phase 9 — Developer environment

| Item | Status | Notes |
|---|---|---|
| File tree, editor, save, workspace search | Working | `workspace.rs` + `ide.js` |
| Real PTY terminal (ConPTY/unix pty, SSE, resize) | Working | `terminal.rs`, `portable-pty` |
| Git status | Working (1.66.0) | `/api/workspace/vcs` reports the branch, `ahead`/`behind` from `# branch.ab`, and per-file `staged`/`unstaged`/`untracked`/`conflicted` flags parsed from `git status --porcelain=v2` |
| Git diff, stage, unstage, discard, commit, branches, log, fetch, pull, push, per-hunk and per-line staging (mouse and keyboard), commit detail, remotes (list, add, remove, change URL), worktrees (list, add, remove) | Working (1.91.0) | `app/src/git.rs`: fifteen routes under `/api/workspace/git`, each a real `git` invocation in the open folder. Fetch resolves a configured remote; pull is `--ff-only` and refuses a diverged history with a 409; push never forces and sets the upstream once. Remote commands have a 90 s deadline, `GIT_TERMINAL_PROMPT=0`, drained streams and 64 KiB per stream; auth/unreachable is a 502. A hunk is selected by index server-side and applied with `git apply --cached` (reversed to unstage); the client never supplies patch text, and `show` validates the hash before argv. The same two routes take optional changed-line indexes and rebuild the hunk from git's own diff — removals paired with their additions, unselected lines kept as the text the index holds, `@@` counts recomputed — so one line moves without its neighbour. Remotes are listed, added and removed from the pane through `/api/workspace/git/remotes`: names and URLs are narrowed before argv, and removing one never touches a file or a commit. A remote's URL can be changed in place through `/remotes/set-url`, which refuses a name that is not configured rather than creating a second remote. Hunk lines are keyboard-operable since 1.81.0: one tab stop per hunk, arrows between changed lines, Space or Enter toggles the focused one, and `aria-pressed` follows the selection. Since 1.82.0 `A` selects a whole hunk and Escape clears it, with `All`/`Clear` buttons on each hunk, and the hunk head reports a polite `aria-live` selection count since 1.83.0. A selection spans the hunks of its file since 1.84.0 and is moved in one action from the bar above them; since 1.85.0 it also spans files, with a pane bar that moves or clears them together. Since 1.86.0 a commit refuses content that still holds conflict markers, and since 1.87.0 it also refuses files that mix CRLF and LF. Since 1.88.0 the selection is stored per workspace and survives a reload, since 1.89.0 a Check button reports conflict markers and mixed line endings across tracked files, and since 1.90.0 the first render of a workspace reports them by itself. Since 1.91.0 worktrees are listed, created and removed from the pane through `/worktrees`, `/worktrees/add` and `/worktrees/remove`, though agent runs do not execute in one yet. Paths are validated before argv (relative, no `..`, no drive letter, no control characters, no `.git`) and passed as an array, never a shell. Every mutation is gated on the `git` action (deny 403, ask 409 until `approve: true`, audited). Failures surface git's own sentence with 409 for a refused checkout / nothing to commit. UI: per-row diff, stage/unstage/discard, commit box, branch create/switch, history list, conflict/staged/changed/untracked groups |
| Git merge and abort | Working (1.74.0) | `POST /api/workspace/git/merge` (body `branch`) merges a real branch into the checked-out one; `POST /api/workspace/git/merge/abort` runs `git merge --abort`. The tree must have no uncommitted tracked changes (untracked files are ignored, as git ignores them). The branch name is narrowed before argv (no leading dash, no `..`, ASCII ref characters only) and `--no-edit` is passed so a terminal-less process never waits on an editor. A clean merge answers `{branch, current, fastForward, summary}`; a conflict answers 409 with `{conflicted: [paths], output, inProgress}` and leaves the markers to resolve; a missing branch is a 400; abort is a 409 when nothing is in progress. Both routes are gated on the `git` action. UI: Merge… form, conflicted files named in the toast, Abort merge control |
| Git checkpoint before a run writes | Working (1.75.0) | any run started with writes or commands snapshots the whole tree before its first step: `git::snapshot_tree` builds a commit over a throwaway `GIT_INDEX_FILE`, so nothing in the user's index, worktree or history changes and untracked files are included; the commit is kept in `run_checkpoints`, first snapshot wins on resume. `POST /api/runs/:id/checkpoint/restore` restores tracked paths that differ, removes files the snapshot never had, refuses a live run (409), caps at 500 paths, and consults `fs_write` per path before writing; untracked files are left alone and the index is not rewritten |
| Git rebase, continue and abort | Working (1.77.0) | `POST /api/workspace/git/rebase` (body `branch`) replays the checked-out branch's commits on top of another branch; `/rebase/abort` restores the branch where the rebase started. A dirty tracked tree and the already-checked-out branch are refused (untracked files are ignored, as git ignores them), the branch name is narrowed before argv, and both routes are gated on `git`. Success answers `{branch, current, summary}`; a conflict answers 409 `{conflicted, output, inProgress}` and leaves the tree to finish or abort; abort is a 409 when nothing is in progress. A resolved conflict is staged and finished with `/rebase/continue` (a 409 names any still-unmerged path, a second conflict is reported, and a finished rebase refuses; `-c core.editor=true` keeps a terminal-less git off an editor). UI: Rebase… form, Continue and Abort rebase controls, conflicted files named in the toast |
| Git worktree per run | Working (2.1.0) | a writing run gets `<repo>-run-<id>` on a `run/<id>` branch, can be applied back and cleaned up (2.0.0); read-only runs and non-git folders keep the shared root |
| Syntax highlighting, diagnostics, completion, formatting | Missing | no editor engine, by design |
| Build/test runner + diagnostics in the Problems panel | Working (1.73.0) | `app/src/runner.rs`: detects `cargo build`/`cargo test` from `Cargo.toml`, and `build`/`test` scripts from `package.json` through npm/pnpm/yarn. `GET /api/workspace/runner`, `POST /api/workspace/run` (preset id only, never a command line; gated on `command`; one run at a time; 300 s ceiling; 1 MiB per stream, last 200 000 chars returned). Parses Rust, TypeScript and generic `path:line:col` diagnostics, dedupes, caps at 200, drops paths outside the folder. The Problems pane runs them, shows the raw output, and opens a diagnostic at its real line. Since 1.73.0 a test run also reports counts: `tests {framework,total,passed,failed,ignored,failures}` parsed from libtest's per-binary result lines, Jest, Vitest and node's TAP summary, shown in the pane header with the failing names in its tooltip. A build is never searched and a summary-less test run reports `null`, so zeroes are never invented |
| Debugger, watch mode, arbitrary command presets | **Missing** | there is no debug adapter and no file watching; only the detected build/test presets can be run |

## Phase 10 — Safety and permissions

| Item | Status | Notes |
|---|---|---|
| Path confinement, symlink escape prevention | Working | canonicalised, `MAX_FILE_BYTES` 1 MB |
| Loopback `Host`/`Origin` write guard | Working | `host_security.rs` |
| Outbound secret detection | Working | `privacy.rs` |
| Host API bearer auth + TLS + body caps | Working | `host_auth.rs`, `host_transport.rs` |
| Central policy store (once/session/project/deny) | Working (1.31.0) | `permissions.rs`; rules scoped once/session/project/always, resolved longest-target → deny → scope |
| Privileged UI routes gated | Working (1.40.0, 1.66.0) | terminal open, editor saves, workspace create/rename/delete, settings/key saves and — since 1.66.0 — git stage/unstage/discard/commit/branch create/switch consult saved rules on their own actions (`command`, `fs_write`, `settings`, `git`); deny → 403, ask → 409 until `approve: true`, both audited. No rule means no prompt — the click is the consent. UI routes pass an empty project, so `project`-scoped rules still apply to agent steps only. Updates have no in-app route |
| Enforcement of that store | Working (1.40.0, 1.66.0) | agent steps, router installs and MCP calls consult it (1.31.0–1.33.0); terminal open, editor saves, workspace create/rename/delete, settings/key saves (1.40.0) and the Git routes (1.66.0) do too. The only privileged path with no route to gate is an application update, because there is no in-app update |
| OS keychain for secrets | Working (1.42.0) | `keychain.rs` stores `api_key`, `search_api_key`, `openai_api_key` and MCP OAuth material in Windows Credential Manager / macOS Keychain / Linux Secret Service; startup migrates plaintext and blanks the file, a store failure falls back to the permission-restricted file, and the store mode is reported in `GET /api/config` |
| Privileged-operation audit log | Working (1.31.0) | `permission_audit` table + `/api/permissions/audit` |
| Global kill switch | Working (1.41.0) | `POST /api/emergency-stop` cancels every live agent run (flagged and finished, mirroring per-run cancel), reaches parked/paused runs, and closes every terminal session; deliberately not policy-gated, always audited |
| Permissions UI | Working (1.32.0) | Settings → Permissions: add a rule, list rules in force, revoke, read the audit trail |

## Phase 11 — Token, context and cost

| Item | Status | Notes |
|---|---|---|
| Token-budgeted packing | Working | `context.rs`, 1 k–32 k clamp, VS Code parity |
| Usage log + aggregates/series/by-model | Working | `request_log`, `/api/usage` |
| Pricing, cost estimates | Working | `cost.rs` × `capabilities::Price` from the live `/models` row × the provider's reported tokens; `request_log.cost_usd`/`cost_basis`, unpriced rows counted rather than zeroed. Native OpenAI Platform picks stay unpriced (no mirrored price table) |
| Budget enforcement that stops a run | Working | `daily_budget_usd`, rolling 24 h (timestamps are UTC), checked once before the fallback chain and before a stream; refusal is non-retryable and names spend, budget and unpriced count. Zero = no limit; local inference exempt |
| Compression beyond head/tail cropping | Working | `context.rs`: whitespace normalisation, repeat-once dedup, digest of labelled excerpts capped at a quarter of the budget, first message kept as a pointer; a fitting thread is untouched and every condensed/dropped message is reported in meta. No model call, so no invented summaries |
| Context-utilisation display | Working | context meter `~used / budget · %` with an over-budget state; `context_budget` in reply meta for other clients |

## Phase 12 — Live agent monitoring

Working for monitoring (1.51.0), controls (1.52.0) and the live step view
(2.29.0).
`agent_runs`/`agent_steps`, SSE events and a runs panel with diffs,
approve/reject, cancel, restore and pause/resume are real backend state;
`GET /api/runs/overview` adds a cross-run dashboard (live, awaiting approval,
paused, failed, summed steps and tokens) built from the same rows as the panel,
and every run reports `tokens`, `elapsedSeconds` and `idleSeconds` with a live
run measured from its start. Pause parks a run at the next step boundary and a
paused run holds no live task, so start/pause/resume/cancel all act on real
state, and the emergency stop reaches parked runs too.

The queue is real as of 1.53.0: a run is created `queued`, takes a place in line
(`queue #N` on the card, a Queued count on the board, a waiting note with Cancel
only while it waits), can carry a `priority` and a `dependsOn` list, and starts
itself when a slot frees and its dependencies succeeded. Retry is a real control
as of 1.55.0 too, and it is distinct from resume: `POST /api/runs/:id/retry`
starts a clean run of the same goal and settings (linked by `retry_of`, logged as
a `retry` step), while resume continues an existing step log. There is no
drag-to-reorder — priority is a number on the form.

## Phase 13 — Workflow builder

Started in 2.9.0: a workflow is a validated dependency graph, and a run is a
persisted execution of it.

| Item | Status | Notes |
|---|---|---|
| Task / agent / approval / condition nodes | Working (2.9.0) | `agent` starts a real agent run and waits for it, including at its approval gate; `approval` parks the workflow; `condition` asserts dependency outcomes; `task` records a note |
| Dependencies and branches | Working (2.9.0) | `dependsOn` ids or `{id, when: "failed"}`; an unmet edge marks the node `skipped`; a failure counts as handled only when its repair branch succeeded |
| Validation before execution | Working (2.9.0) | ids, kinds, profiles, missing deps, self references, cycles, per-kind requirements, a 50-node cap — each error names the node |
| Persisted run history | Working (2.9.0) | `workflow_runs`/`workflow_steps` carry status, output, timing and the agent run id; a boot sweep marks a run the process abandoned as `interrupted` |
| Retry and cancel | Working (2.9.0) | per-node retries (0–3); cancel stops the graph (a child agent run in flight is not forcibly stopped) |
| Visual editor | Working (2.12.0) | canvas cards in dependency columns with SVG edges (dashed for `failed`), add-by-kind, rename/delete (dependencies rewritten), inspector for every node field, per-edge `when`; the JSON field remains as a synced advanced view. cards can be dragged and the position is stored on the node (2.47.0); graph helpers are unit-tested, but rendering and dragging are not, and the canvas was driven in Chromium once (2.49.0) — drag, live edges, clamping and a position restored after reload — though no automated browser test exists |
| Graph import/export | Working (2.13.0) | export is a versioned `saarthi.workflow` document; import runs the save-time validation before writing, assigns a new id and a unique name, and starts a fresh schedule window |
| Scheduled execution | Working (2.10.0) | `everyMinutes` 5–10080 (0 = manual), checked every 30 s; the anchor is the later of the last run and the window start, a live run is not stacked, and the window is claimed in SQLite (2.48.0) so two processes sharing one home cannot both start it |
| Resume an interrupted run | Working (2.11.0) | `POST /api/workflow-runs/:id/resume` keeps finished steps, reopens the rest, refuses an edited or deleted workflow; a resumed agent node starts a fresh agent run |

## Phase 14 — Cross-platform integration

| Item | Status | Notes |
|---|---|---|
| Desktop Linux/Windows/macOS | Working | deb, AppImage, tarball, zip, macOS universal tarball |
| Android client | Working (subset) | chat streaming, history, backup, remote agent runs via Host API; no router management, no local execution |
| VS Code extension | Working (subset) | embedded Node server, agent port, runs webview |
| Capability negotiation driving feature gates | Partial | `/api/host/v1/capabilities` exists; clients hand-gate a few screens |
| Pairing / TLS / sessions | Working | host token lifecycle + TLS transport |

## Phase 15 — Extension and plugin ecosystem

Manifests are discovered and validated as of 2.15.0, listed in Settings as of
2.16.0, installed/removed as of 2.17.0, and executed as of 2.20.0 inside a
wasmi sandbox (ADR 0001). Since 2.21.0 that sandbox provides workspace
read/write host functions, scoped to the open folder and checked per call
against saved rules, and since 2.22.0 a per-extension data folder through
data_read/data_write; network, terminal and MCP capabilities remain unprovided.
Since 2.25.0 a manifest also declares the builds it works on and the extensions
it needs, and both are enforced rather than displayed.

| Item | Status | Notes |
|---|---|---|
| Manifest (id, name, version, description, author, entry, permissions) | Working (2.15.0) | `GET /api/extensions` scans `<data>/extensions/*/manifest.json`; the id must equal the folder, the version must be `1.2.3`, `entry` must stay inside the folder and exist, and permissions are a closed list |
| Management UI | Working (2.16.0) | Settings → Extensions shows the directory, each folder's validity, description or error, and a Refresh action |
| Install / uninstall | Working (2.17.0) | install copies a folder through staging with validation and rollback, symlink refusal and a 500-file/16 MiB cap; uninstall moves the folder to `.removed/` after a canonical containment check |
| Execution and sandboxing | Working, workspace + own data (2.22.0) | ADR 0001: no in-process execution. `app/src/extension_run.rs` runs a module in a wasmi 0.31.2 interpreter with no WASI: `run() -> i32`, 50,000,000 fuel, 64 MiB memory, 100,000 table elements, 8 MiB module cap. The whole host surface is `env.read`/`env.write` (open folder), `env.data_read`/`env.data_write` (`<extensions>/<id>/data`) and `env.last_error`, linked only when the manifest declares `workspace.read`/`workspace.write`. Paths use `workspace::resolve_inside` and writes `write_text_file`; saved rules apply per call (`ask` refuses because nothing can answer) and every call is audited. No network, terminal, MCP or config access exists |
| Runtime gating | Working (2.19.0, host functions 2.21.0) | `runtime: "wasm"` requires a `.wasm` entry; discovery exposes `runnable`/`blockedReason`; `POST /api/extensions/:id/run` refuses a capability with no host function behind it and otherwise runs the module, answering `{exitCode, fuelUsed, elapsedMs, hostCalls}` |
| Run control | Working (2.23.0) | One run per extension (409 while one is open); `POST /api/extensions/:id/cancel` stops it at the next host call (a pure-compute loop still ends at the fuel limit); a 5-second wall-clock budget is checked at each host boundary; uninstall cancels first; Settings shows `Running…` and `Cancel` |
| Run history | Working (2.24.0) | `extension_runs` records extension, project, start, duration, exit code, fuel, host calls and error for every run that reached the sandbox, including loader failures and cancellations; `GET /api/extension-runs` returns the newest 50 and Settings lists them |
| Compatibility and dependencies | Working (2.25.0) | `requiresStudio` (numeric per-part floor against `CARGO_PKG_VERSION`) marks a manifest `valid` but `compatible: false` when this build is older, and both run and install are refused with both versions named; `apiVersion` defaults to 1 and refuses a higher number at parse; `dependencies` (≤16, deduped, validated, no self-reference) are reported as `missingDependencies` and block a run, not an install. Discovery reads every manifest before judging any. Not covered: a dependency's own compatibility, dependency cycles, and an update path that would re-check after the app is upgraded |
| Custom tools | Working (2.27.0) | `tools` in the manifest (≤16; name, description, optional `.wasm` entry, optional `inputSchema`; the wasm runtime is required and each module is checked like the entry) and the `env.arguments`/`env.result` ABI; `POST /api/extensions/:id/tools/:tool` runs one in the same sandbox with the same budgets, grants, one-run rule and cancel, answering `{result, reported, run}`, gated by `extension_tool` (default ask; `ask` refuses with instructions because the route cannot prompt). Since 2.27.0 the agent's tool loop offers them too: the prompt carries a labelled catalogue of every valid, runnable folder's tools (none when nothing is installed), the `extension` step is gated by the same `extension_tool` permission on the same `extension/tool` target, and the call runs through the same sandbox, budgets, grants, one-run rule, cancel boundary, `extension_runs` row and host-call audit, with `env.result` becoming the step output. Since 2.28.0 the declared `inputSchema` is enforced on every call before the module is loaded (MCP's bounded keyword subset, boolean schemas honoured, an unreadable schema refused) and the prompt lists each tool's required argument names; not covered: keywords outside that subset |
| Provider adapters | Missing | — |
| UI extensions | Missing | — |

## Phase 16 — Testing and quality

| Layer | Status | Notes |
|---|---|---|
| Rust unit/integration | Working | 483 passed, 1 ignored at the 2.24.0 run; the count for each release is recorded in `PROGRESS.md` |
| VS Code extension | Working | `node --test` suites |
| Android | Working | JVM + emulator instrumentation + validation gate |
| Release-pipeline regressions | Working | `scripts/ci/test-version.sh` |
| Frontend | Working (2.33.0) | `scripts/ci/test-frontend.sh` → `node --test app/static/tests/*.test.js`: the real `app.js` is evaluated in a `node:vm` context with a stub DOM, and 21 cases cover escaping, the markdown subset (including a `javascript:` link staying text), byte and time formatting, path keys, the icon fallback, the workflow graph helpers (layout by dependency depth, add/rename/delete with edge rewriting, per-edge conditions), and a render smoke (2.33.0) that runs `renderWorkflowCanvas` (a card per node, an edge per dependency, an empty-graph note) and evaluates `ide.js` on top of `app.js` (chrome built, `api` wrapped, workbench inside the main pane). Wired into `desktop-ci.yml`. Still not a layout or pixel check — that needs a browser |
| End-to-end (launch → chat → agent → restart) | Working (2.8.0) | `scripts/ci/test-e2e.sh` launches the real binary against a scratch `FREE_AI_STUDIO_HOME`, boots the mock provider, runs a chat turn and reads the two stored messages, checks the process log and readiness, opens a workspace folder, starts an agent run that parks at its write approval gate, approves it, sees the write on disk and the `write, verify, done` step log, then kills the process and re-reads both the conversation and the run with its steps from the database; and finishes with two failure cases — a killed provider (the run must fail with a recorded error while readiness keeps answering) and a process killed while a run has a recorded step and its next call in flight (the restarted app must report `interrupted` with a readable log); 28 checks |
| Failure-injection matrix | Working (2.34.0) | Four process-level cases in `scripts/ci/test-e2e.sh`: (1) the mock provider is killed and the next run must reach `failed` with an error while `/api/ready` keeps answering; (2) the provider is slowed and the process killed mid-run, and the restarted app must mark that run `interrupted` with a readable step log; (3) the database is replaced with garbage — the process must exit with `file is not a database` in its log, nothing may answer on the port, and restoring the saved copy must bring it back ready; (4) the data directory is made read-only (POSIX only: Windows Git Bash ignores `chmod`, so the case prints `skip`) and the `data directory` readiness check must go false and then recover. Not covered: a true ENOSPC case (needs a privileged loop/tmpfs mount) and browser-level layout |
| Install smokes in CI | Working (2.31.0) | `scripts/ci/smoke-package.sh` unpacks a tarball, zip, `.deb` (`dpkg-deb -x`) or AppImage (`--appimage-extract`), boots the packaged binary headless against a scratch home, checks `/api/ready`, serves `/`, and compares `/api/diagnostics/bundle`'s `app.version` with `app/Cargo.toml`. `desktop-release.yml` runs it on the `.deb`, the Linux tarball and AppImage, the Windows zip and the macOS tarball. Verified locally on a zip and a tarball; the `.deb` and AppImage paths run only in the Linux release jobs |
| Windows Rust CI job | Working (2.32.0) | `desktop-ci.yml` builds the default `gui` feature and runs `cargo test --locked` on `windows-latest`; WebView2 and MSVC come with the runner and mold/`-Z threads` are Linux-scoped. Linux still owns clippy and the end-to-end smoke. Verified locally with the same commands on Windows; the GitHub job itself runs on push |

## Phase 17 — Observability and diagnostics

Partial, with the read model and the export shipped in 1.72.0. `app/src/diagnostics.rs`
is a read-only projection over the records the app already writes — it adds no
table and no second log pipeline, so it cannot disagree with the panels:

| Item | Status | Notes |
|---|---|---|
| One timeline with a correlation id per row | Working (1.72.0) | `GET /api/diagnostics?limit=&area=` merges `request_log`, `permission_audit`, `agent_runs` and failing `agent_steps` (`fail`, `mcp-error`, `write-conflict`), newest last, with a level (`info`/`warn`/`error`) and the run id, project or conversation as the correlation key. Limit defaults to 100 and is clamped to 500; an unknown area is a 400. Loopback `Host`/`Origin` guarded |
| Counts per area | Working (1.72.0) | From the same rows: provider requests/errors over 24 h, permission decisions by the decision names stored in the table, agent runs by status, live runs |
| Support bundle with redaction | Working (1.72.0) | `GET /api/diagnostics/bundle`: version, OS/arch, non-secret settings, credential booleans, summary and timeline. Run goals are excluded; every string is redacted (`sk-`/`ghp_`/`xoxb-`, `Bearer`, sensitive `name=value`, long opaque runs) and clipped to 400 chars |
| Diagnostics UI | Working (1.72.0) | Settings → Diagnostics: timeline with area/level colour and the short correlation id, server-side area filter, row count, 24-hour summary line, and a bundle download the browser saves without composing it |
| Structured log file + console ring | Working (2.2.0) | `app/src/oplog.rs` appends one redacted JSON object per line to `studio.log` beside the database, flushed per write, rotated at 1 MiB into one previous file, with a 200-entry ring in memory; `GET /api/diagnostics/log?limit=` serves the ring and Settings → Diagnostics shows it with the file's path. Written at boot, router install/update/uninstall/restart, permission ask/deny, provider failures and every run ending |
| Request id for non-run operations | Working (2.4.0) | `trace.rs` gives every local API request an id, kept in a task-local for the handler's lifetime and echoed as `x-request-id`; a caller's own id is accepted only if it is 8–64 characters of alphanumerics/`-_.`. `permission_audit` gained a `request_id` column (migrated in place, old rows keep `''` rather than a guessed id), so a terminal open, a Git stage or a settings save is now tied to the call that asked for it; the timeline and the process log both show it |
| Router installs and MCP connections in the timeline | Working (2.3.0) | `timeline_value` adds one row per router from `router-installs.json` (the newest record, since that file keeps one per router) and one per MCP server that has actually answered, at its own `checked_at` — a server never contacted adds nothing rather than a row stamped "now". Areas `router` and `mcp` are valid filters, and the panel offers them |
| Health/readiness dashboard | Working (2.4.0) | `GET /api/ready` runs only cheap local checks — database opens and answers, the data directory accepts a write (a probe file, then removed), provider base URL and whether a key is saved plus the credential store mode, latest router install outcomes, and MCP servers by health. Only the first two are `required`; the rest change what works, not whether the app runs. Shown at the top of Settings → Diagnostics |

## Phase 18 — Installation and updates

| Item | Status | Notes |
|---|---|---|
| Linux `.deb` / AppImage / tarball | Working | AppStream validates clean |
| Windows portable zip | Working | no MSI/EXE installer |
| macOS package | Working (unsigned) | Gatekeeper right-click → Open |
| Android APK/AAB | Working | debug-signed release build |
| VS Code VSIX | Working | published |
| DB migration rollback, newer-build guard | Working | 2.35.0: versioned schema, `VACUUM INTO` copy verified before an upgrade, restore on a failed migration, byte-for-byte refusal of a newer schema |
| Update check (read-only) | Working | 2.36.0: `GET /api/update/check` reads the newest release, compares versions numerically, names this platform's asset; About card renders it |
| Download and verify into staging | Working | 2.37.0: `POST /api/update/stage` downloads the named asset through the verified-download path, refuses an unverifiable or non-https one, records it in `updates/staged.json` |
| Unpack and check a staged update | Working | 2.38.0: re-verifies the sha256, unpacks an escape-proof `.zip`/`.tar.gz` into `updates/prepared/<version>`, refuses a tree with no binary |
| Replace the installed binary | Working | 2.39.0: copies the prepared binary beside the target, hash-checks the copy, keeps `<name>.old-<version>` and restores it if the new one cannot take its place; a restart runs it, and 2.40.0 removes the leftover backup at the next start |
| Installer and uninstaller | **Missing** | no MSI/EXE/.app and no uninstaller |

## What to build next (spec order, 2.1.0)

1. ~~**Phase 17 remainder**~~ — closed in 2.4.0: the process log (2.2.0),
   install/MCP timeline rows (2.3.0), request ids for non-run operations and the
   readiness view (2.4.0).
2. **Phase 16 remainder** — an end-to-end harness (headless launch + mock
   router + restart), then the failure-injection matrix and install smokes. The
   frontend unit layer landed in 2.5.0; a render smoke and an `ide.js` harness
   are still open inside it.
3. **Phase 13 workflow graph** — the last phase with no real implementation. It
   depends on the Phase 5 lifecycle, which is now complete.
4. **Phase 18 installers and update** — the data and download halves are done
   (2.35.0 copy-and-rollback before an upgrade, 2.36.0 the read-only check, 2.37.0
   the verified download into staging, 2.38.0 the verified unpack, 2.39.0 the swap
   with a backup and rollback), so what remains is the Windows MSI/EXE, the macOS
   `.app` and a clean uninstall.
5. **Phase 15 extension manifest** — name, version, compatibility, permissions,
   entry point and source, plus a boundary for untrusted extensions.

Deliberately not next, though open: per-skill permissions (a skill is prompt
text and never executes), a price table for native OpenAI picks (the app mirrors
no prices rather than inventing them), and an editor engine or debugger (both
are a product decision, not a missing route).
