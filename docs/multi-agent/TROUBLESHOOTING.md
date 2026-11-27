# Troubleshooting missions

## "The mission was refused"

| Message | Cause | Fix |
| --- | --- | --- |
| Open a workspace folder first | no space is open | open a folder or pick a space |
| This plan edits files, so allow writes… | template needs writers, `allowWrites` is off | tick **allow file edits**, or use `audit` |
| The {Role} role is switched off… | role disabled in Mission control | re-enable it, or plan without that role |
| Two tasks share the id / Unknown agent role / cycle | the custom plan is invalid | fix the JSON; the message names the task |
| A mission may have at most 12 tasks | plan too wide | split the work into two missions |

## A task sits at `planned` and nothing starts

1. Check its dependencies: a task only starts when every dependency is `done`. A
   `failed` or `blocked` dependency blocks it permanently.
2. Check `parallel`: if the limit is reached, later tasks wait.
3. Check conflicts: another writing task is running and one of them declares no
   `files`. Declare disjoint files or raise nothing — wait for it.
4. Check the mission status: `paused` and `awaiting-approval` mean the supervisor
   is intentionally idle.

## A run says "Calling an extension tool asks first"

A mission run inherits the extension tool rules. Save an allow rule for
`extension_tool` in Settings → Permissions, exactly as for a manual run.

## "Blocked because …"

The task's dependency failed or was cancelled. The fix belongs to the dependency:
retry it (the mission's retry policy already tried), or cancel the mission and
plan again with a corrected task.

## The mission is `failed` but every task looks done

Read `error`: it names the task ids that did not finish. The report lists the same
ids with their status and error. A `failed` mission never hides a failed task
behind a green summary.

## A run failed with "The AI router is not configured" / provider errors

The mission cannot fix credentials. Configure the provider on the Settings page,
then retry the task: `POST /api/missions/:id/tasks/:task/cancel` is not the tool
for this — cancel the mission and start a new one, or let the retry policy run
after fixing the provider.

## A pinned model does not appear on the wire

* The model input must not be blank (blank means the routed default).
* Adaptive routing, when configured, decides the tier for a step and wins over
  the run's model for that step.
* On a retry after a failure, the pin is deliberately dropped and the routed
  default is used. The assignment message says "falling back from …".

## The app was closed mid-mission

Reopen it. Runs that died are marked `interrupted`, the supervisor restarts and
applies the retry policy. Nothing is re-executed silently, and a task that already
succeeded stays done.

## "The mission stalled: no task could start."

No task was running, none was planned and the mission was not settled — the graph
cannot make progress. This indicates a plan bug (a dependency chain that can never
be satisfied); the report lists each task's final status. Start a new mission.

## Where to look

* Mission control page → task list, message log, report.
* Runs page → the run behind a task id, its steps and checkpoints.
* `GET /api/missions/:id/messages` → the exact sequence the orchestrator wrote.
