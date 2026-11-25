# Sarthi Studio — Restoration Report

**Date:** 2026-11-24  
**Branch:** `remediation/security-2026-11-24`<br>
**Baseline:** 3.88.0 (`55d1218`)  
**Final:** 3.88.3<br>
**Approach:** restore the intended integration/behavior without reverting commits, replacing implementations, duplicating features or removing capabilities.

## Result in one paragraph

No shipped feature had been removed or replaced, so no Git revert was needed. Three behaviors were **disconnected from their intended implementation**: the loopback origin guard was not applied to the core router, recent-folder rendering bypassed the safe DOM path, and streaming metadata lost two independent context fields behind a skill condition. The remediation pass then restored two security integrations that existed elsewhere but were missing from the MCP path: the HTTPS-off-loopback policy and OS-keychain storage for saved header values. Each was restored at its original integration point and verified with real tests. All existing product surfaces remain; no file format, database schema, config key or route was removed.

## Restored items

### R-01 — Loopback origin protection for the core local router

**Original intended behavior:** every local UI route accepts only a loopback `Host` and, when an `Origin` is present, a loopback `Origin`. The guard already existed in `workspace.rs` and guarded workspace, terminal, media, agent, skills, memory, providers, index, drafts and router-console routes. Documentation and comments describe the app as loopback-only and DNS-rebinding-resistant.

**What was disconnected:** routes registered directly in `main.rs::bootstrap()` were not passed through the guard. This was not an intentional relaxation; it was an incomplete integration. The guard's own behavior and tests were intact.

**Why it stopped working:** module routers each added `route_layer` locally. Core routes were added directly to the outer router and never received that layer. There was no single enforcement point after the merger.

**Restoration method:**

- Added one `route_layer` after all module merges, before the security-header layer.
- Kept existing module guards as defense in depth.
- Did not change the Host API router, which intentionally uses a separate TLS/bearer model.
- Did not change allowed loopback hostnames or the configured port.

**Verification:**

- Foreign `Host` on `/api/config`: 403.
- Foreign `Origin` on `/api/capabilities`: 403.
- Normal loopback config/chat/workflow/file/terminal requests: 200 and functional.
- 61/61 real-binary e2e checks pass on debug and release.

**Backward compatibility:** loopback browser, Tauri webview and VS Code embed all use a loopback Host; no supported client lost access. The only behavior change is that a foreign Host/Origin is now refused, which is the documented security boundary.

### R-02 — Safe rendering of recent folders

**Original intended behavior:** the Workspaces page lists recent folders and clicking one opens it. Folder names are filesystem data, not markup. The rest of the UI consistently uses `textContent` for paths and names.

**What was disconnected:** `renderWorkspaces()` manually built a recent row with an `innerHTML` template containing the path. The feature still worked, but bypassed the safe text path used elsewhere.

**Restoration method:**

- Extracted `recentFolderRow(path)`.
- Built the row with `createElement`/`appendChild`.
- Assigned the label through `textContent` and the tooltip through the `title` property.
- Kept the same "Open" button and handler.

**Verification:** new frontend regression test with `<img src=x onerror="alert(1)">` as the path; the rendered element's `textContent` is exactly the hostile string and `innerHTML` is empty. Frontend suite 115/115.

**Backward compatibility:** identical visual structure/classes and identical click behavior; only the unsafe string assignment changed.

### R-03 — Streaming context metadata

**Original intended behavior:** a completed chat turn reports `memory_used` and `retrieved` in its meta whenever project memory or code retrieval contributed, independently of whether a skill auto-matched. The non-streaming path already did this.

**What was disconnected:** in both `chat_stream` branches, the two fields were nested inside `if !auto_skills.is_empty()`. That made the streamed path disagree with `chat` and with the UI's context meter.

**Restoration method:** un-nested the two checks in both the deep-search and normal streaming branches; no other metadata behavior changed.

**Verification:** new e2e creates a memory, streams a related message in a separate conversation, asserts `"memory_used"` in the SSE done frame, then deletes both. 61/61 pass. The pre-fix behavior would fail this test.

### R-04 — Remote MCP transport policy

**Original intended behavior:** remote integrations use TLS; plain HTTP is only for a server on this machine. The provider editor, extension network policy and VS Code MCP client already enforced this shape.

