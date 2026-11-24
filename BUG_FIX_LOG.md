# Sarthi Studio — Bug Fix Log

**Audit date:** 2026-11-24  
**Branch:** `audit/full-application-2026-11-24`  
**Baseline:** `55d1218` (3.88.0)  
**Fixed release:** 3.88.1  
**Rule followed:** reproduce first, fix the cause, add a regression test, rerun the affected suite. No test was skipped or weakened to obtain a green result.

## Summary

| ID | Severity | Bug | Status | Regression coverage |
|---|---|---|---|---|
| BUG-01 | P1 / High | DNS rebinding reached core local routes | Fixed | 2 e2e checks + full suite |
| BUG-02 | P2 / Medium | Recent-folder name injected into `innerHTML` | Fixed | frontend unit test + frontend suite |
| BUG-03 | P3 | Stream metadata hid memory/retrieval unless a skill matched | Fixed | new e2e memory-stream check |

## BUG-01 — Core UI routes were outside the loopback origin guard

### Description

A request carrying a foreign `Host` (the DNS-rebinding shape) could read `/api/config` and reach other routes registered directly in `main.rs`, including chat, conversations, projects, workflows, extensions, update and uninstall. The same request against `/api/workspace` was correctly refused with 403.

### Reproduction

Against a live 3.88.0 binary on `127.0.0.1:8899`:

```bash
curl -H 'Host: evil.example:8899' http://127.0.0.1:8899/api/config
# HTTP/1.1 200 OK
# {"adaptive_routing":false,...,"base_url":"http://localhost:3001/v1",...}

curl -H 'Host: evil.example:8899' -H 'Origin: http://evil.example:8899' \
  http://127.0.0.1:8899/api/capabilities
# HTTP/1.1 200 OK

curl -H 'Host: evil.example:8899' http://127.0.0.1:8899/api/workspace
# HTTP/1.1 403 Forbidden
# {"error":"This endpoint only accepts loopback requests."}
```

### Root cause

`workspace::router`, `terminal::router`, `media::router` and other module routers applied `workspace::guard_local_origin` via `route_layer`. Routes built directly in `main.rs::bootstrap()` were never wrapped. The guard implementation was correct; its integration was incomplete.

### Affected components

- `app/src/main.rs` — local router construction.
- All directly registered core routes, including config, chat, chat stream, conversations, projects, workflows, extensions, update, uninstall and static serving.
- Existing module routers were already guarded and provided defense in depth.

### Fix

Added one outer guard after every router is merged:

```rust
.merge(router_console::router(port))
.merge(router_api)
.route_layer(middleware::from_fn_with_state(
    workspace::LocalOriginGuard::new(port),
    workspace::guard_local_origin,
))
.layer(middleware::from_fn(local_security_headers))
```

Why this shape:

- It covers routes added to this router later without maintaining a second route list.
- It runs before the response body and before state-changing handlers.
- It leaves the Host API (a separate TLS router with bearer authentication) untouched.
- It does not change the Android Host API, which has its own security model.

### Tests and results

Added to `scripts/ci/test-e2e.sh`:

```bash
# Foreign Host on /api/config -> 403 with the loopback message
# Foreign Origin on /api/capabilities -> 403 with the cross-site message
```

Before the fix, the same live checks returned 200. After the fix:

- Debug binary e2e: **51 passed, 0 failed**
- Optimized release e2e: **51 passed, 0 failed**
- Foreign Host/Origin checks both 403
- Normal loopback UI and mock-provider chat still 200

### Files modified

