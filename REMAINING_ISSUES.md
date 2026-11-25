# Sarthi Studio — Remaining Issues

**Date:** 2026-11-25  
**Release reviewed:** 3.88.3  
**Open P0:** none identified in the tested Linux surface.  
**Open unmitigated P1 from the audits:** none.  
**Remaining P2/P3/P4:** listed below with the reason each was not changed in this pass.

## 1. External / blocked

### R-01 — Windows and macOS packages are unsigned; checksums share the release origin (SEC-04, P2)

**State:** open, externally blocked.

**Why:** Code signing needs a Windows code-signing certificate and Apple Developer ID/notarization credentials, plus Windows/macOS build hosts. None is available here.

**Current mitigation:** Update downloads require HTTPS, a published SHA-256, and re-check the staged file’s hash. Archive traversal is refused. A release-asset compromise can still replace both the asset and `SHA256SUMS.txt`.

**Next step:** obtain certificates, sign/notarize the 11-asset set, and add a signed-installer install/upgrade/uninstall test on each host. Until then, describe builds as unsigned, not “fully signed”.

### R-02 — Windows/macOS/Android runtime not executed (SEC-08, process blocker)

**State:** open, environment blocker.

**Why:** This host is Linux x86_64 with no Windows/macOS host, no WiX, no JDK/Android SDK/Gradle, and no device.

**What did run:** Linux debug/release, macOS bundle **fixture** tests (13/13), release contract/asset gates, and source/CI inspection for the other platforms.

**Next step:** run and record the existing CI jobs on real hosts, including the MSI/`.app`/APK/AAB smoke tests.

### R-03 — Tagged 11-asset release was not executed

**State:** open, not requested and not authorized.

The contract, naming, checksums, artifact and bundle gates pass offline. The actual GitHub release workflow and signed artifact production were not run.

## 2. Accepted dev-only advisory

### R-04 — 3 moderate npm advisories in the VS Code packaging toolchain (SEC-06, P3)

**State:** accepted; production dependency surface is clean.

```
vscode-extension: npm audit --omit=dev  → 0 vulnerabilities
vscode-extension: npm audit             → 3 moderate (glob, minimatch via @vscode/vsce)
website:         npm audit              → 0 vulnerabilities
```

**Why not fixed:** `@vscode/vsce` is a development packaging tool, not shipped in the extension runtime. The audit’s suggested fix is a breaking major upgrade. An upgrade to 4.1.1 was tested during remediation: it still left **2 moderate** advisories, required Node >= 22, and reformatted `package.json`; the change was reverted. Installing it did not meet the “no blind upgrade” rule and did not clear the finding.

**Next step:** upgrade when the extension’s supported Node baseline can move to 22 and the packaging output has been re-smoked.

## 3. Rust advisory warnings (no vulnerability)

`cargo audit` exits **0** with **0 vulnerabilities** and five warnings:

| Crate | ID | Type | Why it remains |
|---|---|---|---|
| `chrono` 0.4.45 | RUSTSEC-2026-0543 | unmaintained | no fixed upgrade published |
| `paste` 1.0.15 | RUSTSEC-2024-0436 | unmaintained | no fixed upgrade |
| `proc-macro-error` 1.0.4 | RUSTSEC-2024-0370 | unmaintained | no fixed upgrade |
| `rustls-pemfile` 2.2.0 | RUSTSEC-2025-0134 | unmaintained | no fixed upgrade |
| `glib` 0.18.5 | RUSTSEC-2024-0429 | unsound (`VariantStrIter`) | Tauri 2.12.5/GTK 0.18 line; fix requires a newer GTK/Tauri chain |

**Assessment:** these are not known exploitable vulnerabilities in Sarthi Studio’s usage, but the `glib` unsoundness is real and should be removed when the Tauri/GTK dependency line can be upgraded. Upgrading Tauri without running Windows/macOS GUI tests was judged riskier than documenting it. No audit entry was ignored or suppressed.

## 4. MCP credential residuals

### R-05 — VS Code embedded server stores its OAuth session in a 0600 file

**State:** accepted, low.

The desktop Rust MCP client and provider keys use the OS credential store. The separate VS Code extension’s Node server stores OAuth material in a permission-restricted file because its standalone server has no VS Code `SecretStorage` bridge. Its URL policy already refuses TLS downgrade off loopback; its tests pass (45/45).

**Next step:** bridge to VS Code `SecretStorage` when the server is refactored to run inside the extension host, or document the 0600 file explicitly for users.

