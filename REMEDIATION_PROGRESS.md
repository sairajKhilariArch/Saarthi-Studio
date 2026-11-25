# Sarthi Studio — Remediation Progress

**Date:** 2026-11-25  
**Branch:** `remediation/security-2026-11-24`  
**Base:** `9721a53` (audit reports on `audit/full-application-2026-11-24`)  
**Release under remediation:** `3.88.7`
**Status:** Phases 3–5 complete on the tested Linux surface. Phase 5 measured and optimized the 200k-message history path, streaming UI and concurrent SQLite writes, then ran fault, soak and full regression suites. The prior code fixes are retained; the remaining verification gaps for real SSE, stop-generation, settings persistence and the startup CLI are now closed with real tests. External signing, Windows/macOS/Android execution and a system-wide package install remain blocked by this host.

## Working environment

| Item | Value |
|---|---|
| Repository | `/home/goku/Documents/programing/projectss/aham/llm/Saarthi-Studio` |
| Release branch | `main` at `55d1218`; **not modified** |
| Audit branch | `audit/full-application-2026-11-24` at `9721a53` |
| Remediation branch | `remediation/security-2026-11-24`, created from `9721a53` |
| Rollback | revert the single remediation commit; no schema migration or user-data rewrite is part of this work |
| User changes overwritten | none; changes are additive and limited to the files listed below |
| Push/deploy | not performed |

The Rust suites and the real-binary e2e use scratch config/data directories. The one exception is noted in `FINAL_TEST_RESULTS.md`: an accidental command-line launch of the debug binary used the real home for a few seconds because this binary has no `--version` flag. No test or verification step was pointed at real user data.

## Consolidated backlog

The six audit reports were read as claims and checked against source before implementation. Duplicate findings were merged; two report claims were found to be stale.

| ID | Priority | Finding | Source | Status |
|---|---|---|---|---|
| SEC-01 | P1 / High | Core local routes were outside the loopback `Host`/`Origin` guard | Security audit | **Fixed in 3.88.1**; retained |
| SEC-02 | P2 / Medium | Recent-folder path was interpolated into `innerHTML` | Security audit | **Fixed in 3.88.1**; retained |
| BUG-03 | P3 | Streaming metadata hid `memory_used`/`retrieved` behind skill auto-match | Bug log | **Fixed in 3.88.1**; retained |
| SEC-03 | P2 / Medium | Remote MCP accepted plain `http://`, exposing headers/OAuth tokens | Security audit | **Fixed in 3.88.2** |
| SEC-05 | P2 / Medium | Saved MCP header values lived in `mcp.json` | Security audit | **Fixed in 3.88.2** (desktop); VS Code server remains 0600 |
| SEC-07 | P2 / Verification | No Rust advisory scan had been run | Security audit | **Fixed**: `cargo-audit` installed and scan run |
| DEP-01 | P1 / Supply chain | Locked Rust dependencies had known vulnerabilities, including a critical `zip` advisory | Remediation scan | **Fixed**: lock upgraded; audit now reports 0 vulnerabilities |
| SEC-04 | P2 / External | Windows/macOS packages are unsigned; checksums share the release origin | Security audit | **Blocked**: needs certificates, notarization and platform hosts |
| SEC-06 | P3 / Dev-only | 3 moderate npm advisories via the VS Code packaging tool | Security audit | **Accepted**: production dependencies 0; attempted upgrade reverted |
| SEC-08 | Process | Windows/macOS/Android runtime and signed installers unobservable here | Security audit | **Blocked**: no hosts/SDKs |
| P3 | P3 | No browser layout/pixel/accessibility regression suite | Application audit | Open; documented |
| P3 | P3 | Per-message edit/delete/regenerate absent | Application audit | Open by product scope; not a regression |
| P4 | P4 | Tauri/GTK `glib` 0.18.5 unsoundness | `cargo audit` | Open; no safe in-line upgrade |
| VER-01 | P2 / Verification | Real SSE streaming and stop-generation were asserted only by construction; the mock answered one JSON body | Phase 3 review | **Fixed in 3.88.3**: real 6-delta event stream, timed abort keeps one partial reply |
| VER-02 | P3 / Verification | Settings persistence across restart was not driven through the config route | Phase 3 review | **Fixed in 3.88.3**: save, restart, read back from disk |
| CLI-01 | P4 / Usability | `--version`/`--help` were ignored, so a probe booted the app against the real home | Prior verification note | **Fixed in 3.88.3**: built-binary tests and an e2e check |
| VER-03 | P3 / Release | No Linux package payload had been assembled or booted | Phase 3 review | **Partially fixed**: `.deb` payload boots 14/14; system install, signing and `$auto` dependency metadata blocked here |

### Stale report claims corrected

