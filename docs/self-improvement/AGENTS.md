# Agent evaluation framework

Agent performance is measured from missions and task rows this app already wrote. There is no
sampling, no extrapolation and no synthetic baseline.

## Metrics

| Metric | Definition |
| --- | --- |
| Tasks | Task rows for the role in the window |
| Success rate | `done / (done + failed)` |
| First attempt | Done tasks that needed no retry, over done tasks |
| Retry rate | Tasks that needed a retry, over all tasks |
| Recovery rate | Retries that ended `done`, over retried tasks |
| Review rejections | `code_review` tasks that failed, over its tasks |
| Average duration | Seconds from run start to finish, when both exist |
| Tokens | Sum of reported `tokens` on the role's runs |
| Tool errors | Failed step-log entries attributed to the role |

Windows default to 30 days and are stated in the report, so a number always has a period.

## Underperformance

The report lists roles whose success rate is below 60% with at least three tasks, plus roles
with repeated identical failures (normalised error keys). It says which rows support the
finding; it does not infer a cause it cannot see.

## Recommendations, not edits

Instruction suggestions are text derived from the recorded failures (for example "the coding
role failed three times on a missing import: add a step that reads the module's imports before
editing"). They are advisory. Nothing rewrites an agent profile automatically, and per-role
model assignment stays a user decision in the agent profile editor.

## Permissions

No code path in this module can add a permission rule, widen a tool allowlist, change a profile
or lower an approval level. Proposals that concern profiles are delivered as reviewable diffs
through the same engineering pipeline as any other change.

## Reading the numbers

- A rate with a zero denominator prints `not measured`, never `0%`.
- Durations and tokens that a provider did not report are excluded and counted separately.
- One run is a sample, not a trend. Advice on models requires at least two runs per model.
