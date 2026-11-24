# Sarthi Studio — Verified Fixes

**Date:** 2026-11-24  
**Release:** 3.88.2  
**Rule followed:** reproduce, identify the cause, fix it, add a regression test, run the affected suite, then run the full matrix. No test was skipped, weakened or replaced with a mock success.

## Summary

| ID | Fix | Regression evidence | Verified result |
|---|---|---|---|
| SEC-03 | Remote MCP must use HTTPS off loopback at every connection boundary | 3 Rust tests + 2 e2e checks | Pass |
| SEC-05 | MCP header values moved to OS credential store with migration | Rust keychain-registry test | Pass (mock keychain) |
| DEP-01 | Vulnerable Rust lock entries upgraded | `cargo audit`, full suite, release e2e | 0 vulnerabilities |
| SEC-07 | Rust advisory scan performed | `cargo audit` | 0 vulnerabilities, 5 warnings |
| SEC-01 | Whole local router behind loopback guard | 2 e2e checks | Retained, pass |
| SEC-02 | Recent folder rendered with DOM text nodes | Frontend test | Retained, pass |
| BUG-03 | Stream metadata independent of skill match | E2E stream metadata | Retained, pass |

---

## SEC-03 — Remote MCP cleartext downgrade

### Original problem

The desktop MCP editor accepted any `http://` or `https://` URL. A user who configured a remote server with an `Authorization` header, or completed OAuth against it, could send credentials over plain HTTP. The UI only advised HTTPS; the server did not enforce it. An existing `mcp.json` would also keep working after any UI-only change, and OAuth metadata can name arbitrary endpoints.

### Root cause

`valid_url` checked only syntax (`http`/`https` + parsable URL). It had no host policy, and the check existed only on the save path. Connection setup, OAuth discovery, registration, token exchange, authorization URL construction and the legacy SSE announced-message endpoint each trusted their input independently.

### Files modified

- `app/src/mcp.rs`
- `app/src/mcp_oauth.rs`
- `app/static/index.html`
- `scripts/ci/test-e2e.sh`

### Fix implemented

- Added `is_loopback_host`, `remote_url_error` and `require_remote_url`. HTTPS is always accepted. HTTP is accepted only for `localhost`, `127.*`, `::1` and `0.0.0.0`; every other host gets:
  `Use https:// for a remote MCP server; plain http:// is only for this machine.`
- Enforced at:
  - `save_server` (new/updated registry entry → HTTP 400);
  - `Connection::open` (old `mcp.json` refused before any request/header is sent);
  - legacy SSE announced endpoint (`require_remote_url` before opening it);
  - OAuth `get_json` discovery, client registration, token request and authorize/token metadata endpoints;
  - every HTTP redirect hop, using a shared `safe_redirect_policy` for MCP and OAuth clients. A remote HTTPS endpoint can no longer bounce the client to a cleartext host after the first check.
- Changed the MCP panel hint to say remote servers require `https://`.

The URL parser is the same `reqwest::Url` implementation used by the HTTP client, so a value accepted by the policy cannot be reinterpreted into another scheme or host later. The check rejects empty/oversized URLs, whitespace and control characters.

### Tests executed

Rust:

- `mcp::tests::plain_http_is_only_for_this_machine` — accepts loopback forms, rejects `example.com`, a private LAN host, `mcp.internal`, and the numeric-looking DNS names `127.evil.com` / `127.0.0.1.evil.com` (the address parser, not a string prefix, decides).
- `mcp::tests::opening_an_insecure_remote_server_is_refused_before_connect` — `Connection::open` returns an error containing `https://`.
- `mcp_oauth::tests::metadata_and_token_endpoints_never_use_cleartext_off_loopback` — discovery and token calls refuse a remote HTTP endpoint before any network I/O.
- `mcp::tests::a_redirect_to_cleartext_off_loopback_is_refused` — a local HTTP fixture redirects to `http://example.com`; the client refuses the hop and exposes the HTTPS message. No external network is contacted.

