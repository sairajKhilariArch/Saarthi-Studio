# Self-improvement — developer guide

## Files

| File | Purpose |
| --- | --- |
| `app/src/improve.rs` | Settings, report, lessons, scan, proposals, experiments, risk, routes and tests |
| `app/src/engineering.rs` | `create_for_goal`, `pipeline_view`, `delivery_of`, `profile_view` (Phase 14) |
| `app/src/multi_agent.rs` | Calls `improve::observe_mission` when a mission reaches a terminal state |
| `app/src/git.rs` | `revert_checked` for rollback |
| `app/src/main.rs` | `mod improve`, `improve::initialize`, router merge behind `guard_local_origin` |
| `app/static/index.html`, `app.js`, `styles.css` | The Improve view, its loaders and table styles |
| `app/static/tests/improve.test.js` | Frontend wording and registration tests |

## Adding a lesson type

1. Extend `observations_from` with a new `Observation` only if it is justified by report data.
2. Give it a deterministic key; a free-text key defeats deduplication.
3. Set `validated` only for evidence you can point at; otherwise leave it false and it stays
   hidden by default.
4. Add a unit test that proves an unsupported case produces nothing.

## Adding a scan finding

Add a detector function returning `(key, title, problem, evidence, files, benefit, risk,
testing, priority)`, then add it to `scan`'s detector list. Keep it deterministic and read-only;
a detector must never write to the workspace. Add a test with a fixture workspace.

## Adding a metric

Read from existing tables; do not add a counter that duplicates a row. Use `Option` for any
rate and push a human-readable string into the report's `unavailable` list when the source is
missing. Add a unit test for the empty case.

## Checks

```bash
cargo fmt --manifest-path app/Cargo.toml
RUSTFLAGS="" cargo clippy --manifest-path app/Cargo.toml --locked --all-targets -- -D warnings
RUSTFLAGS="" cargo test --manifest-path app/Cargo.toml --locked improve
bash scripts/ci/test-frontend.sh
```

`RUSTFLAGS=""` is required on this machine because the mold linker is not installed.

## Testing with a scripted provider

The integration tests in `improve.rs` use the same scripted provider as the engineering tests.
For a manual check, run the app with a temporary home and a mock OpenAI-compatible server, then
call the endpoints directly:

```bash
FREE_AI_STUDIO_HOME=/tmp/improve-smoke ./target/debug/free-ai-studio --no-gui
curl --noproxy '*' -s localhost:8798/api/improve/report
```

Settings must be saved and a workspace opened before scanning; both endpoints are behind the
local-origin guard.

## Conventions

- User-facing strings are plain sentences; no confidence is presented as certainty.
- Refusals return stable error keys.
- Any new behaviour that could touch code goes through the engineering pipeline, never a direct
  write from this module.
