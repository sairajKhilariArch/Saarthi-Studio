# Sarthi Studio — Final Acceptance Report

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Branch:** `acceptance/phase-10-final-2026-11-26`  
**Base:** `878c50f` (3.88.16, Phase 9)  
**Acceptance code:** 3.88.17 (`1d6e458`) plus the report commit  
**Authority:** independent verification of the actual repository, binary and UI; prior phase reports were treated as claims

## Decision

# CONDITIONALLY ACCEPTED

No critical or high-severity code defect remains in the tested build. The application is usable and stable on the Linux x86_64 host, its CI build matrix and release gates are coherent, and its failure handling is materially better than the pre-acceptance state. It is **not** accepted for unrestricted public production release because signing, real Windows/macOS installation, Linux system-package lifecycle and live update/rollback remain unverified.

## Accepted scope

| Scope | Status |
|---|---|
| Linux x86_64, release binary, headless/local server + browser UI | **Accepted** for the tested build, with the limitations below |
| Windows x86_64 | **Build/CI only**, not accepted for installation or use |
| macOS universal | **Build/CI only**, not accepted for installation or use |
| Android companion | **Emulator/CI only**, unsigned; not production-distributed |
| Web | Loopback UI only; not a public multi-user deployment |
| Public GitHub release / tag / merge to main | **Not performed and not authorized** |

## What was independently verified

### Regression (3.88.17)

| Check | Result |
|---|---|
| `cargo test --locked` | **721 passed, 0 failed, 1 ignored**; plus **3 CLI tests passed** |
| `cargo clippy --locked --all-targets -- -D warnings` | **PASS** |
| `cargo fmt --check` | **PASS** |
| `cargo build --release --locked` | **PASS** |
| Debug E2E | **76/76** |
| Release E2E | **76/76** |
| Frontend | **125/125** |
| VS Code extension | **45/45** |
| Website | `astro check` 0 errors / 0 warnings, 37 pages |
| `cargo audit` | 0 vulnerabilities, 5 warnings |
| VSIX production audit | 0 vulnerabilities (`--omit=dev`); 3 moderate dev-only |
| Website audit | 0 |

### Live UI

The application served at `127.0.0.1:38100` with title “Sarthi Studio”, 10 primary navigation controls, 281 buttons and no console errors. Skills and Settings were opened; dark/light themes round-tripped; 1440×900 and 390×844 showed no horizontal overflow. Screenshots could not be captured because the preview window was minimized/covered; this remains a verification gap, not a pass.

### Performance (release binary)

Cold start **83 ms**, warm **67–84 ms**, shutdown **12 ms**, boot RSS **87.7–88.3 MB**, 1,000-message conversation load **2.2 ms**, 1,000-file index **39 ms**, chat p50 **45 ms** against the mock provider. A ~7.5-minute soak held at **90,848 KB** (+108 KB in the measured final 2 minutes, 0 errors). Phase 5 baseline: 67–93 ms startup, ~88–90 MB idle. No 2–4 hour soak was run.

### Reliability

Provider outage, aborted stream, killed process, corrupt database, unwritable directory, database permissions, skill failure, workflow failure, cancellation and restart recovery all behaved correctly. Two defects were found and repaired:

1. **macOS watcher test race** — fixed with an index-sentinel handshake; previously failed on macOS CI.
2. **Silent loss of an unreadable `config.json`** — now preserved as `config.json.corrupt-<epoch>` before defaults replace it, with tests and a release-binary reproduction.

A third documentation defect was repaired: README commands used a non-existent uppercase binary and wrong environment-variable prefix. The saved-key help text was also corrected.

## Detailed reports

1. `FINAL_FEATURE_INVENTORY.md`
2. `FINAL_SECURITY_AUDIT.md`
3. `FINAL_PERFORMANCE_ACCEPTANCE.md`
4. `FINAL_RELIABILITY_REPORT.md`
5. `FINAL_PLATFORM_ACCEPTANCE.md`
6. `FINAL_UI_ACCEPTANCE.md`
7. `FINAL_RELEASE_ACCEPTANCE.md`
8. `FINAL_TECHNICAL_DEBT_REGISTER.md`
9. `PRODUCTION_READINESS_SCORECARD.md`
10. This report
11. `FINAL_USER_GUIDE.md`
12. `INSTALLATION_AND_UPGRADE_GUIDE.md`

## Release engineering evidence

Phase 9 run `45694555979` built **7/7** platform jobs, finalized successfully, downloaded 14 files, verified **13/13** SHA-256 checksums, produced a **523-component** CycloneDX SBOM, a 12-artifact manifest and provenance attestation, and **refused publication** with 3 BLOCKED signature checks. Phase 10 Desktop CI run `45704212041` was dispatched on 3.88.17 for Linux, Windows, macOS and website. Its completion state is recorded in the CI section below.

### Phase 10 CI result

**Run:** https://github.com/sairajKhilariArch/Saarthi-Studio/actions/runs/45704212041 — **success**

| Job | Result |
|---|---|
| Test, lint, and build desktop (Linux) | **success** |
| Test and build desktop (Windows) | **success** |
| Test and build desktop (macOS) | **success** |
| Website production build | **success** |
| Security audit | **success** |

The macOS log records `test result: ok. 721 passed; 0 failed; 1 ignored` plus 3 CLI tests, and specifically `filewatch::tests::a_started_watcher_indexes_a_file_written_after_open ... ok`. The Phase 9 macOS race is closed on native macOS CI.

## Mandatory limitations / not ready

| Limitation | Impact |
|---|---|
| Unsigned Linux/Windows/macOS artifacts | Origin cannot be cryptographically verified; publish gate blocks |
| Windows/macOS not installed or launched | No claim of a working installer, GUI, terminal or credential store |
| Linux system install/upgrade/uninstall not run | `.deb` dependency metadata and package lifecycle unverified |
| No live update/rollback | Update path only unit-tested |
| No 2–4 h soak or disk-full test | Long-term stability unproven |
| No UI screenshots / full keyboard pass | Visual and accessibility acceptance incomplete |
| All providers/search/MCP/LSP/debugger peers were local fixtures | Vendor API compatibility unverified |
| Real OS keychain unused | Credential-store integration unverified |
| Android unsigned, emulator only | Not distribution-ready |

## Prioritized repair list before unrestricted release

1. Obtain and configure Linux GPG, Windows Authenticode and macOS Developer ID/notarization; sign and verify the complete asset set.
2. Install, launch, upgrade and uninstall on real Windows and macOS hosts; record results.
3. Install/upgrade/purge the `.deb` on a disposable Ubuntu/Debian host with resolved dependencies.
4. Publish a controlled test release; verify update, integrity failure handling and rollback with user data.
5. Run a 2–4 hour soak and a disk-full test.
6. Add real-provider, real-keychain and screenshot/accessibility validation.
7. Upgrade the GTK/Tauri line to remove the `glib` unsoundness warning, after GUI regression testing.

## Commit and branch control

- Work is confined to `acceptance/phase-10-final-2026-11-26`.
- No merge to `main`, no tag, no GitHub release, no deployment.
- Code commit `1d6e458` is 3.88.17; the report commit bumps to 3.88.18.
- No AI attribution trailer is used, per `AGENTS.md`.
