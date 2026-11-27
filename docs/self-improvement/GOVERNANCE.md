# Security and governance

## What the learning system may not touch

The settings table has no column for permissions, credential paths, approval requirements or
release credentials, and no endpoint accepts them. Parsing ignores unknown keys. Concretely, the
module cannot:

- add or remove a permission rule or widen a tool allowlist;
- change where credentials live or how they are read;
- lower an approval level, skip a review or auto-approve a mission task;
- write to the running binary, installers or release artifacts;
- push, tag or publish.

The autonomy setting is a ceiling for *newly created* pipelines, not a permission bypass: the
existing mission approval gates and the `Action::Git` decisions still apply.

## Audit trail

`improve_events` records every governance-relevant action with timestamp, kind, subject and
detail: settings saved, scan run, proposal approved/rejected/rolled back, experiment started and
completed, preferences recorded and lessons forgotten. The report renders the latest events.

## Isolation

- Proposals are implemented by an engineering pipeline on a branch created with
  `git::create_branch_checked`; the working tree is checked for conflicts first.
- The pipeline's delivery stages only the files the pipeline wrote.
- Rollback requires a clean worktree and creates a revert commit.

## Human approval

- Learning is off until enabled.
- Proposals need explicit approval; there is no timer or auto-approve path.
- Rollback needs `{"approve":true}`.
- Experiments are read-only missions, so they cannot modify files even when autonomy is high.

## Emergency stop

`POST /api/improve/stop` cancels live improve missions through `multi_agent::cancel_all` and
stops their runs through the existing run cancellation, returning both counts. It does not
delete lessons or proposals, and it does not disable learning — stopping and disabling are
separate, deliberate actions.

## Integrity limits

Signed artifacts and reproducible-build attestation are provided by the existing release
pipeline, not by this module. Self-improvement cannot produce or verify release signatures and
does not claim to.
