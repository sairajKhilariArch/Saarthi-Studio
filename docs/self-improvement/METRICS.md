# Performance metrics

Every figure is an aggregate over rows the app wrote. The report states its window (30 days by
default) and lists what it could not measure in `unavailable`.

| Report field | Source |
| --- | --- |
| `quality.missions`, `missionsSucceeded` | Mission rows in the window |
| `quality.missionSuccessRate` | Succeeded over terminal missions; `null` when none finished |
| `quality.tasks`, `tasksDone` | Task rows and their `done` status |
| `quality.taskSuccessRate` | Done over done+failed |
| `quality.firstAttemptRate` | Done tasks with zero retries over done tasks |
| `quality.recoveryRate` | Retried tasks that ended done over retried tasks |
| `quality.reviewRejectionRate` | Failed `code_review` tasks over its tasks |
| `quality.tokens` | Sum of reported run tokens |
| `agents[]` | Per-role aggregates of the same rows, plus average duration and tool errors |
| `models[]` | Per-model runs, success, average tokens, average duration, estimated cost |
| `modelAdvice[]` | `rank_models` over models with at least two runs under the chosen priority |
| `workflow` | Task density, retries, repeated reads and commands, verification coverage |
| `failures` | Normalised error keys and counts |
| `maintenance` | Workspace counts, branch, dirty flag, declared dependencies, last check, open proposals, expired lessons |
| `experiences` | Total, validated and unvalidated lesson counts |
| `proposals` | Proposed and in-progress counts |
| `events` | Latest governance events |

## Honesty rules

- A rate with no denominator is `null` and the UI prints `not measured`.
- Tokens and durations the provider did not report are excluded, not zero-filled.
- Costs are estimates from published prices, labelled as such.
- Coverage is reported only when an lcov file exists.
- No metric is computed from a model's own claim about its work.

## Dashboard

The Improve view in the desktop app renders exactly this payload: quality tiles, agent and model
tables, model advice, workflow, failures, proposals, experiments, knowledge, risk, maintenance
and the governance trail. It re-renders from the API; it holds no independent numbers.
