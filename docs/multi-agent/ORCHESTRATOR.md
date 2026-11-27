# Orchestrator

`advance(state, mission_id)` is one supervisor tick. The supervisor runs it every
120 ms until the mission settles, pauses or waits for a person. The same function
is exposed as `POST /api/missions/:id/advance`, which the dashboard's **Advance
now** button uses.

A global async mutex serialises ticks, so a restart sweep, the supervisor and a
manual advance can never both read the same `planned` task and start it twice.

## The tick, in order

### 1. Settle finished runs

For every `running` task with a run:

* `succeeded` → task `done`, summary taken from the run's own first `done` step
  (the agent's words, not a template), completion message.
* `failed` / `interrupted` / `cancelled` → if attempts remain and the run was not
  cancelled, the task returns to `planned` with the error in its history and a
  `request` message; otherwise it becomes `failed` and an `error` message is
  recorded.

A retry is a **new agent run**, not the same run resumed, and the failure is part
of the retry's goal. When the role has a pinned model, the retry drops it and
falls back to the routed default — spending a retry on the same broken assignment
would help nobody.

### 2. Block impossible work

Any `planned` task with a dependency in `failed`, `blocked` or `cancelled` becomes
`blocked` with the reason. Blocked tasks count as settled for the mission
outcome and appear in the report.

### 3. Approval checkpoints

If a `planned` task has `needsApproval` and has not been approved, the mission
status becomes `awaiting-approval`, a `request` message is addressed to the user,
and the tick returns without starting anything. `POST
/api/missions/:id/tasks/:task/approve` marks the task approved, resumes the
mission and restarts the supervisor.

### 4. Start ready work

Tasks are considered in plan order. A task is ready when every dependency is
`done`. A task starts when:

* the mission's `parallel` limit (1–4, default 2) has room; and
* it does not conflict with a running task (see below).

Starting a task builds the goal (see [`CONTEXT.md`](CONTEXT.md)) and calls
`agent::create_run_with` with:

| Run field | Value |
| --- | --- |
| `profile` | the role's fixed profile |
| `allowWrites` | mission allows writes **and** the role writes |
| `allowCommands` | mission allows commands **and** the role runs commands |
| `autoApprove` | the mission's choice |
| `maxSteps` | 24 |
| `model` | the role's pinned model, when set |
| `dependsOn` | the run ids of the dependency tasks |
| `priority` | 5 (mission work outranks background runs, not the user's own) |

If the run is refused — no workspace, project budget exhausted, unknown model
settings — the task fails with the refusal as its error and the mission carries
on with the rest of the graph.

## File-conflict rule

Two tasks conflict when they may write the same file at the same time.

* Two read-only tasks never conflict.
* A writer that declares no files conflicts with every other writer: it could
  touch anything.
* Two writers may run together only when both declare `files` and the sets are
  disjoint. `src/lib.rs` and `src/lib.rs/inner` overlap; `src/lib.rs` and
  `docs/x.md` do not.
* A read-only task may run beside a writer; a reader never changes files.

Declaring `files` is therefore the way to unlock parallelism for writing work,
and the dashboard's custom-task help says so.

## Settling the mission

The mission settles when every task is `done`, `failed`, `blocked` or
`cancelled`:

* all done → `succeeded`;
* any `failed`/`blocked` → `failed`, with the failing task ids in `error`;
* any `cancelled` → `cancelled`.

The report is built **after** the outcome is known (so `report.status` matches
the mission), stored in `missions.report`, and a `report` message is addressed to
the user.

If no task is running, none is planned and the mission is not settled, the graph
can never progress: the mission is failed with "The mission stalled: no task
could start." rather than polling forever.

## Cancellation

`POST /api/missions/:id/cancel` marks every unsettled task `cancelled`, calls
`agent::stop_run` for each live run, builds the report and sets the mission to
`cancelled`. The response reports how many runs were actually stopped.

## Pause and resume

`pause` sets the mission to `paused`; running runs are left alone (no work is
destroyed mid-step), and the supervisor stops on its next tick. `resume` returns
it to `running` and restarts the supervisor.

## Restart recovery

`resume_after_restart` runs at boot and restarts the supervisor for every mission
still `running` or `planned`. Tasks keep their real `run_id`; the agent layer's
own `mark_interrupted` sweep already decided which of those runs died with the
process, so the next tick settles them honestly instead of re-running work that
may have succeeded.
