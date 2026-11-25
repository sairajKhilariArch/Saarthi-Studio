# Sarthi Studio — Phase 8 Completion Report

**Date:** 2026-11-25
**Branch:** `platform/phase-8-cross-platform-2026-11-25`
**Code version:** 3.88.12 (docs commit 3.88.13)
**Base:** Phase 7 `e637722` / 3.88.9

## Outcome

Phase 8 shipped concrete cross-platform fixes and validated them with native CI:

- The Linux `.deb` now contains the entire runtime UI; the prior package would have installed a broken workbench.
- Android release artifacts match the documented unsigned-signing flow instead of being signed with a debug key.
- Android LAN HTTP is enforced in code instead of invalid CIDR rules that Android never matched.
- Android 15 edge-to-edge content is padded by real system-bar/cutout insets.
- The conversation database is owner-only on Unix.
- The installer-dependency test is hermetic on runners that have `mold`.
- Desktop CI passes on Linux, Windows and macOS; Android CI passes including API 35 emulator instrumentation.

**Phase 8 is not 100% cross-platform complete.** Windows/macOS installation, launch and signing and Android physical-device validation remain blocked by unavailable hardware, OS sessions and certificates. The matrix records each state.

## Definition of done

| Criterion | Status | Evidence |
|---|---|---|
| Shared platform architecture validated | **Met** | Audit of Rust/desktop/mobile/web layers; platform-specific modules isolated behind shared interfaces |
| Linux support tested | **Partially met** | Built, packaged, unpacked, launched, DB/assets/security verified; `dpkg` install/upgrade/uninstall blocked |
| Windows support tested | **Partially met / blocked** | Native CI build, 719+3 tests, clippy, WiX packager test; no install or launch |
| macOS support tested | **Partially met / blocked** | Native CI build, tests, clippy; no install, launch, signing or notarization |
| Android support tested | **Met for emulator** | APK/AAB built, debug APK installed and 6 instrumentation tests passed on API 35; physical device blocked |
| Web support tested | **Met for loopback Chromium** | Assets, headers, origin/host rejection, persistence, 125 frontend tests, Phase 7 responsive evidence; no accounts exist |
| Installation and upgrade paths validated | **Not met / blocked** | Linux payload boot verified; no system install or version-to-version upgrade on any desktop OS |
| User data survives upgrades | **Partially met** | Restart persistence and migrations verified; package upgrade on Windows/macOS/Linux not executed |
| Platform-specific security validated | **Partially met** | Linux permissions/DB fixed; Android Keystore/clear-text/signing fixed; CSP/origin verified; Windows/macOS runtime credential stores blocked |
| Automated tests pass | **Met** | Rust 719 + 3, frontend 125, extension 45, E2E 76 debug + 76 release, version 26, release 27, website 37 pages, Android CI green |
| CI evidence available | **Met** | Desktop run `45687899519` 3/3; Android run `45686438710` success |
| All reports accurate | **Met** | Ten reports with states and blocked items separated |

## Final regression results

| Check | Command | Result |
|---|---|---|
| Rust tests | `cargo test --locked` | **719 passed + 3 CLI**, 1 ignored |
| Clippy | `cargo clippy --all-targets -- -D warnings` | pass |
| Formatting | `cargo fmt --check` | pass |
| Frontend | `scripts/ci/test-frontend.sh` | **125 passed** |
| E2E debug | `scripts/ci/test-e2e.sh` | **76 passed** |
| E2E release | release binary + `E2E_NO_BUILD=1` | **76 passed** |
| Version contracts | `test-version.sh` | **26 passed** |
| Release contracts | `test-release.sh` | **27 passed** |
| Artifact gate | `test-artifacts.sh` | **12 passed** |
| Linux metadata | `test-linux-metadata.sh` | **9 passed** |
| `.deb` payload | `test-deb-assets.sh` + unpack/boot | **19 files packaged; all assets 200** |
| VS Code extension | `npm test` | **45 passed** |
| Website | `npm run build` | **37 pages** |
| Dependency audit | `cargo audit` | **0 vulnerabilities**, 5 allowed warnings |
| Desktop CI | run `45687899519` | Linux, Windows, macOS **success** |
| Android CI | run `45686438710` | **success**; 6 instrumentation tests |

The local host lacks `mold`, so Cargo commands used the repository's documented `lld` workaround (`CARGO_ENCODED_RUSTFLAGS`). CI uses the repository default and passed.

## Commits

```
e637722  3.88.9   Phase 7 base
3d30f6b  3.88.10  complete package payloads and harden Android platform rules
f5cb84b  3.88.11  make the installer dependency test independent of host mold
830c4e7  3.88.12  make the conversation database owner-only
<next>   3.88.13  Phase 8 validation reports (docs-only)
```

Pushed only to `platform/phase-8-cross-platform-2026-11-25`; `main` was not modified and no release was created.

## Remaining blockers

1. **Windows install/launch:** needs a Windows 10/11 machine; MSI install, upgrade, uninstall, Credential Manager and data preservation unverified.
2. **macOS install/launch:** needs a Mac; signing, hardened runtime, notarization, `.dmg`, Gatekeeper and Keychain unverified. No certificates.
3. **Linux system install:** needs dpkg/root (or a disposable Ubuntu VM); `.rpm` and Arch packages do not exist.
4. **Android physical device:** touch, rotation, background/foreground, keyboard and production signing unverified.
5. **Signing:** Windows Authenticode and macOS Developer ID/notary credentials are absent; Android release is intentionally unsigned until distribution.
6. **Update execution:** SHA-256 and prepare/rollback logic are tested, but no live release upgrade was installed on any platform.
7. **Notifications/tray/deep links:** not implemented; documented as not supported rather than faked.
8. **Other browsers:** the web UI was validated in Chromium only.

## Recommendation

Treat Linux (package payload/runtime) and Android (emulator) as the strongest validated surfaces. Before distribution, complete the Windows/macOS install matrices and signing pipeline. Do not describe Saarthi Studio as fully validated on Windows or macOS until those rows change from **blocked** to **verified**.