1. **Desktop MCP OAuth tokens were already in the OS keychain.** `mcp_oauth.rs` reads/writes `crate::keychain`; the old SEC-05 text said OAuth tokens were a 0600 file. The real gap was only **saved custom headers** in `mcp.json`.
2. **The VS Code MCP client already refuses TLS downgrade off loopback.** The audit implied a shared gap. The VS Code URL policy is already correct; only the desktop Rust path needed the rule. The VS Code server's OAuth session file is still 0600 because its standalone Node server has no VS Code SecretStorage bridge.

### Dependencies between items

- SEC-03 must be enforced before OAuth discovery/registration/token/authorize URLs are used; a save-only check would leave old `mcp.json` files and OAuth metadata free to downgrade.
- SEC-05 depends on the existing keychain location helper (`crate::keychain_location`), so test fixtures stay isolated.
- DEP-01 had to land before the final test run because `zip` extraction is exercised by the real update/install tests.
- SEC-01's global router guard is retained and remains release-blocking in e2e.

## Implementation sequence

1. Created the remediation branch from the audit commit; confirmed `main` unchanged.
2. Reproduced the desktop MCP cleartext path and added the shared URL policy (`is_loopback_host`, `remote_url_error`, `require_remote_url`).
3. Applied that policy at save, connect, OAuth discovery/registration/token/authorize and legacy-SSE message endpoints.
4. Moved MCP header values into the keychain; added startup migration and failure fallback.
5. Added Rust unit/integration coverage and two real-binary e2e checks.
6. Installed `cargo-audit`; scanned the lock; upgraded `zip`, `rusqlite`/`libsqlite3-sys`, `tokio`, `url` and `cookie`.
7. Re-ran the complete test/build matrix; bumped to 3.88.2 and updated reports.

## Files changed

- `app/src/mcp.rs` — URL policy, connect/save guards, SSE endpoint guard, redirect policy, header keychain storage/migration, tests.
- `app/src/mcp_oauth.rs` — cleartext refusal for discovery, registration, token and authorize endpoints; test.
- `app/src/main.rs` — startup `mcp::migrate_header_secrets(&state)`.
- `app/static/index.html` — MCP URL hint now requires HTTPS for remote servers.
- `app/Cargo.toml`, `app/Cargo.lock` — dependency upgrades and version 3.88.2.
- `scripts/ci/test-e2e.sh` — remote-cleartext refusal and loopback acceptance/removal checks (51 → 53).
- `CHANGELOG.md`, `app/packaging/io.github.sairajkhilariarch.SarthiStudio.appdata.xml`, `docs/KNOWN-ISSUES.md`, `docs/FEATURE-STATUS.md` — release/security record.
- Audit reports and the five remediation reports — updated with actual results.

## Phase 3 verification (3.88.3)

### What changed

- `scripts/mock-router.py` now answers a real `text/event-stream` when the request sets `stream`, and a `mock-slow-stream` model sends one delta and then holds the connection open.
- `scripts/ci/test-e2e.sh` grew from 53 to **61 checks**: a real multi-delta stream stored once, a timed abort that stores the partial reply exactly once and leaves the studio answering, and a setting saved through `/api/config` and read back after restart.
- `app/src/main.rs` answers `--version`/`--help` before resolving any path. `app/tests/cli.rs` runs the built binary and proves both exit without creating a home.

### Test results (all on the remediation branch)

| Check | Result |
|---|---|
| Rust `cargo test --locked` | **712 passed** (709 suite + 3 CLI), 0 failed, 1 ignored |
| `cargo clippy --all-targets -- -D warnings` | Pass |
| `cargo fmt --check` | Pass |
| Real-binary e2e — debug | **61 passed, 0 failed** |
| Real-binary e2e — release | **61 passed, 0 failed** |
| Frontend | **115 passed** |
| VS Code extension | **45 passed** |
| Website build | **37 pages** |
| Version / metadata / release / artifacts / bundle | **26 / 9 / 27 / 12 / 13**, all pass |
| Installer dependency check | 0 passed, 1 skipped (no WiX) |
| `cargo audit` | exit 0, **0 vulnerabilities**, 5 warnings |
| Linux `.deb` payload smoke | **14/14** checks |

### Measured runtime behaviour

Release binary, scratch home, local mock provider:

```text
startup_ms=61
idle_cpu_pct=0.0
rss_after_boot_kb=88908
rss_after_100_turns_kb=91796  (growth 2888 KB after warm-up)
turns_ok=100/100  avg_turn_ms=48  threads=33
```

This is a short stability measurement, not a multi-hour soak; see `REMAINING_ISSUES.md` R-11.

### Packaging

