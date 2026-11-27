# Predictive engineering intelligence

Predictions are risk assessments with visible evidence, never guarantees.

## Risk score

`POST /api/improve/predict` with `{"goal":"…"}` returns:

| Component | Weight |
| --- | --- |
| Base | 0.10 |
| Recorded mission failures in the window | +0.15 each, capped at 0.45 |
| Failed verifier checks | +0.20 each, capped at 0.30 |
| No test files in the workspace | +0.15 |
| Uncommitted changes | +0.10 |

The total is capped at 0.95 and banded: `low` (< 0.35), `medium` (< 0.65), `high`. The
response always includes `label`: "heuristic risk score, not a calibrated probability", and an
`evidence` list naming each contributing count.

## Areas it can speak to

- Likely build failure: recorded check failures and their classified kinds.
- High-risk change: the band above, with the workspace's own history.
- Recurring bugs: normalised failure keys and their counts.
- Poor coverage: absence of test files, or a mission that recorded no check.
- Documentation drift: docs naming paths that do not exist.

## Areas it cannot speak to

Dependency conflicts and performance regressions are not predicted. The app has no dependency
resolver run and no benchmark harness in this module, so the report lists them as unavailable
rather than inventing probabilities.

## Using it

The dashboard shows the band, the score, the label and the evidence lines. A `high` band is a
reason to add a check or a test before approving, not a refusal.
