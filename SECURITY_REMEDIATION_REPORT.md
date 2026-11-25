# Sarthi Studio — Security Remediation Report

**Date:** 2026-11-25  
**Release:** 3.88.3 (security fixes landed in 3.88.2; Phase 3 reran the matrix)  
**Branch:** `remediation/security-2026-11-24`  
**Scope:** desktop/local UI server, local HTTP router, MCP transports/OAuth, extension sandbox, Host API, filesystem/terminal gates, update/install path, SQLite storage, VS Code extension, website, dependency supply chain.

## Executive summary

Two code-level security gaps remained after the 3.88.1 audit, and the dependency surface had never been checked with a real Rust advisory scanner:

1. **Remote MCP could downgrade to cleartext HTTP.** The URL check accepted `http://` for any host. Headers and OAuth tokens could cross the network unencrypted. A UI warning is not an enforcement boundary, and an old `mcp.json` bypassed the panel entirely.
2. **Saved MCP header values were plaintext in `mcp.json`.** Provider keys and desktop MCP OAuth tokens already used the OS credential store; custom headers did not.
3. **RustSec scanning had not run.** Installing `cargo-audit` found vulnerabilities in the locked `zip`, `libsqlite3-sys`, `url`, `tokio` and `cookie` versions, including a critical `zip` advisory. `zip` is used by the update/install extraction path, so this was not theoretical.

All three are fixed. `cargo audit` now reports **0 vulnerabilities**. The full Rust suite (712 passed, 1 ignored), both real-binary e2e runs (61/61 debug and release), clippy, rustfmt, frontend, VS Code, website and release gates pass. A Linux `.deb` payload was also assembled and booted from its unpacked tree; its dependency metadata is incomplete on this host, which is a packaging limitation, not a security regression.

No P0 or unmitigated P1 issue remains in the tested Linux build. External signing, cross-platform runtime validation and five RustSec **warnings** remain; see `REMAINING_ISSUES.md`.

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| SEC-01 | High / P1 | Core routes outside loopback `Host`/`Origin` guard | **Fixed and retained** from 3.88.1; release-blocking e2e |
| SEC-02 | Medium / P2 | Recent-folder path parsed as HTML | **Fixed and retained** from 3.88.1 |
| SEC-03 | Medium / P2 | Remote MCP plain HTTP could expose headers/OAuth tokens | **Fixed**: HTTPS off loopback at save, connect, SSE and every OAuth endpoint |
| SEC-04 | Medium / P2 | Unsigned Windows/macOS packages; checksums from the same origin | **Blocked**: needs certificates/notarization and platform hosts |
| SEC-05 | Medium / P2 | MCP header values stored in `mcp.json` | **Fixed**: OS credential store + startup migration + 0600 fallback |
| SEC-06 | Low / dev-only | 3 moderate npm advisories in `@vscode/vsce` | **Accepted**: production deps 0; attempted major upgrade reverted |
| SEC-07 | Verification | No Rust advisory scan | **Fixed**: `cargo-audit` run; 0 vulnerabilities, 5 warnings |
| SEC-08 | Process | Windows/macOS/Android security behavior not observable | **Blocked**: no hosts/devices |
| DEP-01 | High / supply chain | Vulnerable Rust lock entries, including critical `zip` advisory | **Fixed**: upgrades; audit clean; full test/build matrix passes |

## SEC-03 — MCP transport policy

### Threat

A remote MCP server is a credential-bearing integration. The desktop client may attach a user-configured `Authorization` header or an OAuth access token. Plain HTTP lets a network attacker read or modify tool results and credentials. The audit found `valid_url` accepted both schemes. The panel hint alone did not stop:

- an older `mcp.json` with a cleartext URL;
- OAuth metadata naming `http://` discovery/registration/token/authorize endpoints;
- a legacy SSE server announcing a cleartext message endpoint.

### Enforcement

`app/src/mcp.rs`:

