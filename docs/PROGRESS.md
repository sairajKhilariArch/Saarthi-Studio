# Progress checklist

Persistent record of what is done, what is in flight and what is next. Update on
every shipped commit. Status values: `[x]` verified with evidence, `[~]`
partial, `[ ]` not started.

## Release line

| Version | Commit | Content | Release evidence |
|---|---|---|---|
| 1.26.2 | `75c4192` | LF restore + CR-safe version reads | workflow YAML was invalid, no run |
| 1.26.3 | `58cbea1` | Valid release workflow; first release with assets | 8 assets, macOS missing |
| 1.26.4 | `92b6817` | Single version parser, artifact gate, regression tests, release concurrency, spec docs | run `41869072420`: deb/windows/vscode/android success, Linux AppImage failed, macOS job never acquired |
| 1.26.5 | `3692edc` | AppImage AppStream attempt + macOS on an Intel runner | run `41882152731`: macOS exit 101 (missing aarch64 std), Linux failed `validate-tree` |
| 1.26.6 | `76b9fd1` | validate-clean AppStream identity (RDNS id, OARS rating), both Darwin targets | run `41884439627`: **all six jobs success, 9 assets** |

## Phase 1 — CI/CD and release reliability

- [x] Diagnose the CR-in-artifact-name failure (`41693462775`) to the CRLF manifest rewrite
- [x] Restore LF manifests and normalise the 12 files the bump script flipped
- [x] Make the workflow valid again (block-scalar version reads, YAML parse check)
- [x] Publish a release with real assets (`v1.26.3`, 8 assets + checksums)
- [x] One version parser for Linux/macOS/Windows (`scripts/ci/version.sh`) — `Read package version` green on all three in `41884439627`
- [x] Version validation: non-empty + semver shape (9 manifest cases in the regression suite)
- [x] Artifact-name validation before upload (`scripts/ci/verify-artifacts.sh`) — ran green on ubuntu, macOS and Windows runners
- [x] Checksum verification inside the artifact gate
- [x] Regression suite for LF/CRLF/empty/malformed versions + bad names/paths/checksums (`20 passed, 0 failed`)
- [x] Wire the regression suite into `desktop-ci.yml` (runs on every push)
- [x] Release `concurrency` so two tags cannot race the same asset name
- [x] AppStream metadata that validates: `appstreamcli validate-tree` clean, AppImage built
- [x] macOS job actually runs and attaches its tarball (`free-ai-suite_1.26.6_macos-universal.tar.gz`, both arches verified by `lipo` in-job)
- [x] Install smoke on the produced Windows artifact (local: release build `EXIT=0`, headless HTTP 200, brand string present, checksum verified)
- [ ] Install smoke on deb / AppImage / vsix / apk (needs the respective platform; only the Windows zip could be launched on this machine)
- [ ] Decide fate of dead tags `v1.26.0`/`v1.26.1`/`v1.26.2`

### Phase 1 verdict per previously diagnosed issue

| Issue | Verdict | Evidence |
|---|---|---|
| A. Rust Agent syntax error | **Not reproducible** | `cargo test --locked`: 127 passed, 0 failed; release build clean at 1.26.4/1.26.6. `agent.rs` compiles and its SQL paths are covered by tests. Left unchanged rather than rewritten blindly. |
| B. CR in Linux version | **Fixed** | Single parser; `sed` output stripped of `[:space:][:cntrl:]`; CRLF manifest is a test case. |
| C. macOS version regex | **Fixed** | macOS uses the same `scripts/ci/version.sh`; step green in `41884439627`. |
| D. Artifact upload failure | **Fixed** | Gate rejects control characters/spaces and verifies checksums before every upload; upload steps green on all three OSes. |
| E. AppImage AppStream warning | **Fixed, and it was fatal** | appimagetool runs `appstreamcli validate-tree` and aborts on non-zero. `validate-tree` fails on *warnings*, so two iterations were needed: the non-RDNS id and the missing OARS rating. Now clean; AppImage attached. |
| F. macOS job cancellation | **Diagnosed and fixed** | GitHub's own annotation: "The job was not acquired by Runner of type hosted even after multiple attempts", plus macOS-arm64 capacity notices. `macos-latest` is arm64; the job moved to `macos-15-intel` and then failed for a real reason (only the x86_64 std installed, exit 101), which was fixed by installing both Darwin targets. |

## Foundations running alongside Phase 1

- [~] Permission engine — per-run flags + route guards exist, no central policy store (Phase 10)
- [~] Observability — `request_log` + SSE, no structured logs/correlation IDs
- [x] Spec documents (`ARCHITECTURE`, `IMPLEMENTATION-SPECIFICATION`, `MASTER-ROADMAP`, `TESTING-STRATEGY`, `KNOWN-ISSUES`, `FEATURE-STATUS`, this file)
- [ ] Frontend test runner
- [ ] End-to-end harness (headless launch + mock router)

## Phase 2 — Router Manager

Slice 1 (`app/src/router_registry.rs`, this commit): the registry and platform
capability layer, with 10 unit tests. It reuses `router.rs` for lifecycle and
points at its facts rather than re-deriving them.

- [x] Router registry with the §5.1 fields (id, name, description, OS, arch, install methods, config location, health probe, docs)
- [x] Architecture normalisation for real release-asset spellings (`aarch64`/`amd64`/`x64`)
- [x] Platform gating: Android never claims a local install; remote stays available
- [x] Only routers with an existing lifecycle in the crate are listed (no aspirational entries)
- [x] Tests: unique slug ids, required fields, health probe present, HOME-relative config, Android exclusion, unknown arch rejection, stable JSON keys
- [x] Live probes per router (installed / version / runtime / healthy / ownership) reusing `router::status`, `router::omniroute_status` and `local::fetch_version`
- [x] `healthy` is "the health probe answered", never "the process exists" (unit-tested both ways)
- [x] `/api/routers` route returning capabilities + measured state + host platform (smoked: HTTP 200, three routers, real Windows host values)
- [ ] Probe latency: the three probes run sequentially (~6.5 s on this host); run them concurrently and cache briefly
- [ ] UI: the router list does not render this yet — the endpoint exists, the panel does not
- [ ] Router UI: list, health, start/stop/restart, logs, connection tester
- [ ] Install workflow with approval, checksum verification and recorded install info
- [ ] Auto-start on launch that never stops externally owned routers

## Next up

1. Phase 2 slice 2 — live probes (`installed`/`version`/`healthy`/`owned`) and the
   `/api/routers` route on top of the registry.
2. Phase 3 — capability table per model (needed before Thinking in Phase 4).
3. Phase 10 foundations (permission store) before MCP tool exposure in Phase 7.
