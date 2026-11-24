# Sarthi Studio — Application Audit Report

**Audit date:** 2026-11-24  
**Branch:** `audit/full-application-2026-11-24`  
**Baseline commit:** `55d1218` (`3.88.0: add broker-gated mcp.call to the extension sandbox`)  
**Audited release:** `3.88.1`; remediation implemented in `3.88.2`<br>
**Environment:** Linux x86_64, local desktop, headless and Tauri-capable build; no Windows, macOS, Android, signing or release-runner environment available.

## Executive summary

Sarthi Studio is a mature Rust application with a large automated test estate, real SQLite persistence, a strict local-only network posture, and unusually detailed documentation of its own limitations. The baseline was green: 704 Rust tests, 114 frontend tests, 34 real-binary end-to-end checks, 45 VS Code extension tests, and the release/version/metadata gates all passed.

The audit found **three reproducible defects**, all fixed and regression-tested:

1. **P1 — DNS-rebinding exposure on the core UI router.** The workspace, terminal, media and agent routers had a loopback `Host`/`Origin` guard, but the routes registered directly in `main.rs` (`/api/config`, chat, conversations, projects, workflows, extensions, update and uninstall) did not. A foreign `Host` received `200` and the full config JSON. The whole local router now has one outer guard, and the real-binary suite proves a foreign `Host`/`Origin` gets `403`.
2. **P2 — HTML injection through a recent-folder name.** A filesystem path was interpolated into `innerHTML` in the project menu. It is now built with DOM text nodes, and a frontend regression test uses a hostile folder name.
3. **P3 — Streaming chat omitted memory/retrieval metadata unless a skill auto-matched.** `memory_used` and `retrieved` were nested inside the `auto_skills` condition. They are now independent, and an end-to-end test proves a recalled memory appears in a stream's done metadata.

Final evidence after the 3.88.2 remediation: **709 Rust tests**, **115 frontend tests**, **53 real-binary end-to-end checks against both debug and optimized release binaries**, **45 VS Code extension tests**, 26 version checks, 9 Linux metadata checks, all offline release/install gates, and a live browser smoke with no console errors.

The remediation pass closed two further code-level security gaps and the Rust dependency advisories that the audit’s missing `cargo-audit` step could not see: remote MCP is now HTTPS-only off loopback at save, connect, OAuth and SSE boundaries; saved MCP headers live in the OS credential store with startup migration; and the locked Rust dependencies were upgraded until `cargo audit` reported **0 vulnerabilities**. These are recorded in `SECURITY_REMEDIATION_REPORT.md` and `VERIFIED_FIXES.md`.

The application is a solid release candidate for the tested Linux surface. It is **not** a full cross-platform production sign-off: Windows/macOS/Android execution, signed installers, a green tagged release run and real third-party provider/MCP peers were unavailable.

## Architecture observed

| Layer | Implementation |
|---|---|
| Desktop/runtime | Rust binary (`app/`), Axum HTTP server on `127.0.0.1`, optional Tauri webview (`gui.rs`) using the system webview. |
| Local UI | Hand-written HTML/CSS/ES modules in `app/static/`; no frontend build step or package manager. |
| Persistence | SQLite at `~/.local/share/SAARTHI-STUDIO/studio.db` (implementation path: `history.sqlite3` under the app data directory), 22 tables, WAL/locking and schema migrations. |
| Configuration | `~/.config/SAARTHI-STUDIO/config.json`; secrets prefer OS keychain, with a permission-restricted config fallback. |
| AI transport | OpenAI-compatible router, OpenAI Responses platform path, local Ollama mode, deep search, streaming SSE, provider fallback and model scopes. |
| Agent runtime | Persisted runs/steps, approvals, permission gates, worktree isolation, checkpoints, cancellation, resume, crews/workflows. |
| Extensions | Wasm sandbox (wasmi, fuel/memory caps), host functions for workspace/data/network/terminal/MCP, audit and allow rules. |
| Remote host API | Separate TLS listener with a 256-bit bearer token stored as a SHA-256 verifier, constant-time comparison, body limits and error redaction. |
| Editor/tooling | File explorer, Monaco-free textarea editor with highlight overlay, Git, runner/watch, LSP diagnostics, DAP debugger, terminal PTY. |
| Secondary clients | VS Code extension with its own Node server; Android React Native client over the Host API; Astro website. |
| Packaging/CI | deb/AppImage/tarball, Windows zip/MSI, macOS `.app`/tarball, VSIX, APK/AAB; 11-asset release contract; 6 GitHub workflows. |

Size at audit time (source only): approximately 68,600 Rust LOC across 56 modules, 14,600 frontend JS LOC, 3,345 HTML/CSS LOC, 199 `.route(...)` registrations and 22 tables.

## Feature inventory and condition

