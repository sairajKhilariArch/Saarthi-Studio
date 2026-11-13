# Testing strategy

## What exists today (verified)

| Suite | Location | Run with | Size |
|---|---|---|---|
| Rust unit/integration | `app/src/*.rs` `#[test]` / `#[tokio::test]` | `cd app && cargo test --locked` | 634 test functions in the tree at 3.50.0 (623 passed, 1 ignored at the last full run, 3.46.0); includes MCP argument validation against the validator the route and agent loop share, workflow graph validation, dependency scheduling, schedule due-time anchoring, the resume boundary, and the extension sandbox (exit code, fuel stopping a spin loop, imports and a missing export refused, the memory cap, a granted workspace read, an ungranted import, a path escape, a saved deny, a write requiring an allow rule, `last_error`, its own data folder created lazily with a name escape refused, a pre-cancelled run stopped at its first host call, a zero wall budget refused, the run-history round trip, and the database schema layer — a fresh database recording its version with no copy, an existing database copied and verified at its old version before migrating, a newer-build schema refused byte for byte, and a failed upgrade put back with its version unadvanced), the update check (numeric version order, the platform's asset choice, an unsupported target, a release with no tag), the verified download (a release that is not newer, no asset for the platform, asset-name containment, a malformed digest, and a local HTTP server where a verified download stages and a hash mismatch leaves the previous copy and manifest intact), and the prepare step (nothing staged, a changed file, a real zip whose binary is found, an archive with no binary cleaned up and refused, a version used as a folder name refused, and a real `.tar.gz` built inside the test), and the swap (a replace that keeps the old bytes as a backup, a copy that lands wrong and is rolled back with the target untouched, a missing prepared binary that changes nothing, and a second swap that clears the previous backup), plus the startup cleanup (an older backup removed while a newer one, a foreign file and an unparseable name are kept, and the running binary is never removed even when it looks like a backup), and the rollback (the previous bytes restored with the displaced build kept at `.old-<version>`, a refusal with no record, no backup or another binary's record, a foreign name and a backup outside the install folder refused with the target untouched, and the record read back through the prepared view), and the static-UI layout (a macOS app bundle's `Contents/Resources/static` resolved before a `static/` beside the binary), and the uninstall (a portable folder previewed as removable with its data folder as the confirmation token, a source checkout refused with everything left in place, data and settings removed with either kept on request, and a missing tree reported rather than guessed at), and the run leases seen from the editor's side (a live run's claim found for its path and its checkout only, the command claim covering the whole folder, and both gone once the run is released), and the worktree path comparison (a symlinked alias, a trailing separator and a case difference all compare equal to the real folder, another repository's worktree is refused and left on disk, and a missing one is refused) |
| Language-server diagnostics | `app/src/lsp.rs` tests | `cd app && cargo test lsp` | 9 tests: extension→server mapping, path confinement, URI encoding (a space and a Windows drive letter), framed messages read in order with EOF ending the stream, 1-based lines with named severities, a long message capped rather than pasted whole, and a fake stdio server that publishes diagnostics for the open buffer, returns its own `initialize` error, and names a command that cannot start. **Compiled, not yet run green** (`KNOWN-ISSUES.md` #45) |
| Run admission across processes | `app/src/agent.rs` | `cd app && cargo test queue` | 5 tests: priority admission, admission at boot, cancelling a queued run, one-step reordering, and two connections on one database where the second connection cannot take a slot claimed through the first (limit 1), can once the limit is 2, and still cannot re-claim a run that is no longer queued |
| Frontend helpers | `app/static/tests/` | `bash scripts/ci/test-frontend.sh` (Node `--test`) | 44 cases over the real `app.js`, loaded in a browser-free `node:vm` harness |
| VS Code extension logic | `vscode-extension/test/` | `cd vscode-extension && npm test` (Node `node --test`) | agent, lib and extension suites, including the stdio MCP client against a fake server (3.48.0: save and list a server, health before and after, `test`, a tool call, a failing tool, and a mutation refused from a foreign origin). Two MCP cases were added at 3.48.0 and have not been run green yet (`KNOWN-ISSUES.md` #44) |
| Android JVM | `android/app/src/test/` | `cd android && gradle testDebugUnitTest` | policy/migration tests |
| Android instrumentation | `android/app/src/androidTest/` | emulator script `android/scripts/ci/run-android-instrumentation.sh` | launch, DB migration, paging, backup |
| Release-pipeline regressions | `scripts/ci/test-version.sh` | `bash scripts/ci/test-version.sh` | version parsing + artifact-name rules |
| Release asset gate | `scripts/ci/verify-release.sh` | `bash scripts/ci/test-release.sh` | 2 cases over a loopback fixture: a complete release passes, and a missing VSIX fails and is named |
| Release audit | `scripts/ci/audit-releases.sh` | `bash scripts/ci/test-release.sh` | 2 cases: a complete release at the minimum version passes, and a missing VSIX fails and is named (4 total with the gate) |
| Extension sandbox host surface | `app/src/extension_run.rs` tests + a live headless smoke | `cd app && cargo test extension_run` | WAT modules driven through the real interpreter: read/write with and without the grant, path escape, saved rule deny, I/O error codes |
| macOS bundle packager | `scripts/ci/bundle-macos-app.sh` | `bash scripts/ci/test-bundle.sh` | 13 checks: a complete stage builds the bundle, the executable is in `Contents/MacOS` and still executable, the whole UI is in `Contents/Resources/static`, `PkgInfo` is `APPL????`, `Info.plist` declares executable/identifier/type/name/versions and carries the staged version, the zip holds both paths with forward slashes, a stage with no static tree / no binary / an unusable version is refused, and a rebuild replaces rather than merges |
| Windows MSI packager | `app/packaging/windows/installer.wxs` + `scripts/ci/build-windows-msi.sh` | `bash scripts/ci/test-installer.sh` | 11 checks against a real WiX build: a complete stage builds the MSI and it is named for the version, `msiexec /a` performs an administrative install, the payload puts the binary and the UI beside it in one install folder, a nested file (`static/skills/debug/SKILL.md`) survives, the product name is in the package, three refusals (no static tree, no binary, unusable version), and a rebuild replaces the previous MSI. Prints `skip` where WiX or msiexec is absent |
| End-to-end (process level) | `scripts/ci/test-e2e.sh` | `bash scripts/ci/test-e2e.sh` | 32 checks: real binary boots headless against a scratch `FREE_AI_STUDIO_HOME`, mock provider answers a chat turn, the turn is stored, the process log and readiness payload are read, a workspace opens, an agent run parks at its write approval gate, the write is approved and lands on disk, the step log is `write, verify, done`, the process is killed and restarted, and both the conversation and the run with its steps are re-read from disk |
| Workspace change signal | `app/src/workspace.rs`, `app/static/app.js` | `cargo test workspace::tests::fingerprint` | the hash is stable across reads, ignores `target/`, moves on an edit and on a new file; the frontend seeds on the first poll and refreshes only when the hash moves |
| Brand assets | `scripts/brand/build-brand-assets.py`, `app/static/brand/` | `python scripts/brand/build-brand-assets.py` + Edge render | 7 SVGs from one face; rasters at 16/24/32/48/64/128/512 inspected |
| Branding audit | `docs/BRANDING-AUDIT.md` | `git grep -I -o -F <pattern>` | counts before/after per pattern, plus the retained-identifier table; 2.51.0 left 0 `Free AI Studio`, 0 `Saarthi Studio`, 0 `SaarthiStudio` |
| Artifact gate | `scripts/ci/verify-artifacts.sh` | `bash scripts/ci/verify-artifacts.sh dist` | names, presence, checksums |

CI gates: `desktop-ci.yml` (clippy `-D warnings`, tests, layout checks, version
regressions, frontend tests, end-to-end smoke, ubuntu + macOS), `rust-host-api-ci.yml` (rustfmt on
host modules, tests, clippy), `android-ci.yml` (unit, lint, assemble, emulator
instrumentation, hard validation gate), `desktop-release.yml` (real artifacts),
`release-audit.yml` (daily published-asset audit).

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
   the render smoke itself (21 cases, 2.33.0). Still open: `ide.js` has no harness,
   and the workflow canvas is covered at helper level (layout, add/rename/delete,
   edges), not rendering.
2. **End-to-end.** Shipped in 2.6.0 for launch → chat → restart → verify
   persistence, and extended in 2.8.0 through a real agent run: a folder opens,
   the run stops at its write approval gate, the write is approved, the run is
   verified, and both the run and its steps are re-read after a restart
   (`scripts/ci/test-e2e.sh`, wired into `desktop-ci.yml`). Still open in the
   same flow: none — the provider-failure cases are in (2.30.0/2.34.0, 32
   checks on Windows and 34 on POSIX).
3. ~~**Failure injection.**~~ Done in 2.30.0/2.34.0: an offline provider,
   401/429, a timeout, a locked DB, a corrupted config and a killed agent
   mid-step. Still open: a genuinely full disk — the end-to-end script says so at
   line 637 and does not inject ENOSPC or a full volume.
4. ~~**Installation smokes in CI.**~~ Done in 2.31.0. After packaging: `dpkg -i` in a container,
   run the AppImage, extract the zip and launch, `code --install-extension`
   for the VSIX, `adb install` for the APK.
5. ~~**Platform matrix.**~~ Done in 2.32.0: a Windows CI job builds and runs the
   Rust suite (the `desktop-release.yml` steps remain).

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

## Browser checks (manual)

The frontend layer above runs without a document, so layout-dependent behaviour is
not covered by it. 2.49.0 added the first recorded browser check: a headless build
on a scratch `FREE_AI_STUDIO_HOME`, Chromium at 1440×900, and pointer events
dispatched through the app's own listeners. It drove the workflow canvas — drag,
live edge redraw, clamping to the canvas width, and a saved position restored after
reload — and found a real defect in the drag handler that the unit layer could not
see. It is manual and repeatable by hand, not automated; the checks a browser run
cannot replace (a real OS pointer, touch, and cross-platform rendering) remain
listed in `KNOWN-ISSUES`.