- `is_loopback_host` accepts `localhost`, `127.*`, `::1`, `0.0.0.0` (and bracketed IPv6 spellings).
- `remote_url_error` parses with `reqwest::Url`, rejects empty/oversized/whitespace/control values, accepts `https` anywhere, accepts `http` only for loopback, and returns an actionable sentence otherwise.
- `require_remote_url` is the shared guard.

Applied at:

- `save_server` → HTTP 400 for a new or updated remote cleartext server;
- `Connection::open` → old registry entries are refused **before** a request or header is sent;
- SSE announced endpoint → refused before opening;
- `mcp_oauth.rs`: `get_json` discovery, `register_client`, `token_request`, `prepare` server URL, authorization endpoint and token endpoint.
- every redirect hop: `safe_redirect_policy` re-applies the same rule before following, for the MCP HTTP/SSE clients and all three OAuth clients. Without it, an HTTPS server could redirect the client to a cleartext host after the initial check.

The parser is the same URL implementation the HTTP client uses, so an accepted value cannot be reinterpreted later. HTTPS to a private LAN address remains allowed; only cleartext off loopback is refused.

### Verification

- `plain_http_is_only_for_this_machine` accepts every loopback spelling and rejects `example.com`, `192.168.1.20`, `mcp.internal`, and `127.evil.com` / `127.0.0.1.evil.com`; `https://192.168.1.20` is accepted. The implementation parses an `IpAddr` rather than prefix-matching, so a DNS name cannot inherit the loopback exception.
- `opening_an_insecure_remote_server_is_refused_before_connect` proves no connection is attempted.
- `metadata_and_token_endpoints_never_use_cleartext_off_loopback` proves discovery/token refusal before I/O.
- `a_redirect_to_cleartext_off_loopback_is_refused` uses a local fixture that answers `302 Location: http://example.com`; the client refuses the hop without contacting the external host.
- E2E: remote HTTP save → 400; loopback HTTP save/remove → 200.
- Both debug and release e2e: 61/61.

**Residual:** the policy allows HTTPS to any host, including a hostname that resolves to a local service. TLS certificate validation still applies; this is not an SSRF bypass because the user deliberately configured the server and the connection is authenticated by TLS.

## SEC-05 — MCP header storage

### Threat

Custom MCP headers often carry bearer tokens. A same-user process or a filesystem backup could read them from `mcp.json`, even with 0600 permissions. Provider keys and desktop OAuth tokens already had a stronger store.

### Implementation

- Account name: `mcp_headers/<server>`, service derived from `keychain_location(path)` (the existing config/keychain isolation, including test mock services).
- `read_servers` hydrates values from the store, but only for keys still declared in the file, so a removed header is not resurrected.
- `write_servers` writes the full header map to the store, then blanks values in the JSON copy. On store failure it leaves the value in the permission-restricted file rather than dropping it.
- Removing a server deletes its vault entry.
- `migrate_header_secrets` runs at boot, inspects the **raw** file to avoid rewriting on every boot, and moves legacy plaintext values. A malformed file is untouched.
- API views already returned header **names** only; no value is exposed.

### Verification

`header_values_live_in_the_keychain_not_the_registry` round-trips a header through the real registry functions, asserts the raw file has an empty value, `read_servers` hydrates it, and removal clears the vault entry. The test runs with `FREE_AI_STUDIO_KEYCHAIN=mock` via `.cargo/config.toml`, so it is hermetic.

**Residual:** a real Linux Secret Service/macOS Keychain/Windows Credential Manager round trip was not run; the VS Code embedded server’s OAuth session remains a 0600 file, documented in `REMAINING_ISSUES.md`.

## SEC-07 / DEP-01 — Supply chain

### Scan result after remediation

```
$ cd app && cargo audit
...
Scanning Cargo.lock for vulnerabilities (523 crate dependencies)
warning: 5 allowed warnings found
$ echo $?
0
```

**0 vulnerabilities.** The five warnings are unmaintained/unsound advisories with no safe in-line upgrade:

