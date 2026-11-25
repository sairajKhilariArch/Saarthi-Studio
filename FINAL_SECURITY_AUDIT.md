# Sarthi Studio — Final Security Audit

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Code verified:** 3.88.17 (`1d6e458`)  
**Scope:** Rust desktop/web host, bundled UI, agent/skill/workflow execution, MCP and host API, distribution metadata, CI configuration

## Executive result

No **critical** or **high** code vulnerability was found in the tested Linux surface. Three security-relevant gaps remain and are not code defects that can be repaired in this environment:

1. Windows and macOS artifacts are **unsigned** (no Authenticode, Developer ID, hardened runtime or notarization).
2. No live third-party provider, search, MCP, language-server or debugger peer was tested.
3. The real OS credential store was not exercised; tests use the project’s mock keychain backend.

The application is a **local, single-user loopback service**, not a multi-tenant web application. Authentication/session/CSRF controls for remote accounts are therefore **NOT APPLICABLE** by design; the relevant boundary is the loopback HTTP server, Host/Origin checks, filesystem containment and agent permissions.

This audit does not claim the product is “completely secure”.

## Findings

| ID | Severity | Finding | State | Evidence / action |
|---|---|---|---|---|
| SEC-10-01 | Medium | An unreadable `config.json` was silently replaced by defaults, losing the user's settings with no recovery copy | **Fixed in 3.88.17** | `preserve_unreadable_config` copies to `config.json.corrupt-<epoch>` and logs the path before defaults are written. Reproduced on the release binary: backup contained the original `{broken` bytes; regression tests added. |
| SEC-10-02 | Medium | Windows/macOS release artifacts are unsigned; checksums share the release origin | Open, externally blocked | `CODE_SIGNING_REPORT.md`, Phase 9 publish gate returns 3 BLOCKED signature checks. Must be described as unsigned. |
| SEC-10-03 | Low | Saved-key help text read “goes to in the OS credential store” | Fixed in 3.88.17 | UI string corrected; verified in the live DOM. |
| SEC-10-04 | Low | Three moderate npm advisories in the VS Code **packaging** toolchain | Accepted | `npm audit --omit=dev` = 0; full audit = 3 moderate via `@vscode/vsce`; not shipped at runtime. |
| SEC-10-05 | Low | `glib` 0.18.5 unsoundness warning (RUSTSEC-2024-0429); four unmaintained-crate warnings | Accepted, dependency-line blocked | `cargo audit`: 0 vulnerabilities, 5 warnings. Removing requires a newer GTK/Tauri chain, which cannot be GUI-tested here. |
| SEC-10-06 | Low | No live OS keychain round trip (Linux Secret Service / macOS Keychain / Windows Credential Manager) | Verification gap | Mock keychain exercises read/write/migration; production path compiles but was not opened. |
| SEC-10-07 | Low | Prompt injection remains possible through files, skills and model output | Accepted model-level risk | Skills are local instructions, not executable; tool writes/commands require approval. No claim of immunity. |
| SEC-10-08 | Informational | No per-message edit/delete; no remote account/session layer | Scope decision | Not a vulnerability; documented product scope. |

## Application boundary controls verified

