# Tests and project checks

## Checks

`POST /api/engineering/checks` runs the project's own check command — `cargo check --quiet`
when `Cargo.toml` exists — with a 180 s timeout, its output capped at 8 000 characters, behind
`Action::Command`. The result records the command, exit status, elapsed milliseconds, the
classified failure kind and its suggestion.

JavaScript is deliberately skipped rather than guessed: `package.json` scripts are arbitrary
commands, so the response says so in `skipped` and the project's own tooling stays in charge.

The UI turns a failed check into a repair-pipeline offer. Nothing is auto-repaired by the check
endpoint itself.

## Test generation and regression detection

Test writing is a role, not a generator: the `testing` agent runs with its profile's tools and
the project's own commands. A pipeline's acceptance criterion for tests is satisfied only when
the mission report contains a command or a verifier approval — otherwise the trace says
`not-verified`.

The engineering module does not invent coverage numbers. Coverage is read from an existing
`lcov.info` (`coverage/`, `target/coverage/` or the workspace root); without one, coverage is
`null` and listed as unavailable.

## Tests for this phase

- Unit (in `engineering.rs`): bounded scan, ignored trees, requirement gaps, acceptance,
  autonomy mapping, plan shape, failure classification, written-path extraction, commit
  message, lcov parsing.
- Integration (in `flow`): a vague request becomes questions without a model call; the answer
  starts a supervised mission; level 1 leaves every stage planned and refuses git without
  approval; a level-3 pipeline runs all six stages against a scripted provider and never labels
  a criterion verified when nothing was written; delivery refuses a pipeline with no files;
  the emergency stop leaves live pipelines cancelled.
