# Changelog

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