`cargo-deb` (installed project-locally under `.scratch/tools`) assembled `free-ai-studio_3.88.3-1_amd64.deb` (5,188,988 bytes) from the headless release build. Its unpacked payload:

- contains the binary, UI and bundled skills;
- boots headless, serves the UI and reports `3.88.3` over HTTP;
- writes to the documented home path;
- keeps a conversation and a saved setting when the payload is unpacked over it (the upgrade shape).

**Limit:** `dpkg-shlibdeps` is not present on this Arch host, so the control file's `Depends:` is empty and this artifact is **not** a shippable `.deb`. The official Ubuntu CI job still has to produce the dependency-resolved package. No system-wide install was performed (no root).

### Branch-safety incident and recovery

During the first full run, an external `git checkout main` at 21:49:29 moved the worktree while the script was running; no repository script performs a checkout. The debug e2e (53 checks) had already run on the remediation branch, but the release e2e and later gates ran against `main`. The branch was restored, and the entire matrix was rerun with a guard that records `HEAD` before and after. Final run: `head_before = head_after = 2858e4c`, branch `remediation/security-2026-11-24`. `main` was never modified.

### Finding totals

| Category | Count |
|---|---|
| Total tracked findings (audits + Phase 3 review) | 17 |
| Fixed and verified | 10 |
| Partially fixed | 1 (Linux package payload; system install and `$auto` metadata outstanding) |
| Remaining / open | 3 (browser suite, per-message edit/delete/regenerate, `glib` unsoundness) |
| Accepted risk | 1 (3 moderate dev-only npm advisories in the VS Code packaging tool) |
| Blocked | 2 (code signing/notarization; Windows/macOS/Android execution) |

## Phase 5 — performance and reliability (3.88.5–3.88.6)

Phase 5 established a reproducible baseline on a 200,000-message isolated
database, then fixed only measured bottlenecks.

| Commit | Change |
|---|---|
| `db12aad` (3.88.5) | `messages(conversation_id, id)` index; WAL; `BEGIN IMMEDIATE` for chat writes; startup directory errors; two Rust regression tests |
| `24c3359` (3.88.6) | per-frame stream painting; latest-200 history paging with explicit earlier pages; single-scroll bulk render; frontend regression test; long-answer fixture |

Measured on the same benchmark harness: list 680–790 ms → 1.46–3.79 ms; thread
read 16–22 ms → 2.0–5.3 ms; 2,000-message open 3,589.5 ms → 462 ms; 19k stream
~106.6 s of long tasks → 186 ms with none. Sixteen concurrent chats previously
failed 4/16 with `database is locked`; 32 workers × 64 requests now return zero
errors. Startup, idle CPU, idle RSS and shutdown remain within noise; soak RSS
plateaus at ~206.5 MB.

Fault injection verified provider-down/500 handling, a held writer lock, client
disconnect (partial answer preserved), SIGKILL mid-stream (integrity ok, user turn
only), corrupt database refusal, and restart recovery. One limitation remains: a
malformed HTTP 200 provider body is surfaced as an answer rather than rejected as
a protocol error. Details in `PERFORMANCE_BASELINE.md`,
`PERFORMANCE_OPTIMIZATION_REPORT.md`, `RELIABILITY_TEST_REPORT.md` and
`STRESS_TEST_REPORT.md`.

## Definition-of-done audit

| Criterion | Result |
|---|---|
| Safely fixable critical issues resolved | **Yes** for source-level findings; external signing gap remains |
| High-priority vulnerabilities fixed or mitigated | **Yes**: P1 origin guard retained; P1 dependency advisories removed; SEC-03/05 fixed |
| Broken features restored | **Yes** for the three disconnected behaviors; no feature was removed |
| Regression tests pass | **Yes**: 712 Rust, 115 frontend, 45 extension, 61 e2e on debug and release |
| Existing working functionality preserved | **Yes**: no route, schema, config key or feature removed; full prior suite passes |
| Production builds succeed on tested platforms | **Yes** on Linux debug/release; cross-platform blocked |
| Remaining blockers documented | **Yes** in `REMAINING_ISSUES.md` |

## Next engineering steps

1. Run the normal CI and a tagged 11-asset release; verify the Windows/macOS/Android jobs on their own hosts.
2. Produce the `.deb` in the Ubuntu CI job so `$auto` dependency metadata is resolved, build the GUI tarball/AppImage, and perform a real install/upgrade/uninstall on a disposable machine.
3. Decide the signing/notarization policy and obtain certificates.
4. Add a real Linux Secret Service smoke (current keychain test uses the project’s hermetic mock backend).
5. Resolve or explicitly accept the five RustSec warnings, especially the Tauri/GTK `glib` line.
6. Add browser layout/accessibility coverage; the timed slow-stream stop is now covered.
