# Autonomous engineering — architecture

Phase 14 adds one module, `app/src/engineering.rs`, on top of what already exists. It is a
**reader and a gate**, not a second orchestrator.

## What it reuses

| Need | Existing system | How engineering uses it |
| --- | --- | --- |
| Running agents | `agent.rs` | Missions create runs through `agent::create_run_with`, with the same queue, budget, approvals and profile tools |
| Coordination | `multi_agent.rs` | A pipeline *is* a mission with a fixed plan; dependency order, retries, pauses, restart recovery and the report all come from the mission supervisor |
| Git | `git.rs` | `create_branch_checked` and `commit_checked` wrap the same validated, guarded operations the Source control pane calls |
| Permissions | `permissions.rs` | `guard_user` for `Action::Git` and `Action::Command`, with the same audit trail |
| Code understanding | `index.rs` | The analysis stage reads files; the index and symbol search stay the source of truth for navigation |
| Persistence | SQLite | Three tables only: settings, the profile cache and pipelines (plus a small check log) |

There is exactly one supervisor (`multi_agent::advance`), one run loop (`agent.rs`) and one set
of git operations. Engineering adds project intelligence, requirement intake, the autonomy
policy, the acceptance trace and the delivery step.

## Module layout

- `scan` — a bounded walk (20 000 files, depth 12, `.git`/`node_modules`/`target`/… skipped)
  producing languages, source/test/doc counts, manifests, frameworks and entry points.
- `analyse_requirement` — decides whether a request can be planned, writes deterministic
  acceptance criteria and records the risks the profile implies.
- `engineering_plan` — the six stages: analyse → plan → implement → verify → document → review.
- `project_profile` — the cached profile, keyed by git HEAD + dirty + file counts, refreshed
  after five minutes. A non-git workspace is always rescanned.
- `trace` — which acceptance criteria have evidence, and which do not.
- `checks` — runs the project's own check command behind the command permission.
- `metrics` — counts rows this app already recorded; unavailable values stay `null`.
- `cancel_all` (in `multi_agent`) — the emergency stop.

## Data

```sql
engineering_settings(id, autonomy, allow_branch, allow_commit, updated_at)
engineering_projects(path, fingerprint, profile, analyzed_at)
engineering_pipelines(id, mission_id, goal, autonomy, questions, acceptance,
                      profile, branch, baseline, delivery, created_at, updated_at)
engineering_checks(seq, pipeline, kind, status, summary, output, created_at)
```

A pipeline row never stores a copy of the mission; the mission remains the only record of what
ran. `pipeline_json` reads the mission on every request, so status, tasks, report and counts
cannot drift from the run log.

## Boundaries

- Engineering does not write files itself. The coding and documentation agents do, through the
  run tool loop, so every edit has a step, a run and a task behind it.
- Engineering does not merge, push, rebase or delete branches. Branch creation and commits are
  the only git actions it offers, both guarded.
- The UI never computes a status the server did not send.
