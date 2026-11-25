# Sarthi Studio — Final Technical Debt Register

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Code:** 3.88.17 (`1d6e458`)

Classifications: **Must fix before release** · **Should fix soon** · **Accepted limitation** · **Deferred feature** · **Platform blocked** · **Not applicable**

## Must fix before a public release

| ID | Issue | Why it blocks | Owner action |
|---|---|---|---|
| TD-01 | No code signing (Linux GPG, Windows Authenticode, macOS Developer ID/hardened runtime/notarization) | The release gate correctly refuses to publish; users cannot verify origin beyond checksums | Obtain certificates/credentials and sign the full asset set |
| TD-02 | Windows/macOS installers never installed or launched | A built MSI/`.app` is not a validated product; upgrade/uninstall unknown | Run install → launch → upgrade → uninstall on real hosts |
| TD-03 | Linux system package install/upgrade/uninstall unverified; `.deb` `Depends:` unresolved on this host | Package manager behaviour and dependency correctness unknown | Install on disposable Ubuntu/Debian with `dpkg -i`, upgrade, purge |
| TD-04 | No live update from a published release | Update integrity/recovery is only unit-tested | Publish a test release in a controlled channel, update, roll back, verify data |

## Should fix soon

| ID | Issue | Classification | Notes |
|---|---|---|---|
| TD-05 | No 2–4 hour soak | Should fix soon | Phase 10 ran ~7.5 minutes; Phase 5 reached a 206.5 MB plateau over 14 stress cycles. Long-term drift unknown. |
| TD-06 | Disk-space exhaustion untested | Should fix soon | Use a small loopback filesystem/container, not the host root. |
| TD-07 | No browser screenshot/visual or automated accessibility suite | Should fix soon | Screenshots blocked by the minimized preview window; no Playwright/axe in CI. Keyboard traversal not completed. |
| TD-08 | Real OS credential store never exercised | Should fix soon | Mock keychain only; smoke Linux Secret Service, macOS Keychain, Windows Credential Manager with a throwaway service name. |
| TD-09 | Real provider/search/MCP/LSP/debugger peers never tested | Should fix soon | All integration tests use local fixtures; vendor API drift is invisible. |
| TD-10 | `glib` 0.18.5 unsoundness + four unmaintained crates | Should fix soon | Requires GTK/Tauri upgrade, which needs Windows/macOS GUI regression testing. `cargo audit` reports 0 vulnerabilities. |
| TD-11 | Three moderate npm advisories in VS Code packaging toolchain | Accepted limitation | Dev-only (`@vscode/vsce`); production audit is 0. Upgrade needs Node ≥22 and re-smoking the VSIX. |
| TD-12 | Phase 7/8 performance reports contain an unexplained 14,476 KB idle-RSS figure | Should fix soon (documentation) | Phase 5 and Phase 10 both measure ~88–90 MB; the 14 MB figure is not reproducible and should not be quoted. |

## Accepted limitations

| ID | Limitation |
|---|---|
| TD-13 | Local single-user app: no accounts, sessions, CSRF or multi-tenant authorization; loopback/Host/Origin guards are the boundary |
| TD-14 | Prompt injection through files/skills/model output remains a model-level risk; tool actions are approval-gated |
| TD-15 | Skills are local Markdown instructions; no remote registry or signed skill distribution |
| TD-16 | Native Tauri window not launched in this headless environment; UI was verified in the browser against the same server |
| TD-17 | Android release signing is intentionally external; no physical-device run |
| TD-18 | No tray/notifications/deep links/self-update on Android |
| TD-19 | Per-message edit/delete/regenerate not implemented |
| TD-20 | Very large project trees beyond 1,000 files and threads beyond 1,000 messages were not profiled in Phase 10 |

## Deferred features

| ID | Feature | State |
|---|---|---|
| TD-21 | Remote/multi-user hosting | Not implemented; loopback + optional TLS Host API only |
| TD-22 | Skill marketplace/version registry | Not implemented; version compatibility metadata exists locally |
| TD-23 | Built-in browser automation/debug UI | Client modules exist; no live peer validation |

## Platform blocked

| ID | Item |
|---|---|
| TD-24 | Windows GUI/ConPTY/Credential Manager runtime |
| TD-25 | macOS Gatekeeper/Keychain/notarization runtime |
| TD-26 | Android production keystore, Play distribution, physical device |
| TD-27 | Linux `dpkg` install/upgrade/uninstall without root/dpkg |
| TD-28 | Public release publication (explicitly not authorized) |

## Not applicable

- Browser CSRF/session management for a remote account system: no such system exists.
- ARM64 desktop runtime: not a declared supported target.
- GPU performance: the headless server does not use a GPU.

## Debt created or changed in Phase 10

| Change | Direction |
|---|---|
| Watcher test race fixed with an index sentinel | Debt reduced |
| Corrupt config now backed up instead of lost | Debt reduced |
| README binary/env/path documentation corrected | Debt reduced |
| New `config.json.corrupt-*` files are never automatically cleaned | Small accepted operational note |
| Phase 10 reports add maintenance surface | Expected |

**No critical/high code defects remain open.** The release-blocking debt is signing, real-platform installation and live-update validation.
