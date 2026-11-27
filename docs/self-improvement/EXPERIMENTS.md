# Experimentation framework

## Design

An experiment compares two ways of doing the same read-only job over one goal:

| Arm | Plan |
| --- | --- |
| Baseline | architect → testing → code review |
| Lean | testing → code review |

Both arms are ordinary missions under the Phase 13 supervisor, so dependencies, retries, budgets
and reports behave exactly as they do elsewhere. Neither arm may write: the experiment refuses to
start if the workspace is not usable, and a read-only plan has no writer role.

## Creating one

`POST /api/improve/experiments` with `{"goal":"…"}`. Requires learning enabled and a token budget
if one is configured. The response records both mission ids and the arm plans, so the same
experiment can be re-run against the same goal for a second sample.

## Comparing

`POST /api/improve/experiments/:id/complete` waits for both missions to be terminal, then compares
their own reports:

- Success first: if an arm failed, there is no performance claim. If both failed, verdict
  `neither`. If one failed, the other arm is reported as the only one that finished, with that
  caveat.
- Only then are tokens, elapsed seconds and task counts compared. A smaller number is not a win
  if verification disappeared: when the lean arm recorded no check and the baseline did, the
  verdict is `inconclusive` and the reason says verification was dropped.
- Verdicts: `variant`, `baseline`, `tie`, `inconclusive`, `neither`.

Every comparison carries `sampleSize: 1` and the caveat text. A single mission pair is a
measurement, not a statistical result; the dashboard prints it that way.

## History and reproducibility

Experiments are rows, not files: goal, arm mission ids, status, timestamps and the frozen
comparison. Re-running the same goal creates a new row, so an earlier result is never overwritten.
The missions themselves keep their reports, so the comparison can be audited later.

## Deterministic providers

The integration tests use the scripted provider, which returns fixed tool calls, so a test asserts
the comparison logic rather than a model's mood. A manual experiment against a real provider is
still one sample and is labelled as such.

## What is not supported

- More than two arms.
- Automatic repetition for statistical power.
- Prompt or model sweeps (the profile editor changes one thing at a time by hand).
- Experiments that modify code: those are proposals, not experiments.
