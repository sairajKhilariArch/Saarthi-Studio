# Sarthi Studio — Final Test Results

**Date:** 2026-11-25  
**Release:** 3.88.3  
**Branch:** `remediation/security-2026-11-24` (HEAD unchanged before and after the final run)  
**Host:** Linux x86_64, local desktop; no Windows, macOS, Android, signing or release runner.

## Verdict

All runnable checks pass on the tested Linux surface:

| Suite | Actual result |
|---|---|
| Rust unit/integration (`cargo test --locked`) | **712 passed** (709 suite + 3 built-binary CLI), 0 failed, 1 ignored |
| Rust clippy (`--all-targets -- -D warnings`) | **Pass** |
| rustfmt (`--check`) | **Pass** |
| Real-binary e2e — debug | **61 passed, 0 failed** |
| Real-binary e2e — optimized release | **61 passed, 0 failed** |
| Linux `.deb` payload boot/replace smoke | **14 passed, 0 failed** |
| Startup latency (release, scratch home) | **61 ms** to `/api/ready` |
| 100-turn stability run | **100/100 answered**; RSS +2,888 KB after warm-up; idle CPU 0.0% |
| Frontend | **115 passed, 0 failed** |
| VS Code extension | **45 passed, 0 failed** |
| Website build | **37 pages** |
| Version consistency | **26 passed, 0 failed** |
| Linux metadata | **9 passed, 0 failed** |
| Release contract gate | **27 passed, 0 failed** |
| Artifact verification | **12 passed, 0 failed** |
| macOS bundle fixture | **13 passed, 0 failed** |
| Debug build (`--locked`) | **Pass** |
| Release build (`--locked`) | **Pass**, 20 MB binary |
| Rust advisory scan (`cargo audit`) | **0 vulnerabilities**; 5 warnings; exit 0 |
| npm production audit (VS Code) | **0 vulnerabilities** |
| npm full audit (VS Code) | 3 moderate, dev-only |
| npm full audit (website) | **0 vulnerabilities** |

The ignored Rust test is `installer::tests::a_real_release_asset_verifies_against_its_published_hash`, explicitly marked `network: fetches a real release asset`. It was not counted as a pass.

## Exact commands and evidence

### Rust suite

```bash
export CARGO_ENCODED_RUSTFLAGS=$'-C\x1flink-arg=-fuse-ld=lld'
cd app
cargo test --locked
```

```
test result: ok. 709 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # tests/cli.rs
```

That total includes the new redirect regression: a local fixture answers `302 Location: http://example.com`, and the MCP client refuses the hop before contacting the external host.

The lld setting is the project’s documented workaround for this host’s linker configuration; it does not change application code.

### Static analysis

```bash
cd app
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Both exited 0 with no diagnostics.

### Dependency scan

```bash
cd app
cargo audit
```

```
Scanning Cargo.lock for vulnerabilities (523 crate dependencies)
warning: 5 allowed warnings found
```

Exit: **0**. Warnings: `chrono`, `paste`, `proc-macro-error`, `rustls-pemfile` (unmaintained) and `glib 0.18.5` (unsound). No vulnerability entry.

The scan was run after the final `cookie 0.18.3` update, so it covers the exact committed lock.

### Builds

```bash
cargo build --locked --manifest-path app/Cargo.toml
cargo build --release --locked --manifest-path app/Cargo.toml
```

- Debug: pass, `app/target/debug/free-ai-studio`, 228 MB.
- Release: pass, `app/target/release/free-ai-studio`, 20 MB, built in **2m17s** (final incremental build).
- The release build used `zip 9.8.2`, `rusqlite 0.43.0`, `libsqlite3-sys 0.40.0`, `tokio 1.54.1`, `url 2.5.9` and `cookie 0.18.3` from the final lock.

### Real-binary e2e

```bash
E2E_NO_BUILD=1 bash scripts/ci/test-e2e.sh
E2E_BINARY=$PWD/app/target/release/free-ai-studio E2E_NO_BUILD=1 bash scripts/ci/test-e2e.sh
```

Both: **61 passed, 0 failed**.

Each run uses a temporary home/config/data directory, a real SQLite database, a real filesystem workspace, a real PTY, a real process kill/restart, and a local OpenAI-compatible mock provider. It does **not** contact a real AI provider.

Coverage includes:

- a real `text/event-stream` upstream: six deltas, one done frame, one stored reply;
- a timed abort against a slow stream: the partial reply is stored exactly once and the studio answers the next turn;
- a setting saved through `/api/config`, then read back from disk after a restart;
- `--version` printing and exiting without creating a home;
- boot, readiness, database/data-directory requirements;
- foreign `Host` on `/api/config` → 403; foreign `Origin` on `/api/capabilities` → 403;
- conversation create/list/messages/delete and restart persistence;
- non-stream and streamed chat, one stored reply per stream, token budget;
- config key redaction (`has_api_key`, never the key);
- project, memory and workflow CRUD;
- skill catalogue;
- **remote cleartext MCP save → 400**;
- **loopback HTTP MCP save/remove → 200**;
- workspace open, file create/save/read/rename/delete, traversal refusal;
- interactive terminal command;
- agent run parked at write approval, approved, verified, file present;
- provider outage, failed run record, studio still answers;
- kill mid-run, restart, interrupted run and step log;
- corrupt database refusal, no listener, restore and ready;
- unwritable data directory and recovery.

Expected stderr appears once per run:

```
.write-probe: Permission denied
```

This is the readiness test probing an unwritable directory; the test passes because the permission is refused.

### Frontend

```bash
bash scripts/ci/test-frontend.sh
```

```
tests 115
pass 115
fail 0
```

### VS Code extension

```bash
cd vscode-extension && npm test
```

```
tests 45
pass 45
fail 0
```

The test output includes two intentional `DRIVE_CATCH` lines from the agent failure-path fixture; the suite reports both as passing behavior.

### Website

```bash
cd website && npm run build
```

Astro built **37 pages** with exit 0, including the sitemap.

### Release/version/metadata gates

```bash
bash scripts/ci/test-version.sh
bash scripts/ci/test-linux-metadata.sh
bash scripts/ci/test-release.sh
bash scripts/ci/test-artifacts.sh
bash scripts/ci/test-bundle.sh

