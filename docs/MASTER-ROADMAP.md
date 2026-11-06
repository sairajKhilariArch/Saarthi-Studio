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

## Phase 1 — CI/CD and release reliability (in progress)

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
- AppStream metadata (`app/packaging/free-ai-suite.appdata.xml`) shipped into
  the AppImage.

Remaining for Phase 1 sign-off:

- [ ] A tagged run where **all** jobs succeed, including macOS.
- [ ] macOS runner availability resolved (queue starvation/cancellation).
- [ ] Install smoke tests executed on the produced artifacts (deb install,
      AppImage run, zip extract + launch, vsix install, apk install).
- [ ] Decision recorded for the three dead tags (`v1.26.0`, `v1.26.1`,
      `v1.26.2`) — leave documented as failed, or delete after v1.26.4 ships.

## Phase 2 — Router manager

1. Replace the hard-coded FreeLLMAPI/OmniRoute paths in `router.rs` with a
   registry of descriptors (id, name, OS/arch support, install methods, min
   version, config location, health probe, start/stop/update/uninstall).
   **Done (1.26.9)** — `router_registry.rs`; update/uninstall remain part of (3).
2. Add detection: installed? version? runtime? healthy? owned?
   **Done (1.26.9)** — `/api/routers`, rebuilt from the existing probes;
   `healthy` is "the probe answered".
3. Add an approval-gated install flow with checksum verification. **Mostly done
   (1.26.18–1.26.21)** — plan preview with measured prerequisites, a digest-bound
   approval, recorded installs, registry-digest-pinned images for both gateways
   and verified release downloads (streamed through sha256, unpacked only on a
   match). All three routers have a real path, plus an update check, a gated
   update that pulls before replacing, and an uninstall that keeps data unless
   told to purge it (1.26.23). A config editor remains.
4. Add a startup sweep honouring per-router auto-start preferences, and keep
   the ownership rule (never stop what the app did not start). **Done (1.26.17)**
   — preferences in `router-prefs.json`, sweep starts only an enabled, installed,
   not-already-answering router, so nothing is ever adopted.
5. UI: list, status, logs, connection tester, update/uninstall. **Partial** —
   list, health, start/stop (1.26.13), connection tester and restart (1.26.15),
   container logs (1.26.16); update, uninstall and a config editor still open.

## Phase 3 — Model and provider manager

1. Capability table per model (context, modalities, tools, reasoning,
   structured output) sourced from provider metadata, never invented.
2. Model selection at global/agent/project/task scope with documented
   precedence.
3. Health checks, timeouts, retry policy with idempotency guard, circuit
   breaker, explicit fallback chain.

## Phase 4 — Thinking + OpenAI Platform

1. Native OpenAI Platform provider using the Responses API behind the existing
   provider abstraction; keys stay server-side.
2. Thinking control (`off|low|medium|high|model default`) mapped only to
   parameters the selected model supports; unsupported choices rejected with a
   clear message.
3. `@Thinking` composer token: activates the control, shows level + model,
   preserves scope, and gives a literal-text escape.
4. Surface summaries/progress — never claim access to hidden chain-of-thought.

## Phase 5 — Agent execution engine

1. Full task lifecycle (`Created → Queued → Planning → WaitingForApproval →
   Executing → Verifying → Completed/Failed/Cancelled`) with persistence and
   resume, building on `agent_runs`/`agent_steps`.
2. Task queue with priority, dependencies, retry, cancellation, checkpoints.
   *Delivered 1.53.0 for the queue (`queued` state, priority, dependencies and
   in-order admission behind `max_concurrent_runs`) and 1.55.0 for retry as its
   own control alongside resume; checkpoints and cancellation already existed.*
3. Role-specialised agents with explicit tools/permissions/budgets.
4. Parallel execution guarded by worktree isolation or file locks; conflict
   detection before write.
   *Partially delivered 1.54.0: conflict detection before a `write` and leases
   between live runs (`app/src/filelock.rs`). Worktrees, a lease that also covers
   command steps and the editor, and a cross-process ledger are still open.*
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

Registry + connection lifecycle + tool discovery + schema validation +
permission binding through the Phase 10 engine + health/logs/reconnect.

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
say what it was given. The embeddings index behind a semantic version of this
remains open.*

## Phase 9 — Developer environment

Editor niceties that do not require a bundled editor engine, real git
operations (status/branch/diff/stage/commit/worktree checkpoints), build/test
runner with a problems panel, and diagnostics tied to real file locations.

## Phase 10 — Safety and permissions

Central policy store covering files, directories, terminal, network, MCP,
router install, updates, git, secrets, settings; scopes: once/session/project/
always-deny; OS keychain for secrets; audit log; global emergency stop.

## Phase 11 — Token/context/cost

Accurate usage attribution, context utilisation display, compression that
preserves structure, budget enforcement that can stop a run, provider pricing
with "estimate" labelling when usage is not reported.

## Phase 12 — Live monitoring

Backend-truth dashboard: agents, task, model, router, tokens, duration, tool,
files touched, approvals, errors, with start/pause/resume/cancel/retry/logs.

Delivered: the cross-run board and per-run timing/token facts (1.51.0),
pause/resume of a live run at a step boundary (1.52.0), and the task queue
(1.53.0) — a queued pill, `queue #N` on the card, a Queued count on the board, a
waiting note with Cancel only, and priority / depends-on on the run form — which
completes the control set against real state, queued runs included. Remaining: a
live stepping view of the current step while a model call is in flight, and any
reordering beyond a numeric priority.

## Phase 13 — Workflow builder

Validated node graph (task/agent/model/tool/conditional/approval), dependency
resolution, retry, scheduling with a visible scheduler, and run history.

## Phase 14 — Cross-platform

Capability negotiation driving feature gates; Android remote execution and
router management; VS Code agent parity; consistent API contracts.

## Phase 15 — Extension ecosystem

Manifest (name, version, compatibility, permissions, deps, entry point,
source), sandboxing for untrusted extensions, custom agents/skills/MCP/tools.

## Phase 16 — Testing

Frontend unit tests, end-to-end runs (launch → chat → agent → approve →
restart), failure injection matrix, platform matrices, and release-artifact
install smokes in CI.

## Phase 17 — Observability

Structured logs with correlation IDs per task/request, exportable support
bundle with secret redaction, health/readiness endpoints, error surfaces in UI.

## Phase 18 — Installers and updates

Windows MSI/EXE, macOS `.app`/dmg (signed or clearly documented as unsigned),
update mechanism preserving user data, DB migrations with rollback, clean
uninstall.
