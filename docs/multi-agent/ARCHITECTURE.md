# Multi-agent architecture

Sarthi Studio coordinates eight specialised agents through a **mission**: one
request, a plan of tasks, and one delegated agent run per task.

The important design decision is that the mission layer owns *coordination only*.
It never executes a tool itself. Every task becomes a normal agent run created
through `agent::create_run_with`, so the run inherits the open workspace, the
project token budget, the queue and concurrency limits, the dependency graph, the
approval gate and the per-profile write/command rules that a run started from the
Agents page gets. Nothing is looser because an agent asked for it.

```
user request
    │
    ▼
mission (multi_agent.rs)          plan: tasks + dependencies + declared files
    │
    ├── planner      ── run (profile planner, read-only)
    ├── architect    ── run (profile planner, read-only)
    ├── coding       ── run (profile coding, writes)
    ├── debugging    ── run (profile debugger, commands, no writes)
    ├── testing      ── run (profile testing, commands, no writes)
    ├── review       ── run (profile review, read-only)
    ├── documentation── run (profile coding, writes)
    └── research     ── run (profile planner, read-only)
```

## Layers

| Layer | File | Responsibility |
| --- | --- | --- |
| Roles | `app/src/multi_agent.rs` | The eight agents, their profile, tools and defaults |
| Mission store | `app/src/multi_agent.rs` | `missions`, `mission_tasks`, `mission_messages`, `mission_roles` |
| Supervisor | `app/src/multi_agent.rs` | Ticks the mission: settles runs, retries, blocks, starts ready work |
| Runs | `app/src/agent.rs` | The real agent loop: one model call, one action, one logged step |
| Tools | `app/src/agent.rs`, `permissions.rs`, `extensions.rs` | File/command/search tools, approvals, plugin tools |
| UI | `app/static/index.html`, `app.js` | Mission control page, timeline, message log, report |
| API | `main.rs` | `/api/missions*` and `/api/mission-roles`, behind the local-origin guard |

## Why the mission does not run tools

An orchestrator that could write files would be a second, unchecked permission
system. Delegating every action to a run means:

* the permission gate, the audit log and the step history stay in one place;
* a mission can be reviewed, cancelled and rolled back with the same tools a
  person's run uses;
* the code-review step is looking at the same checkpoints and steps the Runs page
  shows, not at a parallel log that could drift.

## Storage

```sql
missions(id, project_id, goal, template, status, error, parallel, max_retries,
         allow_writes, allow_commands, auto_approve, created_at, updated_at,
         context, report)
mission_tasks(mission_id, id, role, title, goal, depends_on, files, status,
              run_id, attempts, max_attempts, summary, error, needs_approval,
              approved, position, created_at, updated_at)
mission_messages(seq, mission_id, task_id, kind, from_role, to_role, body, created_at)
mission_roles(role, model, enabled, updated_at)
```

`missions.report` holds the frozen JSON report written when the mission settles.
`mission_tasks.run_id` is the link to the live run; the task's status is the
mission's record, and the run's status is what actually happened.

## Guarantees

1. A task is `done` only after its run reached `succeeded`.
2. A task whose dependency failed is `blocked`, never silently retried into
   success.
3. Two writing tasks that could touch the same file never run at the same time.
4. Every status change, retry, block, approval and report is a typed message in
   `mission_messages`.
5. A cancelled mission stops its live runs through the same `stop_run` path the
   Runs page uses.

## Related documents

* [`ORCHESTRATOR.md`](ORCHESTRATOR.md) — the supervisor loop, step by step
* [`AGENT-SDK.md`](AGENT-SDK.md) — adding a role
* [`AGENT-LIFECYCLE.md`](AGENT-LIFECYCLE.md) — task states
* [`PROTOCOL.md`](PROTOCOL.md) — the message kinds
* [`CONTEXT.md`](CONTEXT.md) — what an agent is told, and what it is not
* [`TOOL-PERMISSIONS.md`](TOOL-PERMISSIONS.md) — the authority chain
* [`WORKFLOWS.md`](WORKFLOWS.md) — feature, debug, audit and custom plans
* [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) — when a mission does not move
