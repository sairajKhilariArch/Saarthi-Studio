# Sarthi Studio — Release Readiness Report

**Date:** 2026-11-24  
**Candidate:** 3.88.2<br>
**Branch:** `remediation/security-2026-11-24`<br>
**Baseline:** audit commit `9721a53`; `main` at `55d1218` untouched<br>
**Host:** Linux x86_64 (local); no Windows, macOS, Android, signed-installer or tagged-release runner available.

## Recommendation

**Release 3.88.2 as a Linux patch candidate after the normal GitHub checks and a tagged 11-asset build are green.** The two code-level security gaps found in remediation are fixed and regression-tested, `cargo audit` reports 0 vulnerabilities, and the Linux debug/release binaries pass the expanded 53-check end-to-end suite.

**Not a full cross-platform production sign-off.** Windows/macOS installed-package behavior, Android, code signing/notarization, a real tagged release run and real third-party provider/MCP peers were not observable on this host.

## Build status

| Item | Status | Evidence |
|---|---|---|
| Debug build (`--locked`) | **Pass** | Build succeeds; 228 MB binary |
| Production build (`--release --locked`) | **Pass** | 20 MB binary built in 2m17s (final incremental build) |
| Release binary boot | **Pass** | 3.88.2 reaches readiness in both real-binary e2e runs |
| Release binary RSS | **Observed** | 87.5 MiB at idle; ~88–94 MiB after a live UI session |
| Live response headers | **Pass** | CSP, XFO DENY, nosniff, no-referrer, no-store; foreign Host 403 |
| Linux desktop metadata | **Pass** | 9/9 |
| Version consistency | **Pass** | 26/26 |
| Release contract | **Pass** | 11 asset names defined and gated; actual platform packages not built here |
| Offline release gate | **Pass** | 27/27 |
| Artifact verification | **Pass** | 12/12 |
| macOS bundle fixture | **Pass** | 13/13 |
| Installer dependency check | **Pass** | Fake-PATH install test |
| Windows MSI packager | **Skipped** | No WiX CLI on this host (`0 passed, 1 skipped`) |
| Actual deb/AppImage/tarball build | **Not run** | No `dpkg-deb`/`appimagetool` here |
| Actual Windows/macOS packages | **Not run** | No Windows/macOS host |
| Tagged GitHub release | **Not run** | No release authorization/tag in this audit |

The release contract for 3.88.2 is:

```text
free-ai-studio-android.aab
free-ai-studio-android.apk
free-ai-studio-vscode.vsix
free-ai-studio_3.88.2-1_amd64.deb
sarthi-studio_3.88.2_linux-x86_64.tar.gz
sarthi-studio_3.88.2_macos-universal.app.zip
sarthi-studio_3.88.2_macos-universal.tar.gz
sarthi-studio_3.88.2_windows-x86_64.msi
sarthi-studio_3.88.2_windows-x86_64.zip
sarthi-studio_3.88.2_x86_64.AppImage
SHA256SUMS.txt
```

## Test status

| Suite | Result |
|---|---|
| Rust (`cargo test --locked`, lld) | **709 passed, 0 failed, 1 ignored** |
| Rust clippy (`--all-targets -- -D warnings`) | **Pass** |
| rustfmt (`--check`) | **Pass** |
| Frontend (`node --test`) | **115 passed, 0 failed** |
| Real-binary e2e — debug | **53 passed, 0 failed** |
| Real-binary e2e — release | **53 passed, 0 failed** |
| VS Code extension | **45 passed, 0 failed** |
| Website build | **37 pages** |
| npm production audit (VS Code) | **0 vulnerabilities** |
| npm full audit (VS Code) | 3 moderate, dev-only `@vscode/vsce`/glob/minimatch; attempted 4.1.1 upgrade left 2 and required Node 22, so reverted |
| npm full audit (website) | **0 vulnerabilities** |
| Rust advisory scan | **0 vulnerabilities**; 5 warnings (exit 0) |
| Android tests | **Blocked** — no JDK/SDK/Gradle/device |

### What the 53 e2e checks cover

Real binary, real SQLite, real filesystem, real PTY, real process kill/restart, mock OpenAI-compatible provider:

- boot/readiness/database/data-directory checks;
- foreign `Host`/`Origin` refusal;
- conversation create, chat, stream, storage, budget, deletion;
- config secret redaction;
- project/memory/workflow CRUD;
- skill catalogue;
- remote cleartext MCP save refused (400); loopback HTTP save/remove accepted;
- workspace open and file create/save/read/rename/delete;
- path traversal refusal;
- interactive terminal command;
- agent run through approval, write, verification, step log;
- restart persistence;
- provider outage and failed-run record;
- kill mid-run and interrupted recovery;
- corrupt database refusal and recovery;
- unwritable data directory and recovery.