- `app/src/main.rs`
- `scripts/ci/test-e2e.sh`
- `CHANGELOG.md`
- `docs/KNOWN-ISSUES.md` (#48)
- version/metadata files

### Verification

`SECURITY_AUDIT_REPORT.md` SEC-01, `FEATURE_TEST_MATRIX.md` security rows, `APPLICATION_AUDIT_REPORT.md`.

## BUG-02 — Recent-folder name was parsed as HTML

### Description

The Workspaces page built each recent-folder row with a template literal assigned to `innerHTML`:

```js
row.innerHTML = `<span class="project-menu-main"><small title="${path}">${path}</small></span>`;
```

A folder whose name contains HTML would inject markup into the local app origin. The CSP's `script-src 'self'` blocks inline handlers, so this was not a straightforward script-execution primitive, but it is a genuine injection defect and a latent risk if CSP or markup changes.

### Reproduction

A unit-level reproduction through the frontend harness with:

```js
const hostile = '<img src=x onerror="alert(1)">';
```

The old code passed that string to `innerHTML`; the new test checks the rendered `<small>` element has `textContent === hostile` and `innerHTML === ""`.

### Root cause

Filesystem-controlled text was concatenated into an HTML string instead of being assigned as a text node. The `title` attribute was interpolated at the same time, allowing attribute injection.

### Affected components

- `app/static/app.js`, `renderWorkspaces()` / new `recentFolderRow()`
- Project menu recent folders (Workspace page)

### Fix

Added `recentFolderRow(path)`, which uses `document.createElement`, `textContent` for the label and a property assignment for `title`. `renderWorkspaces()` appends that row. No HTML string contains the path.

### Tests and results

- New test: `a recent folder name is rendered as text, never as markup`
- Frontend suite: **115 passed, 0 failed** (was 114)
- The test fails against the old interpolation and passes against the DOM construction.

### Files modified

- `app/static/app.js`
- `app/static/tests/render.test.js`
- `CHANGELOG.md`

### Verification

The live UI loaded the same code with no console errors; this specific row is covered by the browser-free harness rather than a screenshot.

## BUG-03 — Streaming chat metadata omitted memory/retrieval unless a skill matched

### Description

When a streamed reply used project memory or code retrieval, the done metadata did not include `memory_used` or `retrieved` unless at least one skill had auto-matched. The reply's context meter therefore under-reported why the answer was shaped the way it was.

### Root cause

In `chat_stream`, both the deep-search branch and the main router branch nested the two independent fields inside:

```rust
if !auto_skills.is_empty() {
    held["auto_skills"] = ...;
    if !memory_used.is_empty() { ... }
    if !retrieved.is_empty() { ... }
}
```

`memory_used` and `retrieved` have no logical dependency on `auto_skills`. The non-streaming `chat` path already handled them independently, which made the discrepancy clear.

### Affected components

- `app/src/main.rs`, `chat_stream` deep-search branch and normal stream branch.
- The streaming meta shown by the UI (`applyStreamMeta`) and stored by `ReplyRecorder`.

### Fix

Moved both checks out of the `auto_skills` block in both places. Each field is now emitted independently when non-empty.

### Regression test

`scripts/ci/test-e2e.sh` now:

1. Creates a real memory record (`title: e2e memory`, body: `the audit marker`).
2. Creates a separate conversation.
3. Streams a message that shares the memory's words.
4. Asserts the SSE body contains `"memory_used"`.
5. Deletes the conversation and the memory record.

This test would fail under the old nesting. Result: **51/51 e2e checks pass** on debug and release.

### Files modified

- `app/src/main.rs`
- `scripts/ci/test-e2e.sh`
- `CHANGELOG.md`

## Test-suite changes made for coverage (not bug fixes)

The real-binary suite grew from 34 to 51 checks. New coverage:

- foreign `Host` and `Origin` refusal;
- streaming chat produced a delta/done frame and stored one reply;
- conversation token-budget update and deletion cascade;
- config key redaction;
- project create/rename/list/delete;
- memory create/list/delete and streaming recall metadata;
- workflow save/export/delete;
- skill catalogue;
- workspace file create/save/read/rename/delete;
- workspace path traversal refusal;
- interactive terminal PTY command that writes a file;
- all prior lifecycle/provider/restart/corruption checks.

These are permanent tests, use scratch homes/databases, clean up after themselves, and do not contain real credentials.

## Regression status after repairs

| Check | Result |
|---|---|
| Rust suite | 704 passed, 0 failed, 1 ignored |
| Frontend suite | 115 passed, 0 failed |
| Real-binary e2e (debug) | 51 passed, 0 failed |
| Real-binary e2e (release) | 51 passed, 0 failed |
| VS Code extension | 45 passed, 0 failed |
| Clippy (`-D warnings`) | Pass |
| rustfmt check | Pass |
| Website build | 37 pages |
| Offline release/install gates | Pass (MSI skipped, no WiX) |

## Not a code bug / not fixed

- **No per-message edit/delete/regenerate**: absent by product design, not a regression; reported in the matrix.
- **Bodyless POST to the conversation delete route** showed a 405 in one early harness iteration while the same route answered with an explicit `{}` body. The e2e now sends the client-shaped `{}` request; the route was not re-tested bodyless afterwards, and no application defect was reproduced.
- **Remote MCP HTTP** remains permitted for compatibility; reported as SEC-03 rather than silently changing configuration semantics.
