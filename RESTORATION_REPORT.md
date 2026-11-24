# Sarthi Studio — Restoration Report

**Date:** 2026-11-24  
**Branch:** `audit/full-application-2026-11-24`  
**Baseline:** 3.88.0 (`55d1218`)  
**Final:** 3.88.1  
**Approach:** restore the intended integration/behavior without reverting commits, replacing implementations, duplicating features or removing capabilities.

## Result in one paragraph

No shipped feature had been removed or replaced, so no Git revert was needed. Three behaviors were **disconnected from their intended implementation**: the loopback origin guard was not applied to the core router, recent-folder rendering bypassed the safe DOM path, and streaming metadata lost two independent context fields behind a skill condition. Each was restored at its original integration point and verified with real tests. All existing product surfaces remain; no file format, database schema, config key or route was removed.

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
- 51/51 real-binary e2e checks pass on debug and release.

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

**Verification:** new e2e creates a memory, streams a related message in a separate conversation, asserts `"memory_used"` in the SSE done frame, then deletes both. 51/51 pass. The pre-fix behavior would fail this test.

**Backward compatibility:** additive metadata; no consumer is required to handle it, and existing fields are unchanged.

## What was deliberately not restored

| Item | Why |
|---|---|
| Per-message edit/delete/regenerate | No prior implementation or documented route found in the current tree. Adding it would be a new feature with history/prompt semantics, not a restoration. |
| Remote MCP HTTPS-only policy | Present code intentionally accepts local HTTP; changing it can invalidate existing user configurations and requires a compatibility decision. It is reported as a residual security risk, not silently altered. |
| `cargo-audit` scanning | Tool unavailable; no dependency upgrade or suppression was invented. |
| Windows/macOS/Android runtime validation | No hosts/devices/SDKs available. The source and CI definitions remain intact. |
| Signed installers | Requires external certificates/accounts and platform runners; not a source restoration. |
| Browser layout suite | No browser-layout test ever existed; adding a real one is new verification infrastructure, not restoration. |

No commit was reverted. No working implementation was overwritten. No duplicate feature was introduced. No placeholder or mock was substituted for a production integration.

## Data and configuration preservation

- No database schema migration was added by this audit, and no schema version was changed.
- No existing table, column, config key or data path was renamed or removed.
- No real user database, config file, workspace or credential was modified.
- All tests used `mktemp`/scratch `FREE_AI_STUDIO_HOME` roots and cleaned up (or were kept only with explicit `E2E_KEEP=1`).
- The audit did not run uninstall, swap, rollback or other destructive operations against real data.

## Verification that no functionality was lost

| Surface | Check |
|---|---|
| UI load | Live Chromium: 200, all startup fetches 200, no console errors |
| Chat | Live UI turn through mock provider returned `Mock router reply…`; conversation stored |
| Streaming | e2e delta + done + one stored reply |
| Conversations | Create/list/messages/delete/restart persistence |
| Projects | Create/rename/list/delete |
| Memory | Create/list/recall/delete |
| Workflows | Save/export/delete |
| Workspace | Create/save/read/rename/delete; traversal refused |
| Terminal | Real PTY command wrote a file |
| Agent | Approval, write, verification, restart, interruption |
| Security headers | Present on live response |
| Release binary | Same 51 e2e checks pass |
| Frontend | 115/115 |
| Rust | 704/704 (1 ignored) |

## Final statement

The audit **restored the intended security and reporting connections** without redesigning the application. The feature set at 3.88.1 is a superset of 3.88.0's tested behavior, with no intentional capability removed. Remaining gaps are external, documented product scope, or verification limitations, not silently deleted features.
