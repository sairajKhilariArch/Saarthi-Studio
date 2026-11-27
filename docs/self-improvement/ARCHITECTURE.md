# Self-improvement — architecture

Phase 15 adds one module, `app/src/improve.rs`, on top of the plugin ecosystem (12),
multi-agent missions (13) and autonomous engineering (14). It is a **reader, a gate and a
scheduler of existing work**, never a second orchestrator, agent or memory system.

## What it reuses

| Need | Existing system | How self-improvement uses it |
| --- | --- | --- |
| Execution history | `multi_agent.rs` missions and `agent.rs` runs | Every metric is a SQL aggregate over missions, tasks, runs and step logs that already exist |
| Learning trigger | `multi_agent::advance` | On a terminal mission state the supervisor calls `improve::observe_mission`, so lessons are written exactly once per finished mission |
| Self-modification | `engineering.rs` | An approved proposal starts `engineering::create_for_goal`; the pipeline builds an isolated branch, writes, verifies, documents, reviews and delivers |
| Git safety | `git.rs` | Rollback uses `git::revert_checked` (clean worktree, committed HEAD, `Action::Git` guard); delivery uses `commit_checked` |
| Permissions | `permissions.rs` | Settings have no permission, credential or approval fields; proposal rollback and git actions go through the same guards |
| Storage | SQLite | Five tables, one workspace project key, no files outside the database |

There is still one supervisor, one run loop, one git layer and one audit log. The module adds
evaluation, experience rows, proposals, experiments and a governance trail.

## Module layout

| Part | Responsibility |
| --- | --- |
| `Settings` / `settings_json` | Governance switches: enabled, autonomy ceiling, retention, experiment budget, goal privacy, model-advice priority |
| `report` | One snapshot: quality, agents, models, model advice, workflow, failures, maintenance, experiences, proposals, events, unavailable |
| `observations_from` | Pure function that turns one finished mission into validated lessons or failures |
| `store_observation` | Dedupe by workspace + kind + normalised key, then confidence from use count |
| `scan` | Deterministic repository findings become proposals (`fn`/`function` duplicates, oversized files, docs naming missing paths, no tests) |
| `approve_proposal` | Creates the engineering pipeline at the configured autonomy level; refuses when disabled or already started |
| `start_experiment` / `complete_experiment` | Two read-only missions (baseline and lean) compared from their own reports |
| `risk_score` | Heuristic band with explicit evidence lines |
| `router` | The `/api/improve/*` endpoints, behind `guard_local_origin` |

## Data flow

1. A mission reaches `succeeded`, `failed`, `cancelled` or `stalled`.
2. `observe_mission` reads the mission JSON plus its report, derives a lesson only when the
   status is terminal *and* succeeded *and* an artifact exists (file write, command or check);
   failures are stored as `failure` rows with a normalised error key.
3. Confidence is use count, not a guess: 1 use is 0.35 for a strategy and the ceiling is 0.9.
4. Retrieval scores keyword match, confidence and recency for this workspace only.
5. The dashboard reads `/api/improve/report`; proposals and experiments are separate lists so
   a slow pipeline never blocks the report.

## Boundaries

- The running binary is never rewritten: an improvement is a git commit on a branch, reviewed
  and delivered through the pipeline, and release/build scripts are untouched.
- Learning is off until the user enables it; with learning off, scanning, experiments and
  approval are refused with a stable error key.
- Nothing here can add a permission rule, change a credential path or lower an approval gate.
