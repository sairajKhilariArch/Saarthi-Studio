# Test Report

| Date | Command / check | Result | Notes |
|---|---|---|---|
| 2026-10-29 (prior session) | cargo test | 89 passed / 0 failed | includes new unified_diff, capture_checkpoint, checkpoint-persistence tests |
| 2026-10-29 (prior session) | node --check app/static/app.js | pass | |
| 2026-10-29 (prior session) | live API round-trip: run→pendingDiff→approve→diff in log→restore→disk reverted | pass | seq checkpoint visible, restored step logged |
| 2026-10-29 | git status -s | clean-root assertions during edits | all changes tracked by hand |

## Per-milestone entries are appended below as they complete.

| 2026-10-29 | cd app && cargo test | 94 passed / 0 failed | includes M2 privacy tests; step_once cancel re-check added |
| 2026-10-29 | cd vscode-extension && npm test (build + node --test) | 22 passed / 0 failed | M3 contract: approval/diff/restore flow, denial, cancel-mid-model-call |
| 2026-10-29 | node --check app/static/app.js | pass | |
| 2026-10-29 | cancel-mid-flight regression check | pass | model call parked in stub, cancel issued, reply released → 0 steps, no file written |
| 2026-10-29 | privacy parity check (TS vs Rust) | pass | SERVICE_API_KEY / bearer / aws / jwt / pem / short-value cases identical both sides |
| 2026-10-29 | cd app && cargo test host_api (M4a) | 16 passed / 0 failed | stream route strict shapes; unauth/malformed/oversize; host_not_configured | 
| 2026-10-29 | android gradle testDebugUnitTest (JAVA_HOME=jdk17) | BUILD SUCCESSFUL | streaming host chat + dual-format parse edits |
| 2026-10-29 | android gradle lintDebug | BUILD SUCCESSFUL | ConnectionTransportTest updated for supportsStreaming contract |
| 2026-10-29 | M4b: cd app && cargo test host_api | 18 passed / 0 failed | +agent_run_routes_sit_behind_the_host_bearer_wall: 401 unauth, list shape, masked invalid_request on 400, shared "No such run." 404s on detail/approve/cancel | 
| 2026-10-29 | M4b: cargo test agent | 13 passed / 0 failed | local /api/runs surface unchanged |
| 2026-10-29 | M4b: cargo check | clean | host router merge compiles |
| 2026-10-29 | M4b: android gradle testDebugUnitTest | BUILD SUCCESSFUL | Runs screen compiles; unit tests green |
| 2026-10-29 | M4b: android gradle lintDebug | BUILD SUCCESSFUL | no new lint failures |
| 2026-10-30 | M5: YAML parse of desktop-release.yml + desktop-ci.yml | pass | macos job = universal lipo build + both-slice check + headless HTTP smoke; new macOS CI job |
| 2026-10-30 | M5: executed on macOS runner | not run | no macOS hardware here; smoke executes only on `macos-latest` — documented limitation, not a pass |
| 2026-10-30 | M6: cd app && cargo test | 98 passed / 0 failed | +adaptive_tier_matrix test (routine→cheap, first-step/write/command/repair→strong, off/unconfigured→None); agent migration columns idempotent |
| 2026-10-30 | M6: node --check app/static/app.js | pass | settings UI wiring: load/save/round-trip fields, live tier-row toggle |
| 2026-10-30 | M6: npm run build (extension) | pass | contract additive (run JSON gains model/routerConfig) — no extension change needed |
| 2026-10-30 | M6: cd vscode-extension && npm test | 22 passed / 0 failed | |
| 2026-10-30 | M6: android gradle testDebugUnitTest | exit 0 | |
| 2026-10-30 | Live adaptive run through a real router | not run | no provider credentials in this environment; failover path covered by construction (step-logged) but not exercised live |
| 2026-10-30 | M7: cd app && cargo test | 113 passed / 0 failed | includes local-adapter + local-select/pull/hardware validation and no-remote-fallback tests |
| 2026-10-30 | node --check app/static/app.js | pass | after localBtn/runsBtn wiring fix + local view wiring |
| 2026-10-30 | cargo build (default gui features) | pass | headless (--no-default-features) build panics in tauri-build 2.7.4 "missing cargo:dev" — pre-existing toolchain issue, release CI unaffected |
| 2026-10-30 | Live UI: server on :34521, Local AI view opened in browser; click-through of localBtn | pass | view switches; status panel shows real "not detected" + endpoint error; setup card shows; honest model-list unavailability; hardware panel real (i3-1115G4/4c/15GB/GPU/disk) + quantized-model recommendation; token follow-up fix (dead CSS block replaced with styles matching actual markup); no browser console errors |
| 2026-10-30 | Theme check | pass | dark + light: panel/setup-card/hardware tokens resolve in both themes |
| 2026-10-30 | Live local inference round-trip | not run | no Ollama on this machine; honest failure path verified instead |

### v1.25.0 release pipeline repair

| 2026-10-30 | Release run 41007439650 job analysis | 3 of 6 jobs failed | deb (headless build panic), release (Linux deb step), vscode-extension (tsc); versions in tree said 1.22.0 under tag v1.25.0 |
| 2026-10-30 | headless: cargo build --release --locked --no-default-features (build.rs gated) | pass in 21m55s | "missing cargo:dev" panic gone |
| 2026-10-30 | cargo test --locked --no-default-features | 113 passed / 0 failed | includes the 1.25.0 version bump |
| 2026-10-30 | vscode: npx tsc --noEmit | 2 errors found → fixed → TSC CLEAN | agent.ts unifiedDiff arg; callRouter/ChatMessage[] through lib.ts |
| 2026-10-30 | vscode: npm ci && npm test | 22 passed / 0 failed | after fixes |
| 2026-10-30 | vscode: npx vsce package --no-dependencies | DONE (106.43 KB vsix) | CI's failing step reproduced only via tsc; packaging now works |
| 2026-10-30 | GUI deb: cargo deb --locked (local ubuntu-24.04) | built free-ai-studio_1.25.0-1_amd64.deb | Depends carries libc6 >= 2.39 → refuses Ubuntu 22.04 install |
| 2026-10-30 | deb static checks | skills present; gui-free depends (headless deb) | headless deb job remains the one .deb source |
| 2026-10-30 | desktop-release.yml: GUI deb removed from `release` job | YAML parses | removes glibc-2.39 conflict and the same-filename asset race with the deb job |
