# Engineering metrics

`GET /api/engineering/metrics` separates what was measured from what is unavailable. Every
number is counted from a table this app writes, or read from the workspace.

## Measured

| Field | Source | Window |
| --- | --- | --- |
| `runs.total/succeeded/failed` | `agent_runs` | 30 days |
| `missions.total/succeeded` | `missions` | 30 days |
| `tasks.total/done` | `mission_tasks` joined to missions | 30 days |
| `retries` | `mission_messages` rows starting "Retrying after failure" | 30 days |
| `verifications` | `agent_steps` with action `verify` | 30 days |
| `testCommands` | `agent_steps` action `command` whose summary mentions test/check/build | 30 days |
| `failovers` | `agent_steps` action `failover`/`escalate` | 30 days |
| `pipelines` | `engineering_pipelines` | 30 days |
| `coverage` | first `lcov.info` found, `LH/LF` totals | current files |
| `project.*` | the bounded scan | current files |

## Unavailable, and why

- **Security findings** — no scanner is configured, so the field is `null` rather than a score.
- **Coverage** — `null` when no lcov file exists.
- **Test results** — `testCommands` counts command steps, not parsed test cases; the note says
  so, and the mission report remains the authority on what ran.

The dashboard prints these notes next to the tiles. Nothing here estimates a percentage from
guesswork: `100%` appears only when every counted row says so.
