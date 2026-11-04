# Known issues

Ordered by impact. `Open` = unresolved, `Fixed` = verified in a shipped commit,
`Mitigated` = guarded but the underlying cause remains.

| # | Issue | State | Detail / next step |
|---|---|---|---|
| 1 | macOS release job never produces an artifact | **Open** | `queued` ~35 min in run `41861357127`, `cancelled` in earlier runs; no macOS asset exists in any release. Root cause is runner availability, not code. Options: keep waiting, add `macos-14` fallback, or build macOS locally. |
| 2 | Tags `v1.26.0`, `v1.26.1`, `v1.26.2` published no assets | **Open (historical)** | v1.26.0/1 failed on artifact upload (CR in filename); v1.26.2 never triggered (invalid workflow YAML). Leave documented or delete the tags after a green release. |
| 3 | CRLF checkouts can corrupt version reads | **Fixed** (1.26.4) | One parser (`scripts/ci/version.sh`) strips CR/LF/tab/space and validates semver; regression suite `scripts/ci/test-version.sh` runs on every push. |
| 4 | Invalid artifact names reach upload | **Fixed** (1.26.4) | `scripts/ci/verify-artifacts.sh` rejects control characters/spaces and verifies checksums before upload. |
| 5 | AppImage missing AppStream metadata (warning) | **Fixed** (1.26.4) | `app/packaging/free-ai-suite.appdata.xml` shipped under `usr/share/metainfo`. |
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