| Area | Implementation | Condition at end of audit | Evidence |
|---|---|---|---|
| Conversations | `main.rs` routes + SQLite | Working | create/list/messages/delete, cascade verified in e2e; restart persistence |
| Chat (non-stream) | `chat`, `prepare_chat`, router/client | Working | mock-provider turn stored and returned; 709 Rust tests |
| Chat streaming | `/api/chat/stream`, SSE frames, `ReplyRecorder` | Working | e2e delta + done, one stored reply; UI stream turn against live release |
| Stop generation | browser `AbortController` + server recorder Drop | Code-path verified, not timed against a slow stream | UI code inspected; recorder saves partial replies by construction |
| Message edit/delete/regenerate | none found | **Not implemented** in current product | no route, UI action or documented claim; recorded as a scope gap, not a regression |
| Providers/models | `providers.rs`, model catalog, fallback chain | Working with configured endpoint | CRUD/list/redaction in e2e; URL scheme/loopback validation in code/tests |
| Provider connectivity | `/api/test`, `/api/models`, health probe | Working against mock; real providers unavailable | mock-router e2e; real keys/network not available |
| Agent runs | `agent.rs`, `/api/runs`, approvals | Working | e2e run parks at approval, writes file, verifies, survives restart |
| Agent cancellation/interruption | control map, `mark_interrupted` | Working | kill/restart e2e marks run interrupted and keeps step log |
| Agent queue/concurrency | slot admission, max 4 | Covered by Rust tests | 709-test suite |
| Crew templates | `crew-templates.json` + routes | Working (unit/API) | route registered; Rust tests |
| Skills | `skills.rs`, install/import/refresh/state/remove | Working | catalogue e2e; Git import/refresh tests in Rust/frontend |
| Wasm extensions | `extensions.rs`, `extension_run.rs` | Working, sandboxed | 709 Rust tests, real HTTP MCP fixture documented |
| Extension panels/presets | manifest panels/providers | Working | routes and CSP/sandbox tests; no real third-party extension peer |
| Workspace files | `workspace.rs` | Working | create/save/read/rename/delete e2e; traversal refusal |
| Workspace containment | `resolve_inside`, canonicalization | Working | e2e `../outside.txt` refusal; symlink escape Rust tests |
| Code index/search | `index.rs`, embeddings | Working (unit/API) | capability reported; tests; no live model embedding provider |
| Git | `git.rs` | Working (unit/API); not driven end-to-end in this pass | extensive Rust tests; docs list remaining Git gaps |
| Runner/watch | `runner.rs`, `watch.rs` | Working (unit/API) | tests; not live-run in e2e |
| LSP diagnostics | `lsp.rs` | Working with installed server only | code/tests; no language server installed in this environment |
| DAP debugger | `dap.rs` | Working with debugpy only (documented) | code/tests; no lldb-dlv peer |
| Terminal | `terminal.rs`, portable-pty | Working | **new e2e**: real PTY command wrote a file |
| Memory | `memory.rs` | Working | **new e2e**: create/list/recall/delete; stream metadata |
| Projects | `main.rs` project routes | Working | **new e2e**: create/rename/list/delete |
| Workflows | `workflow.rs` | Working | **new e2e**: save/export/delete; graph engine Rust tests |
| MCP | `mcp.rs`, `mcp_oauth.rs` | Working with configured peer; OAuth uses 0600 file | tests + local fixtures; no live third-party server |
| Deep search | `deepsearch.rs` | Code/API working; external search provider unavailable | capability and tests; no key |
| Media/transcribe | `media.rs`, `/api/transcribe` | Code/API working; no live speech provider | size/MIME/path validation tests |
| Settings/permissions | `permissions.rs`, config routes | Working | e2e config redaction; Rust permission tests |
| Updates | `update.rs`, `installer.rs` | Working, checksum-verified; no signature verification | Rust tests; network release check not run |
| Uninstall | `uninstall.rs` | Working on POSIX; Windows behavior documented | Rust tests; no destructive live uninstall |
| Usage/cost | `cost.rs`, request log | Working | e2e turns recorded; Rust tests |
| Themes/layout/shortcuts | static UI + harness | Working at smoke level | live theme toggle; 115 frontend tests |
| Notifications/events | `events.rs`, EventSource | Working | UI EventSource 200, no console errors |
| Host API | `host_api.rs`, `host_auth.rs`, `host_transport.rs` | Working; not exercised over a real TLS device | Rust tests; Android device unavailable |
| VS Code extension | TypeScript/Node server | Working | **45/45 tests** |
| Website | Astro static site | Working | **37 pages built** |
| Android | React Native client | **Blocked** | no JDK/SDK/Gradle/device |
| Windows/macOS desktop | Tauri/packaging | **Not executed here** | Linux-only audit host |

## Defects, missing functionality and risks

### Fixed in 3.88.1

- **P1 core router outside the origin guard** — see `SECURITY_AUDIT_REPORT.md` SEC-01 and `BUG_FIX_LOG.md` BUG-01.
- **P2 recent-folder HTML injection** — SEC-02 / BUG-02.
- **P3 stream metadata coupling** — BUG-03.

### Fixed in 3.88.2 remediation

