# Tool permissions

A role never grants a capability. It *narrows* the capabilities its agent profile
already has, and the mission's own switches narrow them again.

```
role (multi_agent.rs)  ⊂  agent profile (agent.rs)  ⊂  mission switches  ⊂  permission rules
```

The effective rule is the intersection:

| Role | Profile | Reads | Writes files | Runs commands |
| --- | --- | --- | --- | --- |
| planner | planner | yes | no | no |
| architect | planner | yes | no | no |
| research | planner | yes | no | no |
| coding | coding | yes | yes | yes |
| documentation | coding | yes | yes | no |
| debugging | debugger | yes | no | yes |
| testing | testing | yes | no | yes |
| review | review | yes | no | no |

`allowWrites` / `allowCommands` on the mission are applied on top, so a mission
created without write permission cannot produce a writing task at all — the API
refuses a plan that needs writes with *"This plan edits files, so allow writes for
the mission or choose a read-only workflow."*

## Where enforcement actually happens

1. **Run creation** — `agent::create_run_with` validates the profile and stores
   `allow_writes` / `allow_commands` on the run row.
2. **Every action** — the run loop calls the permission gate before any `write`
   or `command`. The gate consults, in order: the run's flags, the profile's own
   limits (`profile_can_write`, `profile_can_command`), saved rules, and the
   approval policy. A `write` from a `review` run is denied regardless of the run's
   checkbox.
3. **Approval** — a write or command that the policy says to ask about parks the
   run at `awaiting-approval` with a pending action. `autoApprove` on the mission
   answers that prompt the same way the Runs page's "approve" does; it does not
   bypass a deny rule.
4. **Plugin tools** — extension tools carry their own permission
   (`extension_tool`) and allow rules, and remain refused inside mission runs
   unless a rule exists.

## Audit

Every action becomes an `agent_steps` row (`action`, `summary`, `output`,
`checkpoint`). The mission report quotes those steps:

* `filesWritten` — the `write` step summaries of every task run;
* `commandsRun` — the `command` step summaries;
* `checks` — verifier steps.

The dashboard's mission detail also links each task to its run id, so the full
step log is one click away on the Runs page.

## Cancellation and rollback

* Cancelling a mission calls `agent::stop_run` per live run, which sets the cancel
  flag and records `cancelled` through the same path as a person cancelling.
* Rollback uses the run's own checkpoints (`restore` on the Runs page). A mission
  does not add a second rollback mechanism; it would be a second source of truth.
