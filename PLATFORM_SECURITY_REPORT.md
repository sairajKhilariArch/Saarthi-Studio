# Sarthi Studio — Phase 8 Platform Security

**Date:** 2026-11-25
**Build:** 3.88.12 (`platform/phase-8-cross-platform-2026-11-25`)

This report separates what was **verified on this machine**, what was **compiled/tested by CI on another OS**, and what is **blocked by missing credentials or hardware**.

## Linux

| Control | State | Evidence |
|---|---|---|
| Config directory | Verified | `~/.config/free-ai-studio` created **0700** |
| Database | **Fixed and verified** | SQLite file is now **0600** on open; was 0644 under umask 022 |
| Config/state files | Implemented | Atomic temp file + 0600 + rename (`write_private_json`, `save_config_file`) |
| Secrets | Implemented | `keyring` with `sync-secret-service`; API keys are not in `config.json` by default |
| Local HTTP boundary | Verified | Loopback bind; foreign `Origin` → 403; foreign `Host` → 403 |
| Browser hardening | Verified | CSP `script-src 'self'`, `frame-ancestors 'none'`, `X-Frame-Options: DENY`, `nosniff`, `no-referrer` |
| Process isolation | Implemented | Terminal roots contained to the workspace; extension runner is a WASM interpreter with bounded imports (earlier phases) |
| Update integrity | Implemented / blocked runtime | SHA-256 re-verified before unpack; no live update was installed on Linux |

The database permission fix is in `open_db` (`app/src/main.rs`), covered by the new E2E assertion. The 0700 directory already prevented access by other users; the file mode now holds even if that directory is relaxed.

## Windows

| Control | State |
|---|---|
| Credential Manager | **Compiled** via `keyring` feature `windows-native`; Desktop CI builds/tests Windows code. Runtime credential-store access was not exercised (no Windows machine) |
| File permissions | Data sits under the user profile; code relies on the profile ACL (`set_private_permissions` is a documented no-op on non-Unix) |
| Installer scope | Per-user MSI under `%LOCALAPPDATA%\Programs`; no elevation |
| Installer signature | **Not signed** — no Authenticode certificate available |
| Update | Prepared with SHA-256; no automatic replacement of a running `.exe` |
| PowerShell use | Fixed scripts only; no request text interpolated into the folder picker or cleanup command |

Marked **BLOCKED**: Credential Manager runtime test, Authenticode signing, SmartScreen reputation, MSI install/uninstall on a real Windows machine.

## macOS

| Control | State |
|---|---|
| Keychain | **Compiled** via `keyring` feature `apple-native`; Desktop CI builds/tests macOS code. Keychain access not exercised (no Mac) |
| File permissions | Files under the user's home; Unix chmod applies |
| Code signing | **None** |
| Hardened runtime | **Not configured** |
| Notarization | **None** — no Apple Developer certificate or notary credentials |
| Gatekeeper | Will warn/block a downloaded unsigned bundle; documented as a release limitation |
| Entitlements | None added; no sandbox entitlement set |

Marked **BLOCKED**: signing, hardened runtime, notarization, Gatekeeper acceptance, Keychain runtime test, universal-binary execution. CI only proves the source compiles and tests on macOS.

## Android

| Control | State |
|---|---|
| Credential storage | AES-GCM key in **Android Keystore** (`SarthiStudioSettingsKeyV1`); no plaintext key in preferences |
| App sandbox | Standard Android app sandbox; only `INTERNET` and `ACCESS_NETWORK_STATE` permissions |
| Backup | `android:allowBackup="false"` |
| Cleartext | **Fixed:** platform permits cleartext; `CleartextPolicy` refuses public HTTP before opening a socket and allows only localhost/private/link-local hosts |
| Release identity | **Fixed:** no longer signed with the debug keystore; CI produces an unsigned APK/AAB for signing at distribution |
| Exported surface | Only the launcher activity is exported; no deep-link intent filter exists |
| Network security | No custom trust anchors; system CA validation applies to HTTPS |
| Desktop privileges | No filesystem root access, no shell execution, no desktop-host credential mixed with router key |

Verified by Android CI run `45686438710`: unit tests including the new cleartext policy tests, lint (0 errors), debug/release assembly and API 35 emulator instrumentation all passed. Marked **BLOCKED**: production signing key, Play integrity, physical-device Keystore inspection, network-security runtime capture.

## Web

| Control | State |
|---|---|
| Provider secrets in browser | No — the browser calls the local backend, which owns credentials |
| CSP | Enforced; no inline scripts or handlers |
| CORS | Not enabled; same-origin only. Cross-origin writes are rejected by the origin guard |
| SSE | Same-origin, guarded by the same origin middleware |
| Cookies / sessions | No authentication or cookies exist for the loopback UI |
| Remote host API | HTTPS with bearer token, separate router and TLS configuration |
| localStorage tokens | Not used for provider credentials; theme/UI preferences only |

## Dependency and secret checks

`cargo audit` on this branch:

- **0 vulnerabilities**
- **5 warnings**, all allowed by the project policy:
  - `chrono` — unmaintained (RUSTSEC-2026-0543, no fixed upgrade)
  - `paste` — unmaintained (RUSTSEC-2024-0436)
  - `proc-macro-error` — unmaintained (RUSTSEC-2024-0370)
  - `rustls-pemfile` — unmaintained (RUSTSEC-2025-0134)
  - `glib 0.18.5` — unsound iterator impls (RUSTSEC-2024-0429; requires glib ≥ 0.20, currently pinned by the Tauri/GTK stack)

A diff scan for private-key blocks, GitHub tokens and `sk-…` provider keys found no matches. No certificate, signing key or notarization credential is present or committed.

## Honest limits

- Compilation on Windows/macOS is not a runtime credential-store test.
- No installer is signed or notarized.
- Android release artifacts are unsigned until the distributor signs them.
- The local web UI has no authentication; binding it to a non-loopback interface is outside its supported threat model.