- **P2 remote MCP cleartext downgrade** — HTTPS is required off loopback at save, connect, legacy-SSE and every OAuth endpoint; old `mcp.json` files are refused before a header/token is sent. See SEC-03 / BUG-04.
- **P2 MCP header plaintext at rest** — saved header values move to the OS credential store, with startup migration and the 0600 file fallback if the store is unavailable. See SEC-05 / BUG-05.
- **P1 supply-chain advisories** — `zip`, `rusqlite`/`libsqlite3-sys`, `tokio`, `url` and `cookie` were upgraded; `cargo audit` now reports 0 vulnerabilities. See DEP-01.

### Missing or incomplete, not repaired in this pass

These are product/scope gaps or externally blocked areas, not silent failures:

- **Message-level edit, delete and regenerate** are not present. The product supports editing a draft, deleting a whole conversation, resending the last user text, and retrying a failed run; it does not expose per-message mutation. Implementing it would be a new feature and requires an agreed history/prompt policy, so it is reported rather than invented.
- **Remote MCP cleartext and saved-header storage are fixed** (3.88.2). The VS Code embedded server still stores its OAuth session in a 0600 file because its standalone Node process has no VS Code `SecretStorage` bridge.
- **No code signature/notarization** for Windows/macOS packages. Checksums verify downloads but are served from the same release origin.
- **No browser layout/pixel regression suite.** The frontend harness is browser-free; the live smoke covered load, theme, navigation and chat, not every resize/layout combination.
- **Android, Windows and macOS were not executed.** Their CI jobs exist, but this audit host cannot run them.
- **No real third-party provider, search, MCP, language-server, debugger or speech peer.** Mock/local fixtures were used where the external service was unavailable; no fake success was substituted for a real integration.
- **Five RustSec warnings remain**, but no vulnerabilities: chrono/paste/proc-macro-error/rustls-pemfile are unmaintained, and glib 0.18.5 is unsound with no safe in-line upgrade outside a Tauri/GTK line change.

## Performance observations

- Optimized release binary: **20 MB**; debug binary: **228 MB** (3.88.2 build).
- Headless cold start to `/api/health` ready: **174 ms** for the 3.88.1 release binary on this host.
- Resident memory: **87.5 MiB RSS** at idle immediately after boot, around 88–94 MiB after a live UI session.
- Static UI entry document: **102,655 bytes**; local API calls complete in the low milliseconds in the smoke.
- No long-session soak, leak profile, GPU measurement or large-conversation stress test was performed. The existing context packer, request log paging and capped panels are covered by code/tests but were not load-tested here.

## Build and release condition

| Gate | Result |
|---|---|
| `cargo build --locked` (debug) | Pass |
| `cargo build --release --locked` | Pass, 20 MB binary |
| `cargo test --locked` (with lld) | 709 passed, 0 failed, 1 ignored |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass |
| `cargo fmt --check` | Pass |
| Frontend suite | 115/115 |
| Real-binary e2e, debug | 53/53 |
| Real-binary e2e, release | 53/53 |
| VS Code extension | 45/45 |
| Website build | 37 pages |
| Version/metadata | 26/26, 9/9 |
| Offline release/artifacts/bundle/install gates | Pass (27, 12, 13, install-deps; MSI skipped without WiX) |
| Release contract | 11 asset names defined and gated; actual platform packages not built here |
| Rust advisory scan | 0 vulnerabilities; 5 unmaintained/unsound warnings |
| Tagged end-to-end release run | **Not run** |
| Signed installers / notarization | **Not available** |

The default `cargo test` without the project's lld linker setting failed in this environment because the local linker configuration selected `mold` without a usable `ld`; with the documented lld setting the suite passed. This is a host/toolchain issue, not a source failure.

## Platform status

| Platform | Status | Detail |
|---|---|---|
| Linux x86_64 | **Tested** | Debug/release binaries, headless, e2e, local UI, PTY, packaging metadata/gates |
| Windows x86_64 | **Blocked** | No Windows host/WiX; MSI test skipped as designed; source has Windows-specific paths and CI job |
| macOS arm64/Intel | **Blocked** | No macOS host; bundler fixture tests pass; unsigned `.app` only |
| Android | **Blocked** | No Java, Android SDK, Gradle wrapper or device |
| Web (local UI) | **Tested** | Chromium load, no console errors, theme/navigation/chat |
| Public website | **Built** | Astro 37 pages; not deployed in this audit |

## Honest limitations

- A passing mock-provider test proves the application's request/response, persistence and approval machinery, not a real provider's current API behavior.
- A code inspection of stop-generation is not a timed abort test. The fast mock completes before a stop can be observed.
- The live browser smoke does not replace a full cross-browser, responsive or accessibility audit.
- No destructive operations were run against real user data. All tests used scratch homes and databases.
- The reports describe the repository at this commit; historical release assets and external services were not re-verified.

## Final assessment

The tested Linux application is materially more secure and better covered than at baseline. The core request boundary is now coherent, the two discovered injection/exposure paths are closed, and the end-to-end suite exercises real persistence, files, PTY, approvals and restarts rather than mocks alone.

**Recommendation:** ship `3.88.2` as a Linux patch release after the normal CI jobs and a tagged build are green. Do not describe it as a fully signed cross-platform release until Windows/macOS/Android execution and signed artifacts are actually observed.
