# Workflow optimisation

## What is analysed

From missions in the window (default 30 days):

- Mission and task counts, and the average number of tasks per mission.
- Retried tasks and their recovery.
- Repeated file reads: the same path read more than once inside one mission.
- Repeated commands: identical command lines run more than once.
- Verification coverage: missions whose report recorded a command or a verifier check.

Every finding is a count with the rows that produced it. There is no "estimated time saved"
figure, because the app has no counterfactual to measure.

## Recommendations

The report emits deterministic recommendations, each with a finding, a suggestion and the
evidence string:

- Repeated command → cache the result in the task goal instead of re-running it.
- Repeated file read → pass the excerpt between tasks rather than re-reading.
- High retry rate → tighten the task goal with the failure text.
- Missions without a check → add a verify stage before review.

Recommendations never propose removing a verification step. If a workflow experiment would
drop verification, the comparison is marked **inconclusive** and the reason says so.

## Benchmarks

Two kinds of baseline exist:

1. **Recorded baseline** — the mission rows in the window, which are the numbers the report
   shows. Any change is compared against them after the fact.
2. **Controlled experiment** — see [EXPERIMENTS.md](EXPERIMENTS.md): baseline arm and lean arm
   over the same goal, both read-only, compared from their own reports.

## Limitations

- Mission counts in a young database are small; the report states the window and the sample.
- Task decomposition is not optimised automatically; the pipeline plan and the mission plan
  remain the source of truth.
- Scheduling is unchanged: the existing queue and the writer-conflict rule still decide order.
