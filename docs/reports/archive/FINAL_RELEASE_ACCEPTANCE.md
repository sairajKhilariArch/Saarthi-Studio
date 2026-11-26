# Sarthi Studio — Final Release Acceptance

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Release candidate:** 3.88.17 (`1d6e458`), branch `acceptance/phase-10-final-2026-11-26`  
**Policy:** no public release, tag, merge or deployment was performed. Publication requires explicit user authorization.

## Version consistency

| Location | Value | Result |
|---|---|---|
| `app/Cargo.toml` | 3.88.17 | PASS |
| `app/Cargo.lock` package entry | 3.88.17 | PASS |
| `free-ai-studio --version` | `Sarthi Studio 3.88.17` | PASS |
| AppStream `<release>` | 3.88.17 added, date 2026-11-26 | PASS |
| `CHANGELOG.md` | 3.88.17 section | PASS |
| Phase 10 reports | describe 3.88.17, final report commit 3.88.18 | PASS (documented) |

No duplicate or conflicting version string was found in `app/src` or `app/static`.

## Phase 9 pipeline evidence (most recent completed release run)

Run `45694555979` on 3.88.16:

| Gate | Result |
|---|---|
| Isolated platform builds | **7/7 success** |
| Finalize stage | success |
| Publish | **skipped** (no publish authorization) |
| Verify release | skipped |
| Bundle download | 14 files |
| SHA-256 verification | **13/13 OK** |
| SBOM | CycloneDX, **523 components** |
| `release-manifest.json` | 12 artifacts bound to source commit and run |
| Provenance attestation | success |
| Publish gate | **3 BLOCKED** — Linux GPG, Windows Authenticode, macOS Developer ID/notarization signatures absent |

This is the strongest end-to-end release-pipeline evidence: artifacts are produced, checksummed, inventoried and attested, and publication is correctly refused while signatures are missing.

## Phase 10 CI

`Desktop CI` run **45704212041** on `acceptance/phase-10-final-2026-11-26` at 3.88.17 completed **successfully (5/5)**: Linux desktop, Windows desktop, macOS desktop, website production build and Security audit. The macOS log records 721 passed, 0 failed, 1 ignored, 3 CLI tests, and the previously flaky `filewatch::tests::a_started_watcher_indexes_a_file_written_after_open` passing.

Local Phase 10 suite passed before dispatch:

| Check | Result |
|---|---|
| `cargo test --locked` | 721 passed, 0 failed, 1 ignored; 3 CLI passed |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `cargo fmt --check` | PASS |
| `cargo build --release --locked` | PASS |
| Debug E2E | 76/76 |
| Release E2E | 76/76 |
| Frontend | 125/125 |
| VS Code extension | 45/45 |
| Website | `astro check` 0 errors/0 warnings, 37 pages |

## Release gates

| Requirement | State | Evidence |
|---|---|---|
| Version consistency | PASS | table above |
| Build on Linux/Windows/macOS | PASS (CI) | Phase 9 7/7; Phase 10 run 45704212041 success 5/5 |
| Installer generation | PARTIAL | `.deb`, AppImage, tarball, zip, MSI, `.app`, APK/AAB produced in CI; only `.deb` payload locally verified |
| Checksums | PASS | 13/13 Phase 9 bundle |
| SBOM | PASS | 523 components |
| Provenance | PASS | attestation succeeded |
| Signing | **BLOCKED** | no GPG/Authenticode/Developer ID credentials |
| Unsigned artifacts not advertised as signed | PASS | README and reports explicitly say unsigned; publish gate blocks |
| Update verification | PARTIAL | hash/rollback unit-tested; no live update |
| Rollback | PARTIAL | update rollback logic unit-tested; package rollback not executed |
| Release notes | PASS | `CHANGELOG.md` and generated `RELEASE_NOTES.md` |
| Artifact availability | PASS (Phase 9 bundle) | downloaded and verified |
| Unsupported platforms not advertised as fully supported | PASS after Phase 10 docs | platform matrix and acceptance reports state blocked levels |
| Mandatory gate refuses failure | PASS | publish gate returned BLOCKED, pipeline did not publish |

## Actual release assets (Phase 9 bundle)

The verified bundle contained the Linux `.deb` and AppImage/tarball, Windows zip/MSI, macOS universal app zip/tarball, Android APK/AAB, VS Code VSIX, JSON manifest/SBOM and `SHA256SUMS.txt` (14 files including metadata; 13 checksums). No artifact from that run was published.

## Release acceptance limitations

- Windows and macOS installers were built but never installed or launched.
- All signatures are absent; the release is not trust-verified beyond checksums and provenance.
- No live update from a published release was possible because no release exists.
- The `.deb` dependency metadata was not resolved on this Arch host; system installation is unverified.
- Android release artifacts are unsigned by design.

## Verdict

**PARTIAL.** The release machinery is coherent, reproducible and fails closed on missing signatures. A public release is **not authorized and not accepted** in this state; it would ship unsigned, uninstalled-on-Windows/macOS artifacts with no live update validation.
