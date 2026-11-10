# Testing strategy

## What exists today (verified)

| Suite | Location | Run with | Size |
|---|---|---|---|
| Rust unit/integration | `app/src/*.rs` `#[test]` / `#[tokio::test]` | `cd app && cargo test --locked` | 540 passed (1 ignored) at the 2.45.0 run; includes MCP argument validation against the validator the route and agent loop share, workflow graph validation, dependency scheduling, schedule due-time anchoring, the resume boundary, and the extension sandbox (exit code, fuel stopping a spin loop, imports and a missing export refused, the memory cap, a granted workspace read, an ungranted import, a path escape, a saved deny, a write requiring an allow rule, `last_error`, its own data folder created lazily with a name escape refused, a pre-cancelled run stopped at its first host call, a zero wall budget refused, the run-history round trip, and the database schema layer — a fresh database recording its version with no copy, an existing database copied and verified at its old version before migrating, a newer-build schema refused byte for byte, and a failed upgrade put back with its version unadvanced), the update check (numeric version order, the platform's asset choice, an unsupported target, a release with no tag), the verified download (a release that is not newer, no asset for the platform, asset-name containment, a malformed digest, and a local HTTP server where a verified download stages and a hash mismatch leaves the previous copy and manifest intact), and the prepare step (nothing staged, a changed file, a real zip whose binary is found, an archive with no binary cleaned up and refused, a version used as a folder name refused, and a real `.tar.gz` built inside the test), and the swap (a replace that keeps the old bytes as a backup, a copy that lands wrong and is rolled back with the target untouched, a missing prepared binary that changes nothing, and a second swap that clears the previous backup), plus the startup cleanup (an older backup removed while a newer one, a foreign file and an unparseable name are kept, and the running binary is never removed even when it looks like a backup), and the rollback (the previous bytes restored with the displaced build kept at `.old-<version>`, a refusal with no record, no backup or another binary's record, a foreign name and a backup outside the install folder refused with the target untouched, and the record read back through the prepared view), and the static-UI layout (a macOS app bundle's `Contents/Resources/static` resolved before a `static/` beside the binary), and the uninstall (a portable folder previewed as removable with its data folder as the confirmation token, a source checkout refused with everything left in place, data and settings removed with either kept on request, and a missing tree reported rather than guessed at), and the run leases seen from the editor's side (a live run's claim found for its path and its checkout only, the command claim covering the whole folder, and both gone once the run is released) |
| Frontend helpers | `app/static/tests/` | `bash scripts/ci/test-frontend.sh` (Node `--test`) | 36 cases over the real `app.js`, loaded in a browser-free `node:vm` harness |
| VS Code extension logic | `vscode-extension/test/` | `cd vscode-extension && npm test` (Node `node --test`) | agent, lib, extension suites |
| Android JVM | `android/app/src/test/` | `cd android && gradle testDebugUnitTest` | policy/migration tests |
| Android instrumentation | `android/app/src/androidTest/` | emulator script `android/scripts/ci/run-android-instrumentation.sh` | launch, DB migration, paging, backup |
| Release-pipeline regressions | `scripts/ci/test-version.sh` | `bash scripts/ci/test-version.sh` | version parsing + artifact-name rules |
| Extension sandbox host surface | `app/src/extension_run.rs` tests + a live headless smoke | `cd app && cargo test extension_run` | WAT modules driven through the real interpreter: read/write with and without the grant, path escape, saved rule deny, I/O error codes |
| macOS bundle packager | `scripts/ci/bundle-macos-app.sh` | `bash scripts/ci/test-bundle.sh` | 13 checks: a complete stage builds the bundle, the executable is in `Contents/MacOS` and still executable, the whole UI is in `Contents/Resources/static`, `PkgInfo` is `APPL????`, `Info.plist` declares executable/identifier/type/name/versions and carries the staged version, the zip holds both paths with forward slashes, a stage with no static tree / no binary / an unusable version is refused, and a rebuild replaces rather than merges |
| Windows MSI packager | `app/packaging/windows/installer.wxs` + `scripts/ci/build-windows-msi.sh` | `bash scripts/ci/test-installer.sh` | 11 checks against a real WiX build: a complete stage builds the MSI and it is named for the version, `msiexec /a` performs an administrative install, the payload puts the binary and the UI beside it in one install folder, a nested file (`static/skills/debug/SKILL.md`) survives, the product name is in the package, three refusals (no static tree, no binary, unusable version), and a rebuild replaces the previous MSI. Prints `skip` where WiX or msiexec is absent |
| End-to-end (process level) | `scripts/ci/test-e2e.sh` | `bash scripts/ci/test-e2e.sh` | 32 checks: real binary boots headless against a scratch `FREE_AI_STUDIO_HOME`, mock provider answers a chat turn, the turn is stored, the process log and readiness payload are read, a workspace opens, an agent run parks at its write approval gate, the write is approved and lands on disk, the step log is `write, verify, done`, the process is killed and restarted, and both the conversation and the run with its steps are re-read from disk |
| Artifact gate | `scripts/ci/verify-artifacts.sh` | `bash scripts/ci/verify-artifacts.sh dist` | names, presence, checksums |

CI gates: `desktop-ci.yml` (clippy `-D warnings`, tests, layout checks, version
regressions, frontend tests, end-to-end smoke, ubuntu + macOS), `rust-host-api-ci.yml` (rustfmt on
host modules, tests, clippy), `android-ci.yml` (unit, lint, assemble, emulator
instrumentation, hard validation gate), `desktop-release.yml` (real artifacts).

The frontend layer is deliberately narrow. `app/static/app.js` is a classic
script with no exports, so `app/static/tests/harness.js` evaluates the real file
in a `node:vm` context whose DOM records calls and never draws; the tests then
call the top-level bindings directly. Only pure behaviour is asserted — escaping,
markdown, formatting, path keys — because a test cannot honestly verify a layout
it never rendered. Anything that needs a real element belongs in the browser
smoke layer below, not here.

## Gaps to close (ordered)

1. ~~**Frontend unit tests.**~~ Done in 2.5.0: `scripts/ci/test-frontend.sh`
   runs the real `app.js` in a browser-free harness(33 cases, wired into `desktop-ci.yml`). Still open: a browser smoke for render-critical paths, and
   the same treatment for `ide.js`; the workflow canvas is covered at helper level (layout, add/rename/delete, edges), not rendering.
2. **End-to-end.** Shipped in 2.6.0 for launch → chat → restart → verify
   persistence, and extended in 2.8.0 through a real agent run: a folder opens,
   the run stops at its write approval gate, the write is approved, the run is
   verified, and both the run and its steps are re-read after a restart
   (`scripts/ci/test-e2e.sh`, wired into `desktop-ci.yml`). Still open in the
   same flow: a provider failure case.
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
# frontend helpers (Node 18+)
bash scripts/ci/test-frontend.sh
# end-to-end (boots the real binary; needs curl + python3, ~3 s)
bash scripts/ci/test-e2e.sh
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
