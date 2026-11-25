# Release Security Report — Phase 9

**Version:** 3.88.15 (implementation), 3.88.16 (CI path + reports)
**Commit:** `0af8acb` (3.88.15), reports/fix committed as 3.88.16
**Public release:** None

## 1. Workflow permissions

| Workflow/job | Permission | Reason |
|---|---|---|
| Desktop CI | `contents: read`, `checks: write` | Read source; RustSec publishes a check run |
| Release top level | `contents: read` | Build jobs must not be able to publish |
| `finalize` | `contents: read`, `id-token: write`, `attestations: write` | Provenance attestation only |
| `publish` | `contents: write` | Creates the release, after all gates |
| Website deploy | Existing Pages permissions, `main` only | Not triggered in Phase 9 |

No build job has write access to releases. `publish` is the only job that can
create one, and it is gated on `publish=true`, an existing tag and
`check-publish-gates.sh`.

## 2. Dependency audit

Local `cargo audit` and Desktop CI's RustSec step:

```
0 vulnerabilities
5 allowed warnings
```

| Crate | Advisory | Reason allowed |
|---|---|---|
| chrono | RUSTSEC-2026-0543 (unmaintained) | No fixed upgrade available |
| paste | RUSTSEC-2024-0436 (unmaintained) | Transitive macro dependency |
| proc-macro-error | RUSTSEC-2024-0370 (unmaintained) | Transitive |
| rustls-pemfile | RUSTSEC-2025-0134 (unmaintained) | Transitive TLS helper |
| glib 0.18.5 | RUSTSEC-2024-0429 (unsound) | Pinned by Tauri/GTK; upgrade requires stack move |

The first CI run failed before auditing because the action scanned the repo
root; `working-directory: app` is the fix. A failing audit is a release gate,
not an annotation to ignore.

## 3. Secrets and artifacts

- `git grep` for private-key blocks, `ghp_`, `github_pat_` and `sk-` tokens found
  only **test fixtures** (`app/src/privacy.rs`, extension sanitizer test and the
  SBOM self-check), not credentials.
- SBOM generation refuses if it finds secret markers.
- Release manifest/SBOM contain only public identifiers: version, tag, commit,
  repository, workflow ref, run ID, file hashes.
- No signing key, certificate, keystore, `.env` or token is committed.
- The release bundle is built from dedicated `dist/` output; source maps and
  secrets are not included. The `.deb` payload was inspected: binary, UI assets,
  desktop/AppStream metadata and license only.
- The website production output is static and has no environment secrets.

## 4. Supply-chain integrity

| Control | State |
|---|---|
| Global SHA-256 checksums over all 14 release files | **PASS** |
| Manifest binds every hash to commit/version/run | **PASS** |
| CycloneDX 1.5 SBOM (523 Rust components) | **PASS** |
| GitHub build provenance attestation | **PASS in workflow** (see CI report) |
| Tamper rejection test | **PASS** |
| Platform code signatures | **BLOCKED** (no credentials) |
| Publication blocked when unsigned | **PASS** — gate exits 3 |
| Update checksum verification | **PASS** (unit tests) |
| Update signature verification | **N/A** — no signatures exist |

## 5. Update-path security

- HTTPS-only download URLs.
- SHA-256 required; an unverifiable asset is refused.
- `.part` download, rename only after hash match.
- Archive extraction rejects absolute paths and `..`.
- Prepared tree must contain a recognized app binary.
- Running binary is never silently replaced; swap verifies the copy and rolls
  back on mismatch.
- Rollback refuses paths/backups the app did not create.

No downloaded binary is executed by the staging step. Preparation unpacks it;
the explicit apply step replaces the installed binary.

## 6. Privacy/observability

Phase 9 adds no telemetry, analytics or remote logging. Release diagnostics are
CI logs and the public manifest. No conversation content, API key or personal
data is added to release metadata. Existing local diagnostics remain local.

## 7. Residual risks

| Risk | Severity | State |
|---|---|---|
| No platform signing/notarization | High | **BLOCKED** |
| Unsigned Android release | High | **BLOCKED** |
| GitHub Actions referenced by tags, not SHAs | Medium | Accepted, documented |
| 5 unmaintained/unsound warnings | Medium | Accepted with rationale |
| No live update test against a real release | Medium | **BLOCKED** (no release) |
| Windows/macOS install validation absent | High | **BLOCKED** |
| Single-maintainer release credentials | Operational | Not addressed in code |

**No public release was created, so none of these risks were shipped.** The
publish gate refuses to do so while the trust properties are false.
