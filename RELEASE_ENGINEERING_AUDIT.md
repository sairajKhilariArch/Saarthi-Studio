# Release Engineering Audit — Phase 9

**Repository:** `sairajKhilariArch/Saarthi-Studio`
**Branch:** `release/phase-9-production-2026-11-25`
**Base:** Phase 8 head `6bea5fa` (3.88.14)
**Phase 9 implementation commit:** `0af8acb` (3.88.15)
**Audit date:** 2026-11-25/26
**Public release created:** No

## 1. Scope and method

The audit inspected the release workflow, desktop/Android/website CI, version
helpers, artifact contract, packaging metadata, installer scripts, the update
engine and the Phase 8 platform reports. Every defect below was reproduced by
reading the workflow or by running the relevant gate; the fix is in `0af8acb`.

Phase 8 was verified at `6bea5fa` (3.88.14) with a clean tree before branching.
No `main` commit, tag, release or Phase 8 branch was modified.

## 2. Findings

| # | Severity | Finding | Evidence | State after 3.88.15 |
|---|---|---|---|---|
| R1 | Critical | Every build job attached its own artifacts to the public GitHub release (`softprops/action-gh-release`), and the Linux job used `generate_release_notes: true`. A release could become public before the other platforms or integrity gates ran. | `.github/workflows/desktop-release.yml` at `6bea5fa` | **Fixed.** Build jobs only upload private workflow artifacts. `finalize` validates the complete set; `publish` is a separate job gated on `publish=true` and an existing tag. |
| R2 | Critical | `SHA256SUMS.txt` was generated only in the Linux job, so it covered two Linux files and none of the Windows/macOS/Android/VSIX assets. | Linux job checksum step at `6bea5fa` | **Fixed.** One finalize job hashes every non-checksum file after downloading all seven platform artifacts. `verify-checksums.sh` requires the file list to be exact. |
| R3 | Critical | No SBOM, release manifest, curated notes or build provenance existed. GitHub's generated commit list was used as release notes. | Workflow assets and release body | **Fixed.** CycloneDX 1.5 SBOM (523 Rust components), `release-manifest.json` with commit/run/hashes, `RELEASE_NOTES.md` from the exact CHANGELOG section, and `actions/attest-build-provenance`. |
| R4 | Critical | The Android release job still claimed a “signed”/“debug-signed” APK and copied `app-release*.apk`. Phase 8 made release builds unsigned, so the real output is `app-release-unsigned.apk` and `smoke-apk.sh` refused it. | Release job vs `android/app/build.gradle.kts`; local smoke of the CI artifact | **Fixed honestly.** Job is named unsigned, copies the exact unsigned APK/AAB, runs the structural smoke with `SMOKE_ALLOW_UNSIGNED=1`, and writes a `BLOCKED` signing summary. It is not described as installable production. |
| R5 | High | Workflow-level `contents: write` applied to every job, including builds that never publish. | `permissions:` at `6bea5fa` | **Fixed.** Top level is `contents: read`; only `publish` has `contents: write`; `finalize` has `id-token`/`attestations` write for provenance. |
| R6 | High | Nothing enforced signing before publication. An operator could publish unsigned Windows/macOS/Android artifacts. | No signing gate | **Fixed.** `check-publish-gates.sh` exits **3 BLOCKED** unless the manifest records verified Authenticode, Developer ID, notarization, Android keystore and Linux manifest signatures. With no credentials, publication is correctly blocked. |
| R7 | High | Release asset names in `README.md` were stale (`SAARTHI-STUDIO_*`, `android-release-unsigned.apk`, `SAARTHI-STUDIO-vscode.vsix`) and macOS was described as arm64-only instead of universal. | `README.md` install table | **Fixed.** Table now matches `release-assets.sh`, includes the MSI, metadata files and signing status. |
| R8 | Medium | Desktop CI had no dependency audit and did not build the website production output. | `desktop-ci.yml` at `6bea5fa` | **Fixed.** RustSec audit job step and a dedicated website `npm ci`/`check`/`build`/`check:build` job. |
| R9 | Medium | No version matrix covered Android, VS Code and website consistency; only desktop tests existed. | `test-version.sh` scope | **Fixed.** `version-matrix.sh --check` validates desktop/AppStream/CHANGELOG, Android name+code+README, VSIX lockfile and website lockfile. |
| R10 | Medium | Release-workflow triggers included `release: published`, so publishing a release manually rebuilt and re-attached artifacts without the tag/version preflight contract. | `on:` block at `6bea5fa` | **Removed.** Only a `v*` tag push or an explicit dispatch can build; neither publishes automatically. |
| R11 | Low | No `release`-specific gate script; validation was spread across jobs and could drift. | Workflow steps | **Fixed.** `validate-release-metadata.sh`, `verify-checksums.sh`, `check-publish-gates.sh` and the 14-asset contract test are reusable and run in finalize/CI. |

