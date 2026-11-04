# Testing strategy

## What exists today (verified)

| Suite | Location | Run with | Size |
|---|---|---|---|
| Rust unit/integration | `app/src/*.rs` `#[test]` / `#[tokio::test]` | `cd app && cargo test --locked` | 133 test functions; 127 passed at last full run |
| VS Code extension logic | `vscode-extension/test/` | `cd vscode-extension && npm test` (Node `node --test`) | agent, lib, extension suites |
| Android JVM | `android/app/src/test/` | `cd android && gradle testDebugUnitTest` | policy/migration tests |
| Android instrumentation | `android/app/src/androidTest/` | emulator script `android/scripts/ci/run-android-instrumentation.sh` | launch, DB migration, paging, backup |
| Release-pipeline regressions | `scripts/ci/test-version.sh` | `bash scripts/ci/test-version.sh` | version parsing + artifact-name rules |
| Artifact gate | `scripts/ci/verify-artifacts.sh` | `bash scripts/ci/verify-artifacts.sh dist` | names, presence, checksums |

CI gates: `desktop-ci.yml` (clippy `-D warnings`, tests, layout checks, version
regressions, ubuntu + macOS), `rust-host-api-ci.yml` (rustfmt on host modules,
tests, clippy), `android-ci.yml` (unit, lint, assemble, emulator instrumentation,
hard validation gate), `desktop-release.yml` (real artifacts).

## Gaps to close (ordered)

1. **Frontend tests.** `app/static/` has no runner. Add a node-based DOM-free
   unit layer for pure helpers (packing, formatting, state reducers) and a
   browser smoke for render-critical paths.
2. **End-to-end.** Launch the binary headless on a scratch port, drive the API
   for a full flow (open folder → chat via mock router → agent run with
   approval → restart → verify persistence). `scripts/mock-router.py` already
   exists for the provider side.
3. **Failure injection.** Offline provider, 401/429, timeout, disk full,
   locked DB, corrupted config, killed agent mid-step.
4. **Installation smokes in CI.** After packaging: `dpkg -i` in a container,
   run the AppImage, extract the zip and launch, `code --install-extension`
   for the VSIX, `adb install` for the APK.
5. **Platform matrix.** Windows tests exist only through `desktop-release.yml`
   build steps; add a Windows CI job that runs the Rust suite.

## Conventions

- Tests never touch the user's real config or DB: use `HOME`/`USERPROFILE`
  overrides or temp dirs (`FREE_AI_STUDIO_SKILLS_DIR` already exists for skills).
- Tests that need a network are marked and skipped by default.
- A regression test is added for every fixed defect; the test must fail against
  the old behaviour (state the old behaviour in a comment).
- Pipeline logic is tested with POSIX shell tests, since it runs on three OSes.

## Local verification recipe

```bash
# backend
cd app && cargo build --release --locked && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings
# pipeline logic
bash scripts/ci/test-version.sh
# packaging gate
bash scripts/ci/verify-artifacts.sh dist
# extension
cd vscode-extension && npm ci && npx tsc --noEmit && npm test
# android (JVM only, no emulator)
cd android && gradle --no-daemon testDebugUnitTest
```

## Evidence policy

Record the command, exit status and the artifact/run id in the report. Pending
or skipped checks are listed as gaps, never as passing.
