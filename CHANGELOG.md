# Changelog

## 4.4.0

### Self-improving development environment
- Add **measured self-evaluation**: one report aggregates missions, tasks, retries, first-attempt success, recovery, review rejections, tokens, tool errors, per-role and per-model performance, workflow density and maintenance state from rows the app already wrote. A rate without a denominator is `null` and printed as `not measured`, never zero.
- Add **verified engineering experience**: a lesson is stored only when a mission is terminal, succeeded and left an artifact (a file write, a command or a verifier check); failures are stored as normalised failure keys, user preferences are the only records trusted without a mission, unvalidated rows are hidden by default, confidence grows with use and stops below certainty, and deduplication plus workspace scoping keep retrieval honest.
- Add **privacy and retention controls**: goal sentences are stored only when the user opts in (otherwise a digest key), every row is scoped to one workspace, retention sets an expiry and purge removes exactly the expired rows.
- Add **deterministic improvement proposals**: a scan detects duplicate top-level definitions, files over 1200 lines, documentation naming missing paths and a missing test suite, and writes proposals with problem, evidence, benefit, risk, files, testing and rollback. Approving starts a Phase 14 engineering pipeline on an isolated branch; rollback creates a guarded revert commit.
- Add **controlled experiments**: two read-only missions (analyse → check → review versus check → review) over the same goal, compared success-first with tokens, elapsed time and task counts, labelled `sampleSize: 1`, and marked `inconclusive` when verification was dropped.
- Add **model advice**: measured runs, success, average tokens, duration and estimated cost per model, ranked under a user-chosen priority (balanced, reliability, speed, cost) with a stated reason. Advice never pins a model or spends tokens; unpriced models are labelled instead of guessed.
- Add **risk assessment** with an explicit heuristic label and evidence lines, a **maintenance** section, and a **governance trail** of settings changes, scans, approvals, rejections, rollbacks, experiments and deletions.
- Add the **Improve dashboard** with quality tiles, agent and model tables, model advice, workflow recommendations, proposals, experiments, knowledge search and preferences, risk, maintenance and event log, plus an emergency stop.
- Add `docs/self-improvement/` (architecture, experiences, agents, workflows, models, proposals, experiments, governance, prediction, metrics, developer guide), 16 backend tests and 4 frontend tests.

## 4.3.0

### Autonomous engineering
- Add **project intelligence**: a bounded workspace scan reports languages, source/test/documentation counts, manifests, frameworks, declared dependencies and entry points, cached against the git fingerprint and refreshed when HEAD or the file counts move.
- Add **requirement intake**: a request that names nothing is planned by asking at most two questions instead of guessing; every pipeline records deterministic acceptance criteria and the risks the project profile implies. A `needs-input` pipeline calls no model.
- Add **engineering pipelines** on top of missions: analyse → plan → implement → verify → document → review, with the analysis facts in every plan and a trace that labels each acceptance criterion `verified`, `no-artifact`, `in-progress` or `not-verified` from the mission's own evidence.
- Add **autonomy levels** 1–3 mapped to mission task approvals: assisted gates every stage, supervised gates stages that change files, autonomous runs the authorised plan. Branch creation and commits are `Action::Git` decisions that still require approval at levels 1 and 2.
- Add **checks and repair**: `cargo check` behind the command permission with classified failures and suggestions, and a bounded four-stage repair mission (diagnose, fix, verify, review) that never self-loops.
- Add **delivery**: a delivery branch named from the goal and a commit that stages only the files the pipeline itself wrote, through the same guarded git helpers the Source control pane uses.
- Add **measured metrics**: run, mission, task, retry, verification and command counts over 30 days, plus lcov coverage when a file exists; unavailable values are labelled instead of estimated.
- Add the **Engineering dashboard** with the project profile, metrics, autonomy control, pipeline launcher, question intake, stage list, acceptance trace, checks, delivery and an emergency stop that cancels live missions and runs.
- Add `docs/engineering/` (architecture, pipelines, autonomy, safety, debugging, testing, git, metrics, developer guide) and unit plus scripted-provider integration tests for intake, approval gating, a full six-stage run, delivery refusal and the emergency stop.

## 4.2.0

### Multi-agent missions
- Add **Mission control**: one request becomes a plan of tasks, and every task is delegated to the specialised agent whose role fits it — planner, architect, coding, debugging, testing, code review, documentation and research. A task is a real agent run created through the same entry point as a run you start yourself, so the workspace, budget, queue, approval gate and per-profile tool rules all apply unchanged.
- Add the mission supervisor: dependency graph, parallel execution under a writer-conflict rule (two tasks that may write the same file never run together), retries that create a fresh run with the failure in its goal, dependents blocked instead of silently skipped, approval checkpoints, pause/resume/cancel with cancellation propagated through `stop_run`, restart recovery, and a frozen report whose task, file and command lists are quoted from the runs' own step logs.
- Add a typed agent message log (assignment, completion, error, request, status, report) with the orchestrator as the only writer outside user actions, so coordination is traceable and cannot loop.
- Add per-role model assignment at `GET/POST /api/mission-roles`; the pinned model is sent as the run's task-scoped model, and a retry after a failure falls back to the routed default.
- Agent runs now honour the run's own model for its steps (adaptive routing still wins for steps it classified), so one agent in a team can run on a different model from another.
- Add the Mission control dashboard: role cards with model pins, a create form for the feature/debug/audit workflows and custom task lists, mission cards with real progress, task timeline with dependencies and run ids, the message log, the report, and pause/resume/cancel/approve controls.
- Add `docs/multi-agent/` (architecture, orchestrator, agent SDK, lifecycle, protocol, context, tool permissions, workflows, troubleshooting) and unit plus orchestration tests against a scripted provider, including parallel start order, blocked dependents, approval hold, cancellation and a pinned model reaching the wire.

## 4.0.0

### Reliability
- Reject malformed JSON from successful router responses instead of converting it into a fake assistant answer.
- Reject successful JSON responses that do not contain supported chat message content.
- Return a clear gateway error for malformed one-shot streaming responses.
- Add regression tests for invalid JSON, missing content, plain text and structured text parts.

### Release
- Start the Sarthi Studio desktop 4.x major release line.
- Keep Android, VS Code and website versioning independent.
- Move historical 3.x release notes to [the archived changelog](docs/release-history/CHANGELOG-3.x.md).

## Previous releases

Historical 3.x release notes are preserved in [CHANGELOG-3.x.md](docs/release-history/CHANGELOG-3.x.md).