The same checks were run against the optimized release binary, not only the debug binary.

## Security status

| Area | Status |
|---|---|
| Loopback `Host`/`Origin` guard on all local routes | **Fixed and verified** (SEC-01) |
| Recent-folder HTML injection | **Fixed and verified** (SEC-02) |
| Secret redaction (config/providers) | Verified |
| Host API bearer/TLS/bind policy | Rust tests pass; no real device |
| Workspace containment | e2e traversal + Rust symlink tests |
| Terminal/agent approvals | e2e approval + Rust permission tests |
| Wasm extension sandbox | Rust/ADR tests; no hostile third-party extension installed |
| CSP/security headers | Live response verified |
| Update checksum/zip-slip | Rust tests; no live update |
| Remote MCP HTTPS-only | **Fixed and verified** (SEC-03; Rust + e2e) |
| MCP saved headers in OS keychain | **Fixed and verified** (SEC-05; mock keychain) |
| Signed packages | **Open/blocked** |
| Rust advisory scan | **0 vulnerabilities**; 5 warnings |

No P0 or unmitigated P1 issue remains. The loopback-boundary and dependency P1s are fixed and covered by the release-blocking tests.

## Known limitations and blockers

### Release blockers for a fully signed cross-platform claim

1. **Windows package not built or installed here.** WiX is absent; MSI regression suite skips as designed. Windows CI exists but no result was observed in this audit.
2. **macOS package is unsigned and un-notarized.** The bundle fixture passes; Gatekeeper behavior is untested.
3. **Android not built/tested.** No JDK, SDK, Gradle wrapper or device.
4. **No tagged 11-asset release run.** The contract and offline gate pass; the actual GitHub jobs did not run.
5. **No code signing certificates/notarization.**
6. **No real third-party provider, search, MCP, language-server, debugger or speech peer.**
7. **No browser layout/pixel/accessibility suite.** Live smoke covers load/theme/nav/chat only.
8. **Real OS keychain not exercised**; the header migration test uses the hermetic mock backend.
9. **Five RustSec warnings remain** (four unmaintained crates and glib 0.18.5 unsound); no vulnerabilities.

### Product/verification limitations that do not block a Linux patch

- Per-message edit/delete/regenerate is not implemented.
- Stop-generation was not timed against a slow stream (fast mock completed immediately).
- No long-session memory leak/soak profile.
- No real Secret Service/keychain integration exercised.
- Database upgrade from every historical release was not replayed; the suite includes pre-projects migration and backup-before-migration tests.
- The VS Code embedded server stores its OAuth session in a 0600 file (no SecretStorage bridge).

## Rollout plan

1. Run the normal push CI (`desktop-ci`, `rust-host-api-ci`, `android-ci` when Android paths change).
2. Confirm Linux/Windows/macOS desktop jobs are green at the candidate commit; do not infer Windows/macOS from Linux.
3. Build the tagged 11-asset release with `desktop-release.yml`; do not hand-upload a partial set.
4. Let `verify-release` refuse a missing/renamed asset and `release-audit.yml` recheck it.
5. Smoke the actual `.deb`/AppImage/tarball and, on their own hosts, the MSI and macOS `.app`.
6. Publish only after the signed/notarized policy decision is made; if unsigned, keep the documented Gatekeeper/SmartScreen warning.
7. Keep the foreign-Host/Origin and MCP TLS checks and all 53 e2e checks release-blocking.

## Rollback plan

- The change is a small patch on `main` with no schema migration and no data-path change.
- Reverting the source commit restores the prior behavior; no user data rollback is needed.
- The update path stages a verified download, keeps a backup, and supports rollback; no live update was applied in this audit.
- If a release artifact is wrong, remove the asset and rebuild from the tagged commit; never edit a published binary in place.

## Final release decision

| Question | Answer |
|---|---|
| Is the tested Linux build shippable? | **Yes, as 3.88.2 after CI/tagged build** |
| Is the security P1 closed? | **Yes** |
| Are regression tests in place? | **Yes, including release-binary e2e** |
| Is cross-platform runtime verified here? | **No** |
| Are installers signed? | **No** |
| Should this be described as a complete cross-platform production release? | **No** |
| Should the code be pushed/deployed by this audit? | **No — not requested; branch left local** |

**Release readiness: CONDITIONAL PASS for Linux 3.88.2.** The remaining conditions are execution of the normal CI/tagged-release pipeline and external platform/signing validation, not an unresolved code defect found in this remediation.
