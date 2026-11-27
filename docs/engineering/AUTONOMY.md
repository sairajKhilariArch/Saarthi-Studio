# Autonomy levels

The level is saved once in `engineering_settings` and applied when a pipeline is created. It
maps onto the mission's own approval flags — there is no second permission system.

| Level | Task approval | Meaning |
| --- | --- | --- |
| 1 — Assisted | every task `needsApproval` | the supervisor stops before each stage; the person starts it |
| 2 — Supervised | tasks whose role writes (`coding`, `documentation`) | read-only analysis, planning, testing and review run; edits wait |
| 3 — Autonomous | no forced approvals; `autoApprove` on the mission | the plan runs end to end, with checkpoints in the log |

A pipeline may set its own level (`autonomy` in the create body); otherwise the saved level
applies. The mission's profile rules still apply on top: a role without the `write` tool cannot
write at any level, and a command still passes the command permission.

## What still asks at level 3

- `POST .../deliver` and `.../branch` call `permissions::guard_user(Action::Git, …)`. At levels 1
  and 2 they refuse without `approve: true`; at level 3 the pipeline's autonomy is the
  authorisation and the decision is still audited.
- Destructive operations are not exposed by engineering at all: no force-push, no branch
  deletion, no history rewriting, no merge, no deploy.

## Emergency stop

`POST /api/engineering/stop` cancels every live mission (each with the tick lock, so a tick
cannot resurrect a cancelled mission) and calls `agent::cancel_all_active`. It never asks for
approval: stopping is the safe direction. The response reports how many missions and runs were
stopped; the pipelines read back as `cancelled`.