Real binary (`scripts/ci/test-e2e.sh`):

- `POST /api/mcp/servers` with `http://example.com/mcp` → **400** and body contains `Use https://`.
- `POST /api/mcp/servers` with `http://127.0.0.1:<mock>/mcp` → **200**, then remove → **200**, `removed: 1`.

### Actual result

- Full Rust suite: **709 passed, 0 failed, 1 ignored**.
- Debug e2e: **53 passed, 0 failed**.
- Optimized release e2e: **53 passed, 0 failed**.

### Verification status

**Verified.** The rejection happens before a connection is opened; no real remote cleartext server was contacted. The loopback path is exercised against the real mock server.

### Side effects / compatibility

Existing remote HTTP MCP configurations now fail with an actionable message. This is the intended security change. Local development over `http://127.0.0.1` continues to work, including the project’s own fixtures.

---

## SEC-05 — MCP header values in `mcp.json`

### Original problem

Provider keys and desktop MCP OAuth tokens already used the OS credential store, but custom MCP header values were persisted verbatim in `mcp.json`. Those values are commonly bearer tokens. The file was permission-restricted, but it was still a plaintext secret at rest.

### Root cause

The MCP registry serializer had no secret split. `read_servers`/`write_servers` treated every field as ordinary JSON. The keychain helper existed and was already used by provider config and `mcp_oauth.rs`, but not by header entries.

### Files modified

- `app/src/mcp.rs`
- `app/src/main.rs`
- `docs/KNOWN-ISSUES.md`
- `CHANGELOG.md`

### Fix implemented

- `header_secret_name(server)` → account `mcp_headers/<server>`.
- `hydrate_header_secrets`: merges stored values back for header names the current file still declares. A header removed in the panel is not resurrected from an old vault entry.
- `stash_header_secrets`: serializes the header map to the keychain, then blanks the values in the copy written to disk. If the store refuses the write, the value stays in the permission-restricted file instead of being lost (same fallback as provider keys).
- `forget_header_secret`: removes the vault entry when a server is removed.
- `migrate_header_secrets`: runs once at startup, inspects the raw file, and moves any non-empty legacy values into the store. A malformed/unreadable file is left untouched.
- Server views already exposed header **names** only; no API response gained a value.

### Tests executed

`mcp::tests::header_values_live_in_the_keychain_not_the_registry`:

1. Writes a server with an `authorization` header through the real registry functions.
2. Reads the raw `mcp.json` and asserts the value is empty.
3. Reads through `read_servers` and asserts the value is hydrated.
4. Removes the server and asserts the keychain entry is gone.

The Rust test process uses the project’s hermetic `FREE_AI_STUDIO_KEYCHAIN=mock` configuration (`.cargo/config.toml`), so no developer vault is touched.

### Actual result

Test passes in the **709 passed / 1 ignored** run. Clippy and rustfmt pass. The e2e server restart sequence still reads its MCP registry normally.

### Verification status

**Verified with the mock keychain backend.** A live Linux Secret Service round trip was **not** exercised on this host; that remains in `REMAINING_ISSUES.md`.

### Side effects / compatibility

- First launch after upgrade rewrites `mcp.json` with blank values and stores the values in the keychain.
- If no credential service is available, the old plaintext fallback remains, with the existing 0600 permissions.
- No public API shape changed; headers are still returned as names only.

---

## DEP-01 / SEC-07 — Vulnerable Rust dependencies

### Original problem

`cargo-audit` was not installed during the audit, so the Rust dependency surface had never been checked against RustSec. Installing it and scanning `app/Cargo.lock` revealed vulnerabilities, including a critical `zip` advisory in 2.6.1.

### Root cause

The lockfile pinned old versions:
`zip 2.6.1`, `rusqlite 0.32.1` / `libsqlite3-sys 0.30.1`, `tokio 1.43.0`, `url 2.5.8`, `cookie 0.18.2`. The application uses `zip` for update/install extraction, which made the archive advisories directly relevant.

