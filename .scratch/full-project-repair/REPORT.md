# Full-project repair — 2026-11-20

Branch: `repair/full-project-stabilization` from `main` @ `d1cfca6` (3.73.0).
Commits: `c256ae9` (3.73.1), `59a2f6d` (3.73.2).
User's uncommitted WIP (`app/src/dap.rs`, `app/static/ide.js` — DAP attach
feature) preserved untouched in the working tree; NOT committed, NOT modified.

## Workspace shape

Single crate `free-ai-studio` in `app/` (no root Cargo.toml, no other Cargo
members). 53 Rust modules, ~65k LOC. Toolchain: nightly (rust-toolchain.toml),
`.cargo/config.toml` adds `-Z threads=8` and `-fuse-ld=mold` on Linux.

## Environment blockers found (and how they were handled)

- `mold` linker NOT installed → every build script failed to link
  (`collect2: cannot find 'ld'`). Handled with `RUSTFLAGS="-Z threads=8"`
  env override (no file changes); rustc's bundled lld links instead.
- `webkit2gtk-4.1` / `javascriptcoregtk-4.1` NOT installed → default `gui`
  feature cannot compile here. All Rust verification ran
  `--no-default-features` (the documented headless path, same as the deb CI
  job and the machine's existing binary).
- `target/debug/.fingerprint` and `deps/` were missing → full rebuild needed.
- rust-analyzer proxy present but component missing → added via
  `rustup component add rust-analyzer` (nightly). Default toolchain is
  stable; the live-test workspace pins nightly via rust-toolchain.toml.
- No Android SDK, no gradle, no java → Android not built/tested.
- No gh CLI; GitHub API reachable unauthenticated (job metadata yes, raw
  logs 403).

## Commands executed — actual results

| Command (from app/ unless noted) | Result |
|---|---|
| `cargo check --workspace` (default gui) | FAILED: `The system library javascriptcoregtk-4.1 required by crate javascriptcore-rs-sys was not found` (pkg-config). Actual, expected on this machine. |
| `cargo check --workspace --no-default-features` | PASS |
| `cargo test --workspace --no-default-features` | **664 passed, 0 failed, 1 ignored** (ignored = installer network test, deliberate). Baseline before fixes: 660/0/1. |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --no-default-features -- -D warnings` | PASS (`--all-features` blocked by webkit, same as check) |
| `cargo build --no-default-features` (debug) | PASS |
| `cargo build --release --no-default-features` | see PROGRESS tail below |
| vscode-extension: `npm ci && npm test && npm run build` | **45/45 tests pass**, esbuild bundle OK (dist/extension.js 214 kb) |
| website: `npm ci && npm run check && npm run build` | PASS (astro check + 2 pages) |
| website: `npm run check:build` plain | FAILED by design: localhost URLs (Astro default site when SITE_URL unset) |
| website: `SITE_URL=https://sairajkhilariarch.github.io SITE_BASE=/Saarthi-Studio/ npm run build` + `SITE_BASE=/Saarthi-Studio/ npm run check:build` | PASS — identical to CI env; documented in website/README.md. Not a bug. |
| Live smoke (isolated `FREE_AI_STUDIO_HOME`, port 8799) | server boots; `/api/health` (version correct), `/api/capabilities`, `/api/config` (no secrets, `has_api_key:false`), `/api/lsp/servers`, `/api/debug/state`, `/` + `/static/app.js` (398 kb) all OK; workspace open OK |
| Live LSP vs real rust-analyzer 1.102 nightly | broken buffer → E0425 returned (2.1 s); clean buffer → published:true, [] (5 s window). See B2. |
| DAP probes vs real debugpy 1.8.22 (scratch venv) | see B3 |

## Bugs found

### B1 — HIGH — FIXED & VERIFIED (commit c256ae9, 3.73.1)
`app/src/lsp.rs` — `Session::request()` and the diagnostics wait in `run()`
re-queued every non-matching message into the buffer they read next
(`buffered.push_back(message)` then `next_message` pops it again). One
notification before the initialize answer, or one publish for another file,
spun the loop forever: the real answer waited unread in the channel and the
deadline was never consulted (it is only checked when the buffer is empty).
Real servers (rust-analyzer etc.) always send such messages, so diagnostics
against them could hang a tokio worker indefinitely at 100% CPU. The fake
server never sent any, which is why 660 tests stayed green.
Fix: each wait examines every message exactly once (queue + aside deques),
returns non-matches to the buffer in arrival order for later phases.
Regression tests: `a_notification_before_the_initialize_answer_does_not_stall`,
`a_publish_for_another_file_does_not_stall_the_diagnostics_wait` — both hang
>60 s without the fix, pass in ms with it. Class scan: dap.rs/mcp.rs use
channel-only waits, no same pattern elsewhere.

### B2 — HIGH — FIXED & VERIFIED (commit 59a2f6d, 3.73.2)
`app/src/lsp.rs` — the diagnostics wait broke on the FIRST
`publishDiagnostics` for the open buffer. Observed live (probe_lsp.py):
rust-analyzer publishes `count=0` at t+0.63 s, then the real E0425 at
t+2.18 s. The app therefore reported "no problems" for a file that does not
compile — silently wrong output from the Problems pane.
Fix: an empty publish is kept as the answer-so-far and the wait continues
until a non-empty publish or the 5 s deadline; a genuinely clean file still
reports published:true with []. Live re-verified: broken → E0425 line 2 col
13; clean → []. Regression tests: `an_empty_first_publish_is_not_the_final_answer`
(RED: 0 != 2 before fix), `an_empty_publish_that_stays_empty_is_still_reported`.
Trade-off: clean files now use the full 5 s window (correctness over latency).

### B3 — HIGH — in the user's UNCOMMITTED WIP; diagnosed, NOT modified
DAP attach (`dap.rs` working-tree changes + `ide.js`) cannot work with any
real adapter as written; the new fake-adapter test only confirms the app's
own message shape. Verified against real debugpy 1.8.22 and adapter sources:
- debugpy: the app sends top-level `{host, port}` (and the UI hint says
  `debugpy --listen 5678`). The stdio adapter then starts its OWN listener
  and waits for the debuggee to connect IN; a `--listen` debuggee waits for
  the adapter to connect OUT → deadlock, no attach response within the 15 s
  LAUNCH_TIMEOUT (reproduced live; `connect:{host,port}` shape deadlocks
  identically — outbound connect only exists in `--for-server` debugServer
  mode, which requires `--port`, i.e. TCP not stdio).
  **Verified working shape**: attach `{"listen":{"host","port"}}` + target
  runs `python -m debugpy --connect <host:port>` → attach succeeds,
  breakpoint hit (STOPPED reason=breakpoint). Fix path: send `listen` for
  debugpy and change the UI hint to `--connect`, or implement TCP
  debugServer mode for the `--listen` topology.
- dlv: `service/dap/server.go:2334+` — attach defaults to mode "local"
  (requires `processId`/`waitFor`; app sends neither → "The 'processId' or
  'waitFor' attribute is missing"). Mode "remote" requires an ALREADY
  active debug session on that same dlv instance ("no debugger found" on a
  freshly spawned `dlv dap`). Attaching to an external `dlv dap --listen`
  server needs a TCP DAP client, not a new stdio adapter.
- lldb-dap: attach takes `pid`, `attachCommands`, or
  `gdb-remote-port`/`gdb-remote-host` (21.0+); top-level host/port are
  ignored → refusal. Needs per-adapter argument mapping.

### B4 — MEDIUM — in the WIP (committed stop path interacts with it)
`dap.rs:930` `stop_session` sends `disconnect {terminateDebuggee: true}`
unconditionally. Correct for launch sessions; for an attach session it
KILLS the user's independently running program. Attach must disconnect
with terminateDebuggee false/omitted.

### B5 — MEDIUM — documented limitation, proposed improvement
Unix orphan grandchildren: `agent.rs run_command` relies on
`kill_on_drop(true)` (kills only the `sh` child); `runner.rs kill_process`
kills the tree on Windows (`taskkill /T /F`) but only the direct child on
Unix (explicitly documented at runner.rs:345-350). A timed-out/cancelled
`sh -c 'server & wait'` leaves the server running. Proposal:
`Command::process_group(0)` + killpg on timeout/cancel (Unix), keep
taskkill on Windows; needs a drop-guard to cover cancellation and a
regression test asserting the grandchild dies. Not implemented this round:
cross-platform hot-path change, Windows unverifiable here.

### B6 — LOW / environmental — Windows CI LSP flake (run #220, a4ae459b)
`lsp::tests::a_fake_server_publishes_diagnostics_for_the_open_buffer` +
`an_initialize_error_keeps_the_servers_own_words` failed with "initialize
did not answer in time" (643 pass / 2 fail / 1 ignored per the mission
brief). lsp.rs was NOT changed between that run and the next green run
(#221+) — intervening commits are extension/MCP-only. Symptom = the 10 s
initialize deadline expiring, consistent with slow node.exe cold start on
Windows runners (Defender scan). Determination: not fixed by code, remains
potentially flaky on cold runners; distinct from B1 (hang, not timeout).
Not blindly changed. Optional mitigation: node warmup in the test launcher.

### B7 — INFO — macOS worktree history
The historical `/var/folders` vs `/private/var/folders` worktree failure is
already fixed (2.46.0 `same_worktree_path`) with a dedicated regression
test `an_alias_of_a_worktree_path_compares_equal`. The Nov-13 macOS+Linux
failures (runs #207/#208, 3.50.0/3.50.1) could not be attributed to specific
tests: raw CI logs need authentication (403 unauthenticated). Current suite
passes locally; git/filelock tests included.

### B8 — HIGH — FIXED & VERIFIED (3.74.2)
`app/static/ide.css` — the `@media (max-width: 1100px)` block set
`.app > .ai-dock { display: none; }` unconditionally. The whole conversation
(chatView, messages, composer) lives in that dock, so in any window ≤1100px
wide (tiling setups, split screens) threads could be created and listed but
never opened, and no message could be sent; `revealChatDock()` could not
help because the media query's display:none outranked the panel state.
Reproduced live in a browser at 780px and 900px; the user's last session
(v3.54.0, which already had the IDE shell) shows two created threads with
zero messages — consistent with this.
Fix: the dock becomes a fixed overlay at narrow widths, driven by the same
right-panel state — opening/creating a thread reveals it, the dock close
button and panel toggle dismiss it (`body.no-right`). Regression test:
`app/static/tests/ide-layout.test.js` asserts against the real stylesheet
that the narrow layout keeps the dock reachable and only hides it through
the user's toggle. Verified live at 900px: open thread → composer visible,
close → hidden, thread click → revealed, send → message persisted and the
router path answered (honest no-key error in the keyless scratch config);
desktop 1600px layout unchanged. Frontend suite 100/100.

## Release-pipeline round (2026-11-20, after the first push)

The v3.74.1/v3.74.2 tag pushes exposed four more gate bugs; all fixed and
verified, and **v3.74.4 published complete with all 11 assets** (release
run #81, success):

- B9: `smoke-apk.sh` demanded React Native parts (JS bundle, .so libs) and
  v1-only signatures from a native Java app signed v2-only at minSdk 26;
  `android-ci.yml` tested pre-rename `com/sarthi/studio` paths. Re-aimed at
  the real install contract (3.74.3). Verified against locally built real
  artifacts: APK 8/8, AAB 7/7, 34/34 Android unit tests; Android CI #64 green.
- B10: the deb gate read `steps.meta.outputs.version` in a job with no
  `id: meta` step → demanded a versionless filename. Now reads
  `needs.preflight.outputs.version` (3.74.4).
- B11: the Windows gate ran bash backslash continuations under the job's
  pwsh default shell without `shell: bash` → a literal `\` became an
  expected artifact name. Fixed + contract guard (3.74.4).
- B12: `test-release.sh` APK fixtures still encoded the RN contract, so the
  Linux CI "release asset gate" step failed after the smoke fix. Fixtures
  now carry the launch-activity descriptor (namespace read from the real
  Gradle file), a v2 signing-block fixture, and a missing-launch failure
  case (3.74.5). 18/18.

Guards added to `verify-release-contract.sh`: jobs reading
`steps.meta.outputs` must define the step; bash continuations under pwsh
defaults need `shell: bash`. `test-version.sh` 26/26.

## Full-project sweep (2026-11-20, at 3.74.5)

Every tracked file class checked: 33 shell scripts `bash -n` clean; all
static JS `node --check` clean; 2 Python files compile; Rust fmt + 664
tests + clippy `-D warnings` clean (headless); frontend 100/100; extension
tsc + 45/45 + build; website astro check + build + check:build (CI env);
CI scripts: test-version 26, test-release 18, test-artifacts 12,
test-bundle 13, test-linux-metadata 9, test-e2e 34 (real binary, mock
provider, kill/restart, corrupt-DB recovery) — all 0 failed.
test-installer skips locally (no WiX/dotnet); the MSI itself built and
smoked on Windows CI in release run #81. gui-feature builds remain blocked
by missing webkit2gtk on this machine (CI covers them: Windows/macOS green).

## Security review (scanned areas)

- SQL: all queries parameterized; the only `format!` SQL interpolates the
  constant TERMINAL status list (filelock.rs:93,105). No injection surface.
- Windows folder picker: fixed PowerShell script, nothing interpolated
  (workspace.rs:1523+); macOS osascript likewise fixed.
- Agent commands: permission-gated (PermissionGate, profile authority),
  90 s timeout, kill_on_drop; shell use is the intended feature.
- Mutating routes behind LocalOriginGuard; Host API opt-in TLS + bearer +
  rate limiting (host_security); config API exposes `has_api_key` booleans,
  not secrets; tests run against the mock keychain (.cargo/config.toml).
- LSP/DAP: paths confinement-checked, adapter argv fixed per language,
  attach (WIP) restricted to loopback hosts. No new findings.

## Not verified here (environment limits)

- gui-feature compile/test (needs webkit2gtk installed).
- Android build/test (no SDK/gradle/java).
- Windows/macOS runtime behavior (CI history only).
- Full browser click-through of the UI (serving + API contract verified;
  no interactive session run this round).
- install.sh / update swap-rollback live execution (destructive to a real
  install; unit-tested in suite).
- Raw CI log inspection (needs GitHub auth).

## Prioritized next steps (awaiting approval)

1. P1: rework the WIP DAP attach per B3/B4 (per-adapter schemas, debugpy
   `listen` shape + `--connect` hint, terminateDebuggee:false on attach,
   real-adapter tests). User's uncommitted feature — needs their go-ahead.
2. P2: Unix process-group kill for agent commands + runner stop (B5).
3. P3: decide Windows LSP cold-start mitigation with CI log access (B6).
4. P4: install webkit2gtk to unlock local gui builds (system change).
5. P5: authenticated review of Nov-13 macOS+Linux CI failures (B7).

## Release build

`cargo build --release --no-default-features`: **PASS** — "Finished
`release` profile [optimized] target(s) in 2m 10s" (LTO + codegen-units=1,
bundled lld via the RUSTFLAGS override).