- `chrono` 0.4.45 — unmaintained (RUSTSEC-2026-0543)
- `paste` 1.0.15 — unmaintained (RUSTSEC-2024-0436)
- `proc-macro-error` 1.0.4 — unmaintained (RUSTSEC-2024-0370)
- `rustls-pemfile` 2.2.0 — unmaintained (RUSTSEC-2025-0134)
- `glib` 0.18.5 — unsound iterator impls (RUSTSEC-2024-0429); needs a newer Tauri/GTK line

### Upgrades

| Crate | Before | After | Why |
|---|---|---|---|
| `zip` | 2.6.1 | 9.8.2 | critical archive advisories; used by update/install |
| `rusqlite` | 0.32.1 | 0.43.0 | bundled SQLite advisory |
| `libsqlite3-sys` | 0.30.1 | 0.40.0 | bundled SQLite version |
| `tokio` | 1.43.0 | 1.54.1 | advisory |
| `url` | 2.5.8 | 2.5.9 | advisory |
| `cookie` | 0.18.2 | 0.18.3 | advisory |

No blind major upgrade: every step was followed by the full suite, release build and both e2e runs. `zip` keeps only `deflate`; `rusqlite` keeps `bundled`. The archive traversal, checksum, update staging/rollback, SQLite migration/backup/locking and full server tests pass.

### Non-Rust advisories

- VS Code production dependencies: **0 vulnerabilities**.
- VS Code full audit: **3 moderate**, dev-only `glob`/`minimatch` via `@vscode/vsce`.
- Website: **0 vulnerabilities**.

The attempted `@vscode/vsce` 4.1.1 upgrade left 2 moderate advisories, required Node >= 22 and reformatted `package.json`; it was reverted rather than accepted as a partial fix.

## Controls re-verified

| Control | Evidence |
|---|---|
| Loopback-only UI bind | Server binds `127.0.0.1`; e2e connects only there |
| Whole-router `Host`/`Origin` guard | Foreign Host on `/api/config` 403; foreign Origin on `/api/capabilities` 403 |
| Security headers | CSP, `frame-ancestors 'none'`, XFO DENY, nosniff, no-referrer, `no-store` |
| Secret redaction | `/api/config` exposes `has_api_key`, never the key; provider rows `has_key` |
| Host API auth | SHA-256 verifier + `subtle::ConstantTimeEq`, 256-bit token, TLS-only when enabled |
| Host API bind policy | Rejects unspecified/public addresses and hostnames; Rust tests |
| Workspace containment | Canonicalized `resolve_inside`; e2e traversal 400; symlink escape tests |
| Agent/terminal gates | Permission allow/ask/deny; e2e parks a write for approval |
| Wasm sandbox | wasmi, fuel/memory/table caps, permission-gated host calls, no WASI |
| Update integrity | HTTPS, published SHA-256, staged hash recheck, traversal refusal, atomic swap/rollback |
| SQL safety | Bound parameters; dynamic identifiers only from server-side allowlists |
| AI output rendering | Markdown escaped before its formatting subset; code escaped |
| Extension panels | `default-src 'none'` CSP in a sandboxed opaque-origin frame |

## AI-specific security assessment

The application’s core policy remains: model output is **data**, not authority. Agent actions pass through the same permission broker as user actions, writes require approval when the rule says ask, worktree checkouts isolate runs, destructive operations are gated, and the emergency stop cancels live runs and MCP sessions. MCP tool calls are gated by the run and the server/tool rule. The new transport rule closes a credential-exfiltration path without granting the model any new network or filesystem authority.

No prompt-injection escape was reproduced. The remaining risk is the ordinary one: a user can approve a destructive action, and a compromised same-user process is outside the boundary by design.

## Verdict

The high-severity request-boundary fix remains in place, the two code-level MCP credential gaps are now closed in source and covered by tests, and the Rust dependency scan is clean. The tested Linux 3.88.3 build is materially stronger than 3.88.1, and Phase 3’s independent rerun found no new security defect. It is **not** a signed cross-platform security sign-off: Windows/macOS/Android behavior, signed packages, a real OS keychain, and a real third-party MCP peer were not exercised here.