# Package payload (project-local cargo-deb; no root)
(cd app && cargo build --release --locked --no-default-features)
(cd app && ../.scratch/tools/bin/cargo-deb --locked --no-build \
  --output ../.scratch/phase3/free-ai-studio_3.88.3-1_amd64.deb)
bash .scratch/phase3/smoke-deb.sh
```

```
version:  26 passed, 0 failed
metadata:  9 passed, 0 failed
release:  27 passed, 0 failed
artifacts: 12 passed, 0 failed
bundle:   13 passed, 0 failed
```

The macOS bundle fixture names the 3.88.2 archive correctly; this is a fixture, not a macOS execution.

### npm audits

```bash
cd vscode-extension && npm audit --omit=dev   # 0 vulnerabilities
cd vscode-extension && npm audit              # 3 moderate (dev-only)
cd website && npm audit                       # 0 vulnerabilities
```

## What was not run / not verified

| Item | Status | Reason |
|---|---|---|
| Windows runtime, MSI install/upgrade/uninstall | **Not run** | no Windows host/WiX |
| macOS `.app` execution, Gatekeeper, notarization | **Not run** | no macOS host/certificate |
| Android APK/AAB on device | **Not run** | no JDK/SDK/Gradle/device |
| Tagged 11-asset GitHub release | **Not run** | not requested/authorized; CI runner absent |
| Real code signing | **Not run** | no certificates |
| Real OpenAI/Anthropic/OpenRouter/search/speech provider | **Not run** | no external keys/peers |
| Real third-party MCP/OAuth provider | **Not run** | no external peer; local fixtures used |
| Real Linux Secret Service/macOS/Windows keychain | **Not run** | tests use the hermetic mock backend |
| Browser layout, responsive, accessibility, screenshot matrix | **Not run** | no browser test suite in project; not requested |
| System-wide `.deb` install / GUI tarball / AppImage | **Not run** | no root, no `dpkg-deb`/`appimagetool`; payload booted from an unpacked tree only |
| Long-session soak/leak profile | **Not run** | no soak harness |
| `glib` 0.18.5 unsoundness removed | **Not fixed** | needs newer Tauri/GTK; no GUI platform matrix |

These are reported as unverified, not as passes.

## Command-line note (fixed in 3.88.3)

Earlier in remediation, `app/target/debug/free-ai-studio --version` started the app against the real home because the flag was ignored. That is fixed: `--version`/`--help` now print and exit before any path is resolved, `app/tests/cli.rs` runs the built binary, and the e2e suite asserts `--version` matches the manifest version. The package smoke also proves a version probe creates no home.

## Reproduction

From the repository root:

```bash
export CARGO_ENCODED_RUSTFLAGS=$'-C\x1flink-arg=-fuse-ld=lld'
(cd app && cargo test --locked)
(cd app && cargo clippy --locked --all-targets -- -D warnings)
(cd app && cargo fmt --check)
bash scripts/ci/test-frontend.sh
(cd vscode-extension && npm test)
(cd website && npm run build)
(cd app && cargo build --release --locked)
E2E_NO_BUILD=1 bash scripts/ci/test-e2e.sh
E2E_BINARY=$PWD/app/target/release/free-ai-studio E2E_NO_BUILD=1 bash scripts/ci/test-e2e.sh
(cd app && cargo audit)
```

Expected final state: all commands exit 0; `cargo audit` reports 0 vulnerabilities and five warnings.
