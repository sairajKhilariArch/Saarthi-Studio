# Progress checklist

Persistent record of what is done, what is in flight and what is next. Update on
every shipped commit. Status values: `[x]` verified with evidence, `[~]`
partial, `[ ]` not started.

## Release line

| Version | Commit | Content | Cut |
|---|---|---|---|
| 1.26.2 | `75c4192` | LF restore + CR-safe version reads (workflow later found invalid) | tag has no assets |
| 1.26.3 | `58cbea1` | Valid release workflow (block scalar version reads); first release with assets | 8 assets, macOS missing |
| 1.26.4 | this commit | Phase 1: single version parser, artifact gate, regression tests, AppStream metadata, release concurrency, spec docs | in flight |

## Phase 1 — CI/CD and release reliability

- [x] Diagnose CR-in-artifact-name failure (`41693462775`) to the CRLF manifest rewrite
- [x] Restore LF manifests and normalise the 12 files the bump script flipped
- [x] Make the workflow valid again (block-scalar version reads, YAML parse check)
- [x] Publish a release with real assets (`v1.26.3`, 8 assets + checksums)
- [x] One version parser for Linux/macOS/Windows (`scripts/ci/version.sh`)
- [x] Version validation: non-empty + semver shape
- [x] Artifact-name validation before upload (`scripts/ci/verify-artifacts.sh`)
- [x] Checksum verification inside the artifact gate
- [x] Regression suite for LF/CRLF/empty/malformed versions + bad names
- [x] Wire the regression suite into `desktop-ci.yml`
- [x] Release `concurrency` so two tags cannot race
- [x] AppStream metadata for the AppImage
- [ ] macOS job actually runs and attaches its tarball
- [ ] Install smokes on produced artifacts (deb, AppImage, zip, vsix, apk)
- [ ] Decide fate of dead tags `v1.26.0`/`v1.26.1`/`v1.26.2`

## Foundations running alongside Phase 1

- [~] Permission engine — per-run flags + route guards exist, no central policy store ([Phase 10](#))
- [~] Observability — `request_log` + SSE, no structured logs/correlation IDs
- [x] Spec documents (`ARCHITECTURE`, `IMPLEMENTATION-SPECIFICATION`, `MASTER-ROADMAP`, `TESTING-STRATEGY`, `KNOWN-ISSUES`, `FEATURE-STATUS`, this file)
- [ ] Frontend test runner
- [ ] End-to-end harness (headless launch + mock router)

## Next up

1. Close Phase 1 sign-off (macOS + install smokes).
2. Phase 2 router registry extraction from `router.rs`.
3. Phase 3 capability table (needed before Thinking).
