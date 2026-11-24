# Sarthi Studio — Remediation Progress

**Date:** 2026-11-24  
**Branch:** `remediation/security-2026-11-24`  
**Base:** `9721a53` (audit reports on `audit/full-application-2026-11-24`)  
**Release under remediation:** `3.88.2`  
**Status:** implementation complete for every code-level item that could be verified on this Linux host. Remaining items are external, platform, or explicitly accepted residuals.

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

## Definition-of-done audit

| Criterion | Result |
|---|---|
| Safely fixable critical issues resolved | **Yes** for source-level findings; external signing gap remains |
| High-priority vulnerabilities fixed or mitigated | **Yes**: P1 origin guard retained; P1 dependency advisories removed; SEC-03/05 fixed |
| Broken features restored | **Yes** for the three disconnected behaviors; no feature was removed |
| Regression tests pass | **Yes**: 709 Rust, 115 frontend, 45 extension, 53 e2e on debug and release |
| Existing working functionality preserved | **Yes**: no route, schema, config key or feature removed; full prior suite passes |
| Production builds succeed on tested platforms | **Yes** on Linux debug/release; cross-platform blocked |
| Remaining blockers documented | **Yes** in `REMAINING_ISSUES.md` |

## Next engineering steps

1. Run the normal CI and a tagged 11-asset release; verify the Windows/macOS/Android jobs on their own hosts.
2. Decide the signing/notarization policy and obtain certificates.
3. Add a real Linux Secret Service smoke (current keychain test uses the project’s hermetic mock backend).
4. Resolve or explicitly accept the five RustSec warnings, especially the Tauri/GTK `glib` line.
5. Add browser layout/accessibility coverage and a timed slow-stream stop test.