## 3. What was deliberately not changed

- **No `.rpm` or Arch package** was added. The project has no RPM/Arch packaging
  or validation harness; generating one and calling it supported would be
  fabrication. The supported Linux artifacts remain `.deb`, portable tarball and
  AppImage.
- **No Windows/macOS signing configuration** was invented. There are no
  certificates, Apple Developer identity, notary credentials or Android release
  keystore in the repository or this machine. The pipeline records these as
  blocked instead of producing a fake signature.
- **No public release, tag or deployment** was created. The brief requires
  explicit authorization, which was not given.
- The update engine was not rewritten. It already verifies checksums, stages
  safely and has rollback logic; Phase 9 adds validation and reporting rather
  than replacing a working design.

## 4. Remaining release risks

| Risk | State | Why |
|---|---|---|
| Unsigned Windows/macOS artifacts | **BLOCKED** | No Authenticode certificate, Developer ID, hardened runtime or notarization credentials. |
| Unsigned Android release | **BLOCKED** | No production keystore; CI artifacts are diagnostics. |
| Windows/macOS install, launch, upgrade, uninstall | **BLOCKED** | No hardware; CI only builds/tests the source and packages. |
| Linux `dpkg -i` install/upgrade/uninstall | **BLOCKED** | This host has no `dpkg`/root; payload and version-to-version upgrade were tested from unpacked install layouts. |
| Live update from a published release | **BLOCKED** | No public release exists, so the network upgrade path was not exercised end-to-end. |
| GitHub Actions pinned to tags, not commit SHAs | **Accepted risk** | Existing project convention; pinning all actions is a supply-chain hardening follow-up. |
| `cargo audit` warnings | **Accepted** | 0 vulnerabilities; 5 unmaintained/unsound transitive warnings documented in `RELEASE_SECURITY_REPORT.md`. |

## 5. Verification of the audit fixes

| Check | Command | Result |
|---|---|---|
| Release contract | `scripts/ci/verify-release-contract.sh` | **PASS** — 14 assets, each built by a job and demanded by the gate |
| Release fixture suite | `scripts/ci/test-release.sh` | **PASS** — 27/27 |
| Metadata suite | `scripts/ci/test-release-metadata.sh` | **PASS** — 8/8 |
| Version matrix | `scripts/ci/version-matrix.sh --check` | **PASS** — 3.88.15 / 1.51.1 / 2.6.1 / 0.5.2 consistent |
| Workflow YAML parse | `python3 -c 'yaml.safe_load(...)'` | **PASS** — 10 jobs, top permission `contents: read` |
| Publication gate on complete unsigned fixture | `scripts/ci/check-publish-gates.sh` | **BLOCKED (3)** — expected; five signing properties absent |

The real pipeline evidence is recorded in `CI_CD_IMPLEMENTATION_REPORT.md` and
`BUILD_MATRIX_REPORT.md`.
