# Safety and the approval model

## Rules the implementation enforces

1. **No duplicate orchestration.** Pipelines are missions. One supervisor, one run loop.
2. **No fabricated activity.** A pipeline status is the mission's status. A task is done only
   when its run reached `succeeded`. The trace labels a criterion `verified` only when a task
   or command produced evidence *and* every stage finished; otherwise it says
   `in-progress`, `no-artifact` or `not-verified`.
3. **No silent edits to user changes.** Delivery stages only the paths in the mission report's
   `filesWritten` list, and calls the same commit helper the Source control pane uses, which
   refuses staged conflict markers and mixed line endings. Nothing else in the tree is staged.
4. **No uncontrolled repair loop.** Retries are a mission setting (0–3) and a retry is one
   fresh run; the run itself is bounded by its step budget. A repair pipeline is a four-stage
   mission, not a loop.
5. **No permission bypass.** Commands go through `Action::Command`, git through `Action::Git`,
   both recorded in the audit trail. Agents use the profile tools they already had.
6. **No deployment.** There is no deploy, publish or release action in the module.

## Approvals in practice

- Level 1: each stage waits. Approve in Mission control (`/api/missions/:id/tasks/:task/approve`).
- Level 2: edits wait; analysis, planning, checks and review do not.
- Level 3: the plan runs. Branch and commit are audited decisions.
- Cancelling a mission or the emergency stop needs no approval; both are reversible in the
  sense that nothing is deleted.

## Baseline and recovery

A pipeline records its starting `branch@commit` in `baseline`. The mission keeps per-run
rollback checkpoints, and the report quotes which files were written. Because delivery is a
normal commit, `git revert` or the Source control undo path remains available to the user.
