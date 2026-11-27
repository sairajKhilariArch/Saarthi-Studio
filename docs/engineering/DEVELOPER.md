# Developer guide

## Adding a stage

`engineering_plan` in `app/src/engineering.rs` returns the task specs. Every stage needs an id,
a role from `multi_agent::ROLES`, a title and its dependencies. Writing roles must be declared
so the approval rule and the writer-conflict rule see them. Add a test that the plan still
starts with analysis and ends with review.

## Adding a check

`checks` currently supports Cargo. To add a toolchain:

1. detect its manifest in the same place as the profile scan;
2. run it through `run_command_check` (timeout, output cap, classification);
3. add the failure signals to `classify_failure` with their suggestion;
4. extend the unit test that asserts kinds.

Do not add a check that writes to the workspace; checks are read-only.

## Adding a metric

Add a `count(...)` query with the same 30-day window, put it in `measured`, and add a tile in
`renderEngineeringMetrics`. If the value cannot be measured, put its label in `unavailable`
instead of filling it with an estimate.

## Rules for this module

- Do not create a second orchestrator, run loop, git wrapper or permission check.
- Do not stage anything outside the mission report.
- Do not print a status, a test result or a speed number the code did not observe.
- Keep the scan bounded; a bigger limit needs a reason and a test.

## Verifying a change

From the repository root:

```
cargo fmt --manifest-path app/Cargo.toml
RUSTFLAGS="" cargo clippy --manifest-path app/Cargo.toml --locked --all-targets -- -D warnings
RUSTFLAGS="" cargo test --manifest-path app/Cargo.toml --locked
bash scripts/ci/test-frontend.sh
```

`RUSTFLAGS=""` is required on a machine without `mold`. The engineering tests bind a loopback
provider, so they need no network and no key.
