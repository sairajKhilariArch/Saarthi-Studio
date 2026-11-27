# Agent SDK

The agent surface is small on purpose: a role is a name, a purpose, a profile and
two capability flags. Everything else — the loop, the tools, the permissions — is
the existing agent engine.

## Adding a role

```rust
Role {
    id: "migration",                     // stable id used by plans and the API
    name: "Migration",                   // shown in the dashboard
    purpose: "Move data and schema without losing rows.",
    profile: "database",                 // the profile that owns the real rules
    writes: true,
    commands: true,
},
```

Then add it to `ROLES` in `app/src/multi_agent.rs` and cover it in
`every_role_maps_to_a_real_agent_profile` (the test asserts the profile exists)
and `only_the_builder_roles_may_write`.

Rules for a new role:

* `profile` must be one of `agent::PROFILES`; a role never invents tool rules.
* `writes` may only be true when the profile can write, `commands` only when the
  profile can run commands. The intersection is what runs.
* The purpose becomes prompt text in every task the role receives — write it as
  an instruction, not a job title.

No other change is needed: the supervisor, templates and dashboard read `ROLES`.

## Using the API

### Create a mission

```http
POST /api/missions
{
  "goal": "Add CSV export to the reports page and document it",
  "template": "feature",
  "parallel": 2,
  "maxRetries": 1,
  "allowWrites": true,
  "allowCommands": true,
  "autoApprove": false
}
```

`202`-shaped success payload:

```json
{ "ok": true, "mission": { "id": "…", "status": "planned", "tasks": [ … ] } }
```

Refusals come back as `{ "error": "…" }` with the reason: no open workspace, a
plan that needs writes without `allowWrites`, an unknown template, a role switched
off, an invalid custom plan, or "Describe what the team should accomplish."

### Inspect

| Route | Returns |
| --- | --- |
| `GET /api/missions` | the 30 newest missions, each with its tasks and report |
| `GET /api/missions/:id` | one mission with task detail and live run status |
| `GET /api/missions/:id/messages` | the typed message log |
| `GET /api/missions/:id/report` | the frozen report (built on demand while running) |

### Control

| Route | Effect |
| --- | --- |
| `POST /api/missions/:id/pause` | stop starting new tasks |
| `POST /api/missions/:id/resume` | continue (also clears an approval wait) |
| `POST /api/missions/:id/cancel` | stop live runs and cancel remaining tasks |
| `POST /api/missions/:id/advance` | one supervisor tick, useful for scripting |
| `POST /api/missions/:id/tasks/:task/approve` | release an approval checkpoint |
| `POST /api/missions/:id/tasks/:task/cancel` | stop one task |
| `POST /api/missions/:id/remove` | delete a terminal mission and its log |

### Per-role models

```http
GET  /api/mission-roles
POST /api/mission-roles   { "role": "testing", "model": "deepseek/deepseek-r1", "enabled": true }
```

An empty `model` means "routed default". The pinned model is passed as the run's
task-scoped model, so it reaches the provider as the request's `model` field; the
provider decides whether it supports what the step needs. A retry after a failure
drops the pin and falls back to the default.

## The report

```json
{
  "missionId": "…",
  "goal": "…",
  "template": "feature",
  "status": "succeeded",
  "elapsedSeconds": 42.1,
  "tasks": [ { "id": "implement", "role": "coding", "status": "done", "attempts": 0,
               "runId": "…", "summary": "wrote hello.txt", "error": "" } ],
  "filesWritten": ["write hello.txt (15 bytes)"],
  "commandsRun": ["command echo tests-ok"],
  "checks": ["verify …"],
  "tokens": 4213,
  "context": { "workspace": "/…", "projectId": "…" }
}
```

Every array is quoted from the runs' own step log; nothing in the report is
composed by the orchestrator.
