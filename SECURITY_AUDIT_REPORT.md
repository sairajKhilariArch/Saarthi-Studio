# Sarthi Studio — Security Audit Report

**Date:** 2026-11-24  
**Branch:** `audit/full-application-2026-11-24`  
**Version:** `3.88.1` audit, remediated in `3.88.2`<br>
**References used:** OWASP Top 10 (2021), OWASP ASVS 5.0 themes, desktop/local-web threat modeling, existing project ADRs and `docs/KNOWN-ISSUES.md`.

> **Remediation update (3.88.2, 2026-11-24):** SEC-03 and the header half of SEC-05 are fixed in source with regression tests; SEC-07 is closed by a real `cargo-audit` scan (0 vulnerabilities); the dependency advisories found by that scan are fixed. The original findings and reproduction text are retained below, with each section marked with its final disposition. See `SECURITY_REMEDIATION_REPORT.md`, `VERIFIED_FIXES.md` and `REMAINING_ISSUES.md`.

## Scope and threat model

The audit covered the local desktop/web UI server, the separate Android Host API, filesystem and terminal tools, the Wasm extension sandbox, MCP transports/OAuth, provider credentials, update/install paths, SQLite storage, the VS Code extension server, the Astro website and the release scripts.

Primary adversaries considered:

1. A **malicious website** the user visits, attempting DNS rebinding or cross-site requests against the loopback UI.
2. **Untrusted content** reaching the UI or an AI agent: model output, repository files, retrieved documents, MCP/extension output.
3. A **malicious or compromised extension/skill** attempting to escape its declared permissions.
4. A **network attacker** against a deliberately enabled Host API or remote MCP endpoint.
5. **Supply-chain** compromise of dependencies or release artifacts.
6. Accidental **secret leakage** into config responses, logs, prompts or exported files.