| Control | Result | Evidence |
|---|---|---|
| Loopback bind | PASS | Server listens on `127.0.0.1`; documented and exercised |
| Foreign `Host` rejection (DNS rebinding) | PASS | E2E: `Host: evil.example` → 403 with loopback message on core routes |
| Foreign `Origin` rejection | PASS | E2E: `Origin: http://evil.example` → 403 “Cross-site…” |
| Workspace path containment | PASS | E2E traversal/absolute/symlink escapes rejected; 1 MB/binary limits |
| Command execution approval | PASS | E2E approval and denial; denied command does not run |
| Agent write permission | PASS | E2E `allowWrites:false` run cannot write; a write-requiring run failed safely |
| MCP transport policy | PASS | E2E TLS downgrade rejected off loopback; loopback fixture accepted |
| Local API secrets exposure | PASS | Config API returns `has_api_key`, never the key; UI placeholder “leave blank to keep” |
| Database permissions | PASS | E2E verifies `0700` data directory and `0600` database |
| Config permissions | PASS | `0600`; save uses temp file + rename |
| Log redaction | PASS | Unit tests cover secret redaction in operation log and request errors |
| Crash/error responses | PASS | E2E provider outage, corrupt DB, unwritable directory; actionable errors, no secret echo |
| Update integrity | PARTIAL | HTTPS + published SHA-256 + staged re-hash + archive traversal refusal unit-tested; no live update because no release was published |
| CSRF | NOT APPLICABLE | No cookie/session-authenticated browser mutation surface; same-origin/Host guards still enforced |
| Multi-user authorization | NOT APPLICABLE | Single local user; no accounts or remote tenants |

## AI and agent security

| Check | Result | Notes |
|---|---|---|
| Prompt injection | Residual risk | Model can be influenced by workspace content and skills; approvals bound actions, not reasoning |
| Tool permission enforcement | PASS | Writes/commands gated; E2E allow/deny |
| Agent permission boundaries | PASS | Profiles, `allowWrites`, max steps, token budget |
| Skill trust boundary | PASS (bounded) | Local Markdown instruction packs; no code execution; version compatibility enforced |
| MCP security | PASS (tested policy) | Loopback/TLS policy and OAuth storage; no real MCP vendor server |
| Remote URL validation | PASS | Provider/MCP URL validation rejects unsupported schemes and TLS downgrade off loopback |
| Tool output handling | PARTIAL | Outputs are treated as untrusted data and rendered as text/Markdown; no formal sanitizer audit beyond tests |
| Sensitive data leakage | PASS | Keys masked; request logs redact known secret fields |
| Unauthorized file access | PASS | Canonical path containment and root checks |
| Dangerous shell execution | PASS (bounded) | Approval required; no unsandboxed auto-run claimed |

## Data security

- Provider keys: OS credential store when available; fallback is `0600` config for a broken keychain. Migration is idempotent.
- Database: SQLite under `~/.local/share/free-ai-studio` with private permissions.
- Backups: database backup before migration; failed upgrade rolls back (unit-tested).
- Temporary files: atomic config write via `.tmp`; attachment store uses per-message files.
- Deletion: conversation delete and file delete are explicit; uninstall data-retention behavior is documented (user data outside the package is retained).
- Crash reports: no external crash-report upload path was found.

## Supply chain

| Item | Result |
|---|---|
| Rust dependencies (`cargo audit`) | 0 vulnerabilities, 5 warnings (see SEC-10-05) |
| Frontend dependencies | 125 tests; no production npm runtime for the desktop UI |
| VS Code extension production deps | `npm audit --omit=dev` = 0 |
| Website dependencies | `npm audit` = 0 |
| Installer contents | `.deb` payload 14/14 checks; CI artifact list; no secrets shipped |
| Checksums | Phase 9 bundle 13/13 SHA-256 verified |
| SBOM | CycloneDX 523 components verified in Phase 9 |
| Provenance | Build provenance attestation succeeded in Phase 9 |
| Artifact signatures | Absent; publish gate correctly blocks |
| CI permissions / secrets | Workflows use scoped tokens; no secret values printed; release signing secrets absent |

## Repairs made in Phase 10

1. **Corrupt configuration preservation** (SEC-10-01), with two regression tests and a release-binary reproduction.
2. **Credential-store help text** (SEC-10-03), verified live.
3. **README runtime commands and data paths** corrected to the real `free-ai-studio` binary, `FREE_AI_STUDIO_*` variables and `free-ai-studio` data directories. Incorrect docs are a security risk when they send users to the wrong files or make them expose a port with a non-working variable.

No critical/high finding remains unresolved. The unresolved items are external trust/signing and verification gaps, not a known exploitable code path on the tested build.
