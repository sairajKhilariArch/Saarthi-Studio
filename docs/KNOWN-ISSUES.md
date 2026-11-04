# Known issues

Ordered by impact. `Open` = unresolved, `Fixed` = verified in a shipped commit,
`Mitigated` = guarded but the underlying cause remains.

| # | Issue | State | Detail / next step |
|---|---|---|---|
| 1 | macOS release job never produces an artifact | **Fixed** (1.26.6) | Root cause was runner availability, not code: GitHub annotated run `41869072420` with "The job was not acquired by Runner of type hosted even after multiple attempts" and macOS-arm64 capacity notices. `macos-latest` is arm64; the job now uses `macos-15-intel` and installs both Darwin targets (the Intel runner has no aarch64 std — that was the exit 101 in `41882152731`). `free-ai-suite_1.26.6_macos-universal.tar.gz` is attached to the release. The job is still unsigned, so Gatekeeper needs right-click → Open on first launch. |
| 2 | Tags `v1.26.0`, `v1.26.1`, `v1.26.2` published no assets | **Open (historical)** | v1.26.0/1 failed on artifact upload (CR in filename); v1.26.2 never triggered (invalid workflow YAML). Leave documented or delete the tags after a green release. |
| 3 | CRLF checkouts can corrupt version reads | **Fixed** (1.26.4) | One parser (`scripts/ci/version.sh`) strips CR/LF/tab/space and validates semver; regression suite `scripts/ci/test-version.sh` runs on every push. |
| 4 | Invalid artifact names reach upload | **Fixed** (1.26.4) | `scripts/ci/verify-artifacts.sh` rejects control characters/spaces and verifies checksums before upload. |
| 5 | AppImage missing AppStream metadata (warning) | **Fixed** (1.26.6) | The warning was actually fatal: appimagetool runs `appstreamcli validate-tree`, which fails on warnings too. Needs a real TLD in the component ID, matching metainfo file name and desktop-id, and an OARS rating. `io.github.sairajkhilariarch.SaarthiStudio.{desktop,appdata.xml}` now validate clean; the deb installs the metainfo as well. |
| 6 | Overlapping release runs can race an asset name | **Fixed** (1.26.4) | `concurrency: desktop-release-${{ github.ref }}`, `cancel-in-progress: false`. |
| 7 | Mixed line endings across the tree (125 files CRLF) | **Mitigated** | 12 files normalised in `75c4192`; the rest remain CRLF in the working tree. Editors that normalise on save reintroduce the class of bug. |
| 8 | `AGENTS.md` references workflows that are not on `main` (`build-artifacts.yml`, `desktop-runtime-smoke.yml`) | Open (docs) | They exist on other branches; GitHub lists them repo-wide. Either land them on `main` or fix the doc. |
| 9 | `website` branch diverged (35 behind / 14 ahead of `main`) | Open | Version line differs (1.22.x vs 1.26.x); merging will need a deliberate decision. |
| 10 | Formatting gate is partial | Open | `rust-host-api-ci.yml` checks only four host modules with `rustfmt --check`; the rest of `app/src` is ungated. Add `cargo fmt --check` once the tree is formatted. |
| 11 | Clippy runs only on ubuntu | Open | Windows/macOS clippy differences go unnoticed until release. |
| 12 | No frontend tests | Open | `app/static/` has no runner; see `docs/TESTING-STRATEGY.md`. |
| 13 | No end-to-end test | Open | Only ad-hoc local smokes so far. |
| 14 | Secrets stored in `config.json`, not an OS keychain | Open | File mode restricted; keychain integration is Phase 10 work. |
| 15 | MCP, Thinking/reasoning, codebase indexing, multi-agent orchestration, workflow builder, plugin ecosystem | **Missing** | Tracked in `docs/FEATURE-STATUS.md`; not defects, absent phases. |
| 16 | No Windows installer / macOS bundle / auto-update | Open | Phase 18. Portable zip and unsigned tarball only. |