Out of scope: a hostile process running as the same OS user (it can read the app's files and token store by design), physical access, and a compromised operating system.

## Findings

| ID | Severity | Finding | Affected component | Status |
|---|---|---|---|---|
| SEC-01 | **High / P1** | Core UI routes were outside the loopback `Host`/`Origin` guard, enabling DNS-rebinding read/write access | `app/src/main.rs` core routes | **Fixed** |
| SEC-02 | **Medium / P2** | Recent-folder path interpolated into `innerHTML` | `app/static/app.js` project menu | **Fixed** |
| SEC-03 | **Medium / P2** | Remote MCP may use plain `http://` off loopback, exposing configured headers or bearer tokens in transit | `app/src/mcp.rs`, `mcp_oauth.rs` | **Fixed 3.88.2** |
| SEC-04 | **Medium** | Windows/macOS packages are unsigned; update checksums come from the same release origin | packaging/update | Accepted external gap |
| SEC-05 | **Medium / P2** | Saved MCP header values were plaintext in `mcp.json`; desktop OAuth tokens were already in the OS keychain | `app/src/mcp.rs` | **Fixed 3.88.2** (headers); VS Code server remains 0600 |
| SEC-06 | **Low** | 3 moderate npm advisories in the VS Code packaging toolchain (dev-only, fix requires a major upgrade) | `vscode-extension` dev dependencies | Accepted, no production exposure |
| SEC-07 | **Low** | No Rust advisory scanner (`cargo-audit`/`cargo-deny`) had been run | dependency supply chain | **Fixed**: 0 vulnerabilities, 5 warnings |
| SEC-08 | **Process** | Windows/macOS/Android runtime security and signed-installer behavior were not observable here | cross-platform | Blocked |

No SQL injection, command injection, arbitrary file read/write outside the workspace, authentication bypass of the Host API, or Wasm sandbox escape was reproduced.

## SEC-01 — Core routes outside the loopback origin guard

**Severity:** High (network-adjacent P1).  
**Component:** `app/src/main.rs`; the guard itself is `workspace::LocalOriginGuard` / `guard_local_origin`.

### Root cause

The workspace, terminal, media, agent, skills, memory, providers, index, drafts and router-console routers each wrapped their routes with `route_layer(... LocalOriginGuard ...)`. The routes registered directly in `bootstrap()` did not. That left `/api/config`, `/api/chat`, `/api/chat/stream`, conversations, projects, workflows, extensions, update, uninstall and the static UI outside the check.

### Reproduction (before the fix)

```bash
curl -H 'Host: evil.example:8899' http://127.0.0.1:8899/api/config
# HTTP 200 + full config JSON (keys are redacted, but base URL/settings are exposed)

curl -H 'Host: evil.example:8899' http://127.0.0.1:8899/api/capabilities
# HTTP 200

curl -H 'Host: evil.example:8899' http://127.0.0.1:8899/api/workspace
# HTTP 403 (the already-guarded comparison route)
```

A browser cannot set `Host` arbitrarily, but DNS rebinding makes `evil.example` resolve to `127.0.0.1`; the browser then sends `Host: evil.example:8899` to the local server. Without the guard, the attacker's page can read config and invoke state-changing routes that do not require a custom header or a body the attacker cannot send.

### Fix

The entire local router now receives one guard **after** all module routers are merged:

```rust
.merge(router_console::router(port))
.merge(router_api)
.route_layer(middleware::from_fn_with_state(
    workspace::LocalOriginGuard::new(port),
    workspace::guard_local_origin,
))
.layer(middleware::from_fn(local_security_headers))
```

This is deliberately one enforcement point rather than another route list. Existing per-module guards remain as defense in depth; the outer layer closes the forgotten routes and any future route added to the same router.

### Verification

- `scripts/ci/test-e2e.sh` new checks:
  - foreign `Host: evil.example:$port` on `/api/config` → **403**, body says loopback only;
  - foreign `Origin: http://evil.example` on `/api/capabilities` → **403**, body says cross-site;
  - normal loopback requests continue to return 200.
- The complete 53-check suite passes against both debug and optimized release binaries.
- Manual before/after reproduction is recorded above; a foreign `Origin` is separately refused on `/api/capabilities`.

### Remaining risk

The guard trusts `Host`/`Origin` string matching against `localhost`, `127.0.0.1`, `[::1]` and the configured port. A future reverse proxy or a non-loopback bind would need a different policy; the app intentionally binds only `127.0.0.1`. Same-user local processes remain outside this boundary by design.

## SEC-02 — Recent-folder HTML injection

**Severity:** Medium (P2). CSP (`script-src 'self'`) blocks inline event handlers, which reduced exploitability, but markup injection into the app origin is still a defect and could be used for UI spoofing or future CSS/data exfiltration.

**Component:** `app/static/app.js`, `renderWorkspaces()`.

### Root cause

A path from the filesystem's recent-folder list was interpolated into a template assigned to `row.innerHTML`:

```js
row.innerHTML = `<span ...><small title="${path}">${path}</small></span>`;
```

A folder named `<img src=x onerror=...>` becomes markup. `title` interpolation also allowed attribute injection if quoting was ever changed.

### Fix

`recentFolderRow(path)` now creates the elements and assigns `small.textContent = path` and `small.title = path`. No path string is parsed as HTML. The row is still a normal clickable project-menu item.

### Verification

A new frontend test in `app/static/tests/render.test.js`:

```js
const hostile = '<img src=x onerror="alert(1)">';
const recent = call(`recentFolderRow(${JSON.stringify(hostile)})`);
assert.equal(small.textContent, hostile);
assert.equal(small.innerHTML, "", "no folder path is parsed as HTML");
```

Frontend result: **115/115**.

## SEC-03 — Remote MCP plain HTTP

**Severity:** Medium, configuration-dependent.  
**Component:** `app/src/mcp.rs`, `app/src/mcp_oauth.rs`.<br>
**Status:** **Fixed in 3.88.2.**

### Root cause

`valid_url` accepted both schemes for any host and was applied only by the save handler. Connection setup read an existing registry file directly, and OAuth discovery/registration/token/authorize URLs were trusted as returned by metadata. A remote cleartext endpoint could therefore receive a configured bearer header or OAuth token even if the panel advised HTTPS.

### Fix

One shared policy (`is_loopback_host` / `remote_url_error` / `require_remote_url`): HTTPS anywhere, HTTP only for `localhost`, `127.*`, `::1` or `0.0.0.0`. It is enforced when a server is saved, before `Connection::open` contacts a server, before a legacy-SSE announced endpoint is opened, for every OAuth URL (discovery, registration, token, authorization), and for every redirect hop through `safe_redirect_policy`. The parser is the same `reqwest::Url` used by the client, and the host check parses an IP address rather than prefix-matching a string.

### Verification

- `plain_http_is_only_for_this_machine`
- `opening_an_insecure_remote_server_is_refused_before_connect`
- `metadata_and_token_endpoints_never_use_cleartext_off_loopback`
- `a_redirect_to_cleartext_off_loopback_is_refused`
- e2e: remote HTTP save → **400**; loopback HTTP save/remove → **200**.

**Residual:** HTTPS to a host that resolves to a local service remains allowed by design; TLS validation still applies. Existing remote HTTP configurations now fail with an actionable HTTPS message, which is the intended compatibility change.

## SEC-04 — Unsigned packages and checksum trust

Windows/macOS artifacts are not code-signed or notarized. Downloads are verified against a SHA-256 published by the same GitHub release. This detects corruption and a tampered asset that did not also replace `SHA256SUMS.txt`, but it is not a cryptographic signature rooted in an offline key. A compromise of the GitHub release or repository can replace both.

No change was made in this audit. Code signing requires external certificates/accounts and a platform build environment that was not available. The release gate does reject an asset with no checksum and refuses an update whose hash differs.

## SEC-05 — MCP credential storage

**Status:** **Header storage fixed in 3.88.2.** The report’s claim that desktop MCP OAuth tokens were a 0600 file was stale: `mcp_oauth.rs` already uses `crate::keychain`. The real gap was saved custom headers in `mcp.json`.

### Fix

- Header values are written to the OS credential store under `mcp_headers/<server>`.
- `mcp.json` keeps only header names; `read_servers` hydrates values only for names the file still declares.
- Startup migrates legacy plaintext values. If the store is unavailable, the value remains in the permission-restricted file rather than being lost.
- Removing a server deletes its vault entry.

### Verification

`header_values_live_in_the_keychain_not_the_registry` asserts the raw file value is empty, the read path hydrates it, and removal clears the entry. The test uses the project’s hermetic mock keychain, not a developer vault. A live Secret Service test remains a verification gap.

**Residual:** the VS Code embedded Node server stores its OAuth session in a 0600 file (no VS Code `SecretStorage` bridge). Same-user filesystem access remains outside the boundary by design.

## SEC-06 — VS Code dev-tool advisories

- Production dependencies: `npm audit --omit=dev` → **0 vulnerabilities**.
- Full audit: **3 moderate** advisories, all via `@vscode/vsce` (dev packaging tool), `glob` and `minimatch`; no high/critical; fix requires a semver-major `@vscode/vsce` upgrade.
- Website: full audit → **0 vulnerabilities**.

No blind major upgrade was made. The packaging tool is not shipped in the extension runtime.

## SEC-07 — Rust advisory scan

**Status:** **Fixed in 3.88.2.** `cargo-audit 0.23.0` was installed and run against the final lock. The scan found vulnerabilities in the locked dependency set, including critical `RUSTSEC-2026-0369` for `zip 2.6.1`; those dependencies were upgraded and the final scan reports **0 vulnerabilities**.

```text
Scanning Cargo.lock for vulnerabilities (523 crate dependencies)
warning: 5 allowed warnings found
exit 0
```

The five warnings are `chrono`, `paste`, `proc-macro-error`, `rustls-pemfile` (unmaintained, no fixed upgrade) and `glib 0.18.5` (unsound; requires a newer Tauri/GTK line). They are documented, not ignored.

## Controls verified as effective

| Control | Evidence |
|---|---|
| Loopback-only UI bind | Server starts on `127.0.0.1`; e2e connects only there |
| Global local-origin guard | SEC-01 fix; foreign Host/Origin 403 |
| Security headers | Live: CSP `default-src 'self'`, `script-src 'self'`, `frame-ancestors 'none'`, XFO DENY, nosniff, no-referrer, `Cache-Control: no-store` |
| Secret redaction | `/api/config` returns `has_api_key`, never the key; provider rows `has_key`; both tested |
| Constant-time token compare | `host_auth.rs` uses SHA-256 + `subtle::ConstantTimeEq`; 256-bit OS-random token |
| Host API transport policy | TLS required when enabled; bind rejects `0.0.0.0`, public IPs and hostnames; bearer required on every route |
| Host API error redaction | 401 is uniform; internal error marker stripped; `no-store`/nosniff on responses |
| Workspace containment | `resolve_inside` canonicalizes and rejects `..`/symlink escape; e2e 400 for `../outside.txt`; Rust symlink tests |
| Command execution gates | Agent/terminal actions go through `permissions` allow/ask/deny; `ask` parks for approval; emergency stop exists |
| Wasm sandbox | wasmi, no WASI, fuel/memory/table caps, host calls permission- and budget-gated; ADR 0001 |
| Update integrity | HTTPS only, published SHA-256 required, staged hash rechecked, zip/tar traversal refused, atomic swap/rollback |
| SQL safety | Queries use bound parameters; dynamic SQL fragments are fixed column/table identifiers from server-side matches, not user text |
| Archive extraction | `installer.rs` refuses entries outside the destination and unsafe names |
| AI output rendering | Markdown is escaped before its small formatting subset; code blocks escaped; model/tool text is not raw HTML |
| Extension panels | `default-src 'none'` CSP into `sandbox="allow-scripts"` opaque-origin frame; no data bridge |
| Database foreign keys | `PRAGMA foreign_keys = ON`; conversation/project cascade verified |
| Test isolation | e2e uses a scratch home and SQLite database; no real user data touched |

## AI-specific assessment

- Agent tool calls are parsed from untrusted model output but are **not** executed directly: they pass through the action parser, permission resolution, approval gate, workspace path checks, and in the extension case a manifest and schema check.
- Prompt injection can still influence what an agent *proposes*. The security boundary is the approval/permission layer; a model cannot grant itself a saved allow rule through chat.
- Secrets are not put in the model context by the config serializers; provider keys are used by the server transport, not returned to the browser. A malicious file or MCP result can still contain a user secret and cause the model to repeat it; users should not place secrets in workspace files or prompts.
- The extension `ask` policy is a refusal, not an interactive prompt, because unattended Wasm code cannot answer. This is safe-by-default but restrictive.
- MCP/extension network and command calls require explicit saved allow rules; no default network capability is granted by installing an extension.

## Database and data protection

- SQLite stores conversations, messages, projects, memory, runs, steps, workflows, permissions, usage and local indexes.
- The database file is created under the platform data directory; no remote database is used.
- Deleting a conversation cascades to messages (verified). Project deletion cascades conversations and clears the active pointer (code/Rust tests).
- Backup/recovery: startup copies and verifies an existing database before migration (`an_existing_database_is_copied_and_verified_before_it_is_migrated`).
- Import/export of workflows is document-based and size/field validated; no arbitrary archive extraction in that path.
- No destructive database migration was performed on real data during this audit.

## Recommendations, in priority order

1. ~~Implement the remote MCP HTTPS policy (SEC-03)~~ — done in 3.88.2.
2. ~~Run `cargo audit` and fix advisories~~ — done; 0 vulnerabilities, 5 documented warnings.
3. ~~Move saved MCP headers to the OS keychain~~ — done on desktop; VS Code OAuth session remains 0600.
4. Obtain Windows code-signing and macOS notarization credentials; sign the 11-asset release set.
5. Add a real signed-package install/uninstall/upgrade test on Windows and macOS.
6. Add a browser layout/accessibility check and a timed slow-stream stop test.
7. Keep the foreign-Host/Origin and MCP TLS e2e checks release-blocking.
8. Add `cargo audit` to CI (or a scheduled job) so the clean scan cannot silently regress.

## Verdict

The high-severity request-boundary defect is closed, the MCP cleartext and header-storage gaps are closed, and the locked Rust dependency set has no known vulnerabilities after the 3.88.2 upgrades. The tested Linux build enforces the loopback origin policy on every local route, keeps secrets out of config responses, contains filesystem access, gates agent/tool actions, and preserves a strong default-deny Wasm sandbox. Remaining findings are external signing/platform gaps, five advisory-database warnings, a real-keychain verification gap, and product/test breadth—not unrepaired high-severity code defects found in this pass.
