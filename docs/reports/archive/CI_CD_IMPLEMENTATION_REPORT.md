# CI/CD Implementation Report — Phase 9

**Branch:** `release/phase-9-production-2026-11-25`
**Implementation commit:** `0af8acb` (3.88.15)
**Final docs/CI commit:** 3.88.16 (audit working-directory + these reports)
**Public release:** None

## 1. Pull-request/main pipeline (`desktop-ci.yml`)

Every push/PR runs:

| Gate | Job |
|---|---|
| Rust formatting (`cargo fmt --check`) | Linux |
| Debug build + 719 unit tests + 3 CLI tests | Linux, Windows, macOS |
| Clippy `-D warnings` | Linux, Windows, macOS |
| 76-case debug E2E | Linux |
| Frontend 125 tests | Linux |
| Release version 26 tests | Linux |
| Release asset gate 27 tests | Linux |
| New metadata suite 8 tests | Linux |
| Version matrix (desktop/Android/VSIX/website) | Linux |
| Release contract (14 assets) | Linux |
| `.deb` asset guard + Linux metadata 9 tests | Linux |
| Installer dependency + macOS bundle tests | Linux |
| RustSec dependency audit | Linux |
| Website `npm ci` + `astro check` + build + output check | Website job |

Permissions are `contents: read` plus `checks: write` for the RustSec check
run. No PR job can publish or write a release.

### First run failure and fix

Run `45694547969` on `0af8acb`:

- Windows: **success**
- macOS: **success**
- Website: **success**
- Linux: **failure** at `Audit Rust dependencies`

The RustSec action ran at the repository root and looked for `./Cargo.lock`,
but the workspace lockfile is `app/Cargo.lock`:

```
error: not found: Couldn't load ./Cargo.lock
##[error]Unexpected end of JSON input
```

Fixed in 3.88.16 by adding the action's documented input:

```yaml
with:
  token: ${{ secrets.GITHUB_TOKEN }}
  working-directory: app
```

The audit was not removed or allowed to fail. The next run is the final Linux
evidence.

## 2. Release pipeline (`desktop-release.yml`)

### Triggers

- `workflow_dispatch` with optional existing `tag` and `publish` (default false)
- `push` of a `v*` tag
- The old `release: published` trigger was removed

A tag push builds and verifies but **never publishes**. Publication requires a
dispatch with `publish=true` and a tag that exactly matches `Cargo.toml`.

### Jobs

| Job | Runner | Output |
|---|---|---|
| `preflight` | Ubuntu | Validates version/tag; exports version/tag/release |
| `deb` | Ubuntu 22.04 | headless `.deb`, static dependency/UI checks, unpack+boot |
| `release` | Ubuntu | portable tarball + AppImage, boot smoke |
| `windows` | Windows | zip + WiX MSI, package smoke |
| `macos` | macOS | universal binary, `.app.zip` + tarball, lipo + HTTP smoke |
| `vscode-extension` | Ubuntu | VSIX, extension tests, `vsce` packaging |
| `android` | Ubuntu + SDK 35 | unsigned release APK/AAB, structural smoke |
| `finalize` | Ubuntu | SBOM, notes, manifest, global checksums, 14-asset validation, provenance |
| `publish` | Ubuntu | **only** write job; existing-release refusal + publish gate + release upload |
| `verify-release` | Ubuntu | re-reads the published tag and checks all 14 assets |

### Permissions

Top-level `contents: read`. `finalize` adds `id-token: write` and
`attestations: write` for provenance. `publish` alone gets `contents: write`.
This is a deliberate least-privilege change from the previous workflow, which
gave every job write access.

### Publication gate

`check-publish-gates.sh` validates the complete bundle and then requires all
five signing properties to be `true`. With no certificates/keystore, it exits
**3 BLOCKED**:

```
BLOCKED: the bundle is complete and verified, but publication is refused because:
  BLOCKED  Windows Authenticode signature
  BLOCKED  macOS Developer ID signature
  BLOCKED  macOS notarization and stapling
  BLOCKED  Android release keystore signature
  BLOCKED  signed Linux release manifest
```

This is intentional: the pipeline can produce and verify artifacts, but cannot
publish an unsigned production release. No public release was created.

## 3. Release run evidence

### Run `45694555979` (3.88.15, no tag, publish=false)

| Job | Result |
|---|---|
| preflight / tag validation | **success** |
| `.deb` | **success** |
| Linux tarball + AppImage | **success** |
| Windows zip + MSI | **success** |
| VS Code VSIX | **success** |
| Android unsigned APK/AAB | **success** |
| macOS universal | **success** (universal arm64+x86_64, 25 min build) |
| finalize (14 assets, SBOM, checksums, provenance) | **success** |
| publish | **skipped** (publish=false) — no public release |
| verify-release | **skipped** (depends on publish) |

Finalize evidence, downloaded independently from the run:

```
14 files, 69 MB total
SHA256SUMS.txt covers and matches 13 files          OK
metadata valid: 523 SBOM components, 12 manifest artifacts
Attest artifact provenance                          success
```

The publish gate on the downloaded bundle exits **3 BLOCKED** for all five
signing properties, so no release could be created.

No tag was pushed. This run is an artifact-only exercise of the real workflow.

### Run `45694560967` (Android CI, 3.88.15)

**success** — unit tests, lint (0 errors), debug/release assembly, API 35
emulator instrumentation (6 tests).

### Run `45694547969` (Desktop CI, 3.88.15)

Windows/macOS/website success; Linux failed at the RustSec path. Fixed in
3.88.16; final run is recorded in the completion report.

## 4. Local gate evidence

| Command | Result |
|---|---|
| `cargo test --locked` | **719 passed**, 3 CLI passed, 1 ignored |
| `cargo clippy --locked --all-targets -- -D warnings` | **PASS** |
| `cargo fmt --check` | **PASS** |
| `cargo build --release --locked` | **PASS** |
| `cargo audit` | 0 vulnerabilities, 5 allowed warnings |
| `test-e2e.sh` debug | **76 passed** |
| `test-e2e.sh` release binary | **76 passed** |
| `test-frontend.sh` | **125 passed** |
| VS Code `npm test` | **45 passed**, 0 failed |
| Website `check`/`build`/`check:build` | **PASS**, 37 pages |
| `test-version.sh` | **26 passed** |
| `test-release.sh` | **27 passed** |
| `test-release-metadata.sh` | **8 passed** |
| `test-artifacts.sh` | **12 passed** |
| `test-linux-metadata.sh` | **9 passed** |
| `verify-release-contract.sh` | 14 assets, all built/gated |
| `version-matrix.sh --check` | consistent |

## 5. Branch protection / merge policy

The workflow is a required-check candidate, but repository branch-protection
settings were not modified or verified in this phase. The brief says PRs must
not merge when required checks fail; enforcing that requires the repository
owner to mark `Test, lint, and build desktop`, `Website production build` and the
platform jobs as required in GitHub settings. This cannot be set from code.

## 6. Known CI limitations

- macOS/Windows builds are compile/package tests, not install/launch tests.
- Signing is absent, so `publish` cannot succeed.
- Android release artifacts are unsigned diagnostics.
- Actions use version tags, not SHA pins.
- No staging deployment environment exists.
- GitHub's macOS runner is slow; the universal build dominates run time.