### Files modified

- `app/Cargo.toml`
- `app/Cargo.lock`
- `CHANGELOG.md`

### Fix implemented

| Crate | Before | After |
|---|---|---|
| `zip` | 2.6.1 | **9.8.2** |
| `rusqlite` | 0.32.1 | **0.43.0** |
| `libsqlite3-sys` | 0.30.1 | **0.40.0** (bundled SQLite) |
| `tokio` | 1.43.0 | **1.54.1** |
| `url` | 2.5.8 | **2.5.9** |
| `cookie` | 0.18.2 | **0.18.3** |

No dependency was upgraded blindly: each was followed by the complete Rust suite, release build and both e2e runs. `rusqlite` stays on `bundled`, preserving the build without a system SQLite. `zip` keeps `default-features = false, features = ["deflate"]`.

### Tests executed

- `cargo audit` — **exit 0, 0 vulnerabilities**.
- `cargo test --locked` — **709 passed, 0 failed, 1 ignored**.
- `cargo clippy --locked --all-targets -- -D warnings` — pass.
- `cargo fmt --check` — pass.
- `cargo build --release --locked` — pass, 20 MB binary.
- Debug and release real-binary e2e — **53/53 each**, including real SQLite create/read/restart/corruption-recovery and real zip extraction/checksum tests.

### Actual result

No vulnerability remains. Five advisory-database **warnings** remain:

| Crate | ID | Kind | Reason not upgraded |
|---|---|---|---|
| `chrono` 0.4.45 | RUSTSEC-2026-0543 | unmaintained | no fixed upgrade |
| `paste` 1.0.15 | RUSTSEC-2024-0436 | unmaintained | no fixed upgrade |
| `proc-macro-error` 1.0.4 | RUSTSEC-2024-0370 | unmaintained | no fixed upgrade |
| `rustls-pemfile` 2.2.0 | RUSTSEC-2025-0134 | unmaintained | no fixed upgrade |
| `glib` 0.18.5 | RUSTSEC-2024-0429 | unsound | needs a newer Tauri/GTK line; not attempted without a platform test matrix |

### Verification status

**Verified.** `cargo audit` no longer reports a vulnerability and the full suite passes on the upgraded lock. The remaining warnings are documented rather than suppressed.

### Side effects

- `zip` 9 changes internal APIs, but the wrapper in `installer.rs`/`update.rs` compiles and its traversal, hash and extraction tests pass.
- `rusqlite` 0.43 / SQLite 3.5x passes all schema, migration, backup, locking and persistence tests.
- `tokio` 1.54 passes server, streaming, process, signal and PTY tests.

---

## Retained 3.88.1 fixes

### SEC-01 — Core router outside the loopback guard

The whole local router now has one `route_layer(LocalOriginGuard)` after all module routers are merged, plus the existing per-module guards. The e2e suite sends a foreign `Host` to `/api/config` and a foreign `Origin` to `/api/capabilities`; both return **403**. Normal loopback requests continue to pass. This remains release-blocking.

### SEC-02 — Recent-folder HTML injection

`recentFolderRow(path)` builds the row with `createElement`/`textContent`/`title` instead of an `innerHTML` template. The frontend test uses `<img src=x onerror="alert(1)">` and asserts the text is literal and `innerHTML` is empty. Frontend suite: **115/115**.

### BUG-03 — Streaming context metadata

`memory_used` and `retrieved` are emitted independently of `auto_skills` in both stream branches. The e2e creates a memory, streams a related turn in a separate conversation, and asserts `memory_used` appears in the done frame. **53/53** e2e checks pass.

## What was not changed

- No feature was removed to make a test pass.
- No assertion was weakened; the new e2e checks are additional.
- No production integration was mocked. `zip`, SQLite, PTY, process kill/restart and OAuth refusal are exercised through the real binary.
- `main` and the audit branch were not modified or pushed.