### R-06 — Real OS keychain was not exercised

**State:** verification gap, low.

The new header migration test runs with the project’s hermetic `FREE_AI_STUDIO_KEYCHAIN=mock` backend, so it proves the read/write/migration logic without touching a developer vault. A live Linux Secret Service (and macOS Keychain / Windows Credential Manager) round trip was not run.

**Next step:** a one-time manual smoke on each desktop OS with a throwaway service name (`FREE_AI_STUDIO_KEYCHAIN_SERVICE`), then remove the entry.

## 5. Product and verification gaps

### R-07 — Per-message edit/delete/regenerate is not implemented

**State:** product scope gap, P3.

No route, UI action, schema or prior implementation was found. The app supports editing a draft, deleting a whole conversation, resending text and retrying an agent run. Implementing per-message mutation requires an agreed history/prompt policy; it was not invented in a security patch.

### R-08 — No browser layout/pixel/accessibility regression suite

**State:** verification gap, P3.

The frontend harness is browser-free and covers 115 behaviors. The audit’s live smoke covered load/theme/navigation/chat only. No screenshots, responsive matrix, keyboard-only pass, contrast check or screen-reader pass is part of CI.

**Next step:** add Playwright (or equivalent) at desktop and narrow widths, plus axe, only if the project accepts the added browser-test dependency and CI time.

### R-09 — Stop-generation was not timed against a slow stream

**State:** **resolved in 3.88.3.**

The mock now emits a real event stream and a `mock-slow-stream` model sends one delta, then holds the connection open. The e2e aborts the fetch after that delta and observes the stored partial reply exactly once; the next turn on the same conversation succeeds. Running the same scenario twice gave the same result. This also closed the related gap that all earlier e2e “streaming” checks exercised only the one-shot JSON fallback.

### R-10 — No real third-party provider, search, speech, MCP, language-server or debugger peer

**State:** environment limitation.

Tests use the project’s real HTTP mock provider and local MCP/stdio fixtures. This verifies transport, persistence, approval and failure machinery, not a vendor’s current API behavior. No mock was reported as a live integration.

### R-11 — No long-session soak/leak profile

**State:** measured for a short run; no multi-hour soak.

Release binary on a scratch home with the mock provider: 61 ms to `/api/ready`, 0.0% CPU over five idle seconds, 88,908 KB RSS after boot, 91,796 KB after 100 completed turns (100/100 answered, average 48 ms per turn), 33 threads. The +2.9 MB is consistent with allocator/SQLite page-cache warm-up; it is not proof of leak-freedom over hours. Context packing, request-log paging and panel caps remain unit-tested, not load-tested.

### R-12 — Database upgrade from every historical release not replayed

**State:** partial coverage.

The suite covers a pre-projects migration, a newer-schema refusal, backup-before-migrate and failed-upgrade rollback. It does not replay every historical release database.

### R-13 — Linux package payload only; no system install or dependency metadata

**State:** partially verified, P3.

A 3.88.3 `.deb` was assembled with project-local `cargo-deb` and its unpacked payload passed 14/14 checks (binary, UI, bundled skills, `--version`, headless boot, version over HTTP, conversation/setting creation, payload replacement with preserved data).

**Why it is not a shippable package here:** this Arch host has no `dpkg-shlibdeps`, so the control file's `Depends:` is empty and no system-wide install was performed. The GUI tarball/AppImage were not built. The Ubuntu CI job must produce the dependency-resolved `.deb`, and a disposable Debian/Ubuntu machine must install, upgrade and uninstall it before calling Linux packaging signed off.

## 6. Incidental note from verification

While checking the debug binary, `app/target/debug/free-ai-studio --version` was run. The binary has no version flag and started the app against the real home for a few seconds before the command was killed. It did not modify `config.json` (mtime unchanged from 2026-11-22) and made no user-directed request. The test and e2e suites otherwise use scratch homes. No destructive operation was performed. A `sqlite3` quick-check is not part of this report; the database is the user’s real file and was not altered intentionally.

## Closing assessment

The code-level security defects found by the audit and the RustSec scan are closed and regression-tested on Linux. Phase 3 closed the real-SSE, stop-generation, settings-persistence and startup-CLI verification gaps. The remaining list is dominated by external signing/platform validation, GUI/system package production, intentionally deferred toolchain upgrades, explicit product-scope gaps and long-soak/browser coverage — not by an unrepaired critical code path in the tested build.