**What was disconnected:** the desktop MCP `valid_url` accepted HTTP for any host, and OAuth/SSE endpoints were not rechecked. The rule existed in the product elsewhere but was not applied at the MCP boundary.

**Restoration method:** one shared policy at save, connect, legacy-SSE endpoint, and OAuth discovery/registration/token/authorize. No new transport was introduced.

**Verification:** three Rust tests and two e2e checks; 61/61 on debug and release. Existing remote HTTP configurations now fail with an HTTPS message, which is the intended compatibility change.

### R-05 — MCP header secret storage

**Original intended behavior:** credentials use the OS credential store, as provider keys and desktop MCP OAuth tokens already did.

**What was disconnected:** saved custom header values were serialized into `mcp.json`; only OAuth material used the keychain.

**Restoration method:** write header values to the keychain, blank them in the registry, hydrate on read, migrate old values at startup, and keep the 0600 fallback if the store is unavailable. No header value is exposed through the API.

**Verification:** `header_values_live_in_the_keychain_not_the_registry`; full Rust suite 712 passed, 1 ignored. The test uses the project’s hermetic mock keychain; a live Secret Service round trip remains unverified.

**Backward compatibility:** existing servers keep working. On first launch their plaintext header values are migrated; if the credential store is unavailable, the 0600 file fallback preserves the value. The public server view still lists header names only.

## What was deliberately not restored

| Item | Why |
|---|---|
| Per-message edit/delete/regenerate | No prior implementation or documented route found in the current tree. Adding it would be a new feature with history/prompt semantics, not a restoration. |
| ~~Remote MCP HTTPS-only policy~~ | Restored in 3.88.2 (R-04). |
| ~~`cargo-audit` scanning~~ | Run in 3.88.2; 0 vulnerabilities, 5 documented warnings. |
| Windows/macOS/Android runtime validation | No hosts/devices/SDKs available. The source and CI definitions remain intact. |
| Signed installers | Requires external certificates/accounts and platform runners; not a source restoration. |
| Browser layout suite | No browser-layout test ever existed; adding a real one is new verification infrastructure, not restoration. |

No commit was reverted. No working implementation was overwritten. No duplicate feature was introduced. No placeholder or mock was substituted for a production integration.

## Data and configuration preservation

- No new database schema migration was added by this work, and no schema version was changed for the remediation.
- No existing table, column, config key or data path was renamed or removed.
- All tests used `mktemp`/scratch `FREE_AI_STUDIO_HOME` roots and cleaned up (or were kept only with explicit `E2E_KEEP=1`).
- No test intentionally used the real user database, config, workspace or credentials.
- One earlier verification command (`target/debug/free-ai-studio --version`) accidentally started the app against the real home because the binary had no version flag. It was stopped by the command timeout; `config.json` kept its 2026-11-22 mtime. **3.88.3 fixes the cause:** `--version`/`--help` now print and exit before any path is resolved, with built-binary tests and a package-payload check that asserts no home is created.
- No uninstall, swap, rollback or other destructive operation was run against real data.

## Verification that no functionality was lost

| Surface | Check |
|---|---|
| UI load | Live Chromium: 200, all startup fetches 200, no console errors |
| Chat | Live UI turn through mock provider returned `Mock router reply…`; conversation stored |
| Streaming | Real six-delta SSE, one stored reply; one-shot fallback also covered |
| Stop generation | Timed abort keeps one partial reply; studio answers afterwards |
| Settings persistence | Saved through `/api/config`, read back after restart |
| Conversations | Create/list/messages/delete/restart persistence |
| Projects | Create/rename/list/delete |
| Memory | Create/list/recall/delete |
| Workflows | Save/export/delete |
| Workspace | Create/save/read/rename/delete; traversal refused |
| Terminal | Real PTY command wrote a file |
| Agent | Approval, write, verification, restart, interruption |
| Security headers | Present on live response |
| Release binary | Same 61 e2e checks pass |
| Package payload | `.deb` unpacked, booted and replaced with data preserved (14/14) |
| Frontend | 115/115 |
| Rust | 712 passed, 1 ignored |

## Final statement

The remediation **restored the intended security and reporting connections** without redesigning the application. The feature set at 3.88.3 is a superset of 3.88.0's tested behavior, with no intentional capability removed. Phase 3 added real-SSE, stop-generation, settings-persistence, startup-CLI and package-payload verification rather than changing product behavior. Remaining gaps are external, documented product scope, or verification breadth, not silently deleted features.
