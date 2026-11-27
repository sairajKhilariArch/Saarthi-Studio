# Model selection strategy

The app does not silently switch models. It measures what each model actually did and ranks
that evidence under a priority the user chooses.

## Measured per model

| Field | Source |
| --- | --- |
| Runs | Run rows grouped by the model recorded on the run |
| Success rate | Runs whose task ended `done` |
| Average tokens | Reported `tokens`, averaged over runs that reported them |
| Average duration | Run start to finish |
| Estimated cost per run | `openai::published_price(model)` × average total tokens |

Cost is an estimate, not a bill: the published price is per direction and the recorded total
does not split input from output, so the report labels it `output split unknown`. A model with
no published price prints `price unavailable` and is listed as unavailable instead of guessed.

## Priorities

`priority` in the improve settings is one of:

- **balanced** (default) — success rate first, then speed and estimated cost.
- **reliability** — success rate only.
- **speed** — shorter measured duration first.
- **cost** — lower published price × tokens first; unpriced models score neutrally.

`rank_models` filters to models with at least two runs, scores them, and returns the top five
with a `reason`. It only orders rows; it never pins a model, edits a provider, spends tokens or
changes a profile.

## Task requirements

Context window and capability matching are not automated. The report states which requirements
it cannot evaluate (context limits, tool support, provider reliability outside recorded runs)
in `unavailable`, and the agent profile editor remains how a user assigns a model to a role.
Provider failures appear as failed runs in the same table, so fallback decisions can be based
on evidence rather than assumption.

## Local and remote

Both count the same way when they appear on a run. A local model with no price is not penalised
in the reliability ranking and is neutral in cost ranking.

## Controlled fallback

Not implemented as an automatic policy in this phase. A fallback would need a recorded failure
rate per provider and an explicit user choice; claiming it without those would be fabricated
reliability. Recorded failures are the input for that work (see remaining work).
