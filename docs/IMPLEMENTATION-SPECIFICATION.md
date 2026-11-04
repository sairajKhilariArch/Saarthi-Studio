# Implementation specification (adopted)

This is the engineering contract for Saarthi Studio. It states the rules every
phase must follow; the phase-by-phase work is in `docs/MASTER-ROADMAP.md` and
the current state in `docs/FEATURE-STATUS.md`.

## 1. Principle

**Functionality before appearance.** A feature is real only when the shipped UI
reaches the backend, the backend reaches the actual service or file, the result
persists, and a test or a recorded run proves it. A route returning `200`, a
rendered panel, or a hardcoded success string is not implementation.

## 2. Core ownership

| Concern | Owner | Never duplicated in |
|---|---|---|
| App lifecycle, listener setup | `app/src/main.rs` | clients |
| Agent execution, permissions, checkpoints | `app/src/agent.rs` | clients (VS Code keeps a port for the embedded-server mode, which must track the Rust contract) |
| Router lifecycle + ownership | `app/src/router.rs` | clients |
| Provider calls, model discovery | `app/src/main.rs` (+ `local.rs`, `deepsearch.rs`) | clients |
| Workspace file access | `app/src/workspace.rs` | clients |
| Skills registry | `app/src/skills.rs` | clients |
| Terminal PTYs | `app/src/terminal.rs` | clients |
| Remote API for companions | `app/src/host_api.rs` | clients |

Android and VS Code talk to these surfaces. They do not reimplement business
logic that changes persistent state.

## 3. API contracts

1. Every new route lives in the module that owns the concern and is registered
   in that module's `Router` builder — do not grow `main.rs` with unrelated
   handlers.
2. Requests are validated before any side effect: body caps, path confinement,
   enum allowlists, numeric ranges. Reject with a specific message and a
   correct status code (400/403/404/409/413/415/422/429).
3. Mutating routes keep the loopback `Host`/`Origin` guard; remote routes go
   through the Host API router with a bearer token.
4. Long work is asynchronous, cancellable, and streams progress over SSE.
5. Responses that carry user data are `no-store`.
6. Breaking a response shape requires a version bump of the Host API and a
   compatibility shim for one release.
7. Secrets never appear in responses, logs, or the frontend.

## 4. Persistence rules

1. SQLite migrations are additive: `CREATE TABLE IF NOT EXISTS`, `ALTER TABLE
   ADD COLUMN`, never a destructive rewrite of user data.
2. New tables carry the columns the UI needs to render history without a
   second query, and an index for the query the UI actually runs.
3. Config additions are optional with defaults; a missing key never fails
   startup.
4. Paths stay `~/.config/free-ai-studio/config.json` and
   `~/.local/share/free-ai-studio/studio.db`.
5. A crash must leave an honest record (agent runs become `interrupted`), not
   a silently resumed half-run.

## 5. Frontend rules

1. No build step, no bundler, no package manager for `app/static/`. Hand-written
   ES modules only.
2. New styling goes through the CSS custom-property tokens in `styles.css`.
3. The IDE shell stays additive (`ide.js`, `ide.css`) and must not break the
   base UI when its assets fail to load.
4. UI state reflects backend truth; never optimistically show "done" before the
   backend confirms.

## 6. Dependency policy

Before adding a crate/package/executable: (1) is there an equivalent already in
the tree, (2) is it maintained and safe, (3) does it build on Linux, Windows,
macOS and stay buildable under `--no-default-features`, (4) why is it needed,
(5) record it in the changelog. Duplicate capability libraries are rejected.

## 7. CI and release contract (Phase 1)

1. One version parser for all OSes: `scripts/ci/version.sh`. No inline `sed` in
   workflows.
2. Version output must be non-empty and match `MAJOR.MINOR.PATCH[-pre|+build]`.
3. Artifact basenames are validated by `scripts/ci/verify-artifacts.sh` before
   upload: no control characters, no spaces, restricted charset, at least one
   artifact, and `SHA256SUMS.txt` verified when present.
4. Release runs never overlap (`concurrency`), so two tags cannot race a single
   asset name.
5. Pipeline regressions get a test in `scripts/ci/test-version.sh` (or a Rust
   test for backend behaviour) that fails on a plain `cargo test`/CI push, not
   only during a release.
6. A release is "shipped" only when the tag run is green and the assets are
   attached and verified.

## 8. Cross-platform contract

- Desktop Linux/Windows/macOS build from the same source with no
  platform-specific feature loss in the server.
- Android never promises local execution it cannot do; remote operations go
  through the Host API.
- VS Code prefers its native terminal/git where available.
- Feature gates are driven by the Host API `capabilities` response.

## 9. Testing contract

1. Every meaningful change adds or updates a test: Rust unit/integration for
   backend logic, `node --test` for extension logic, JVM/instrumentation for
   Android, pipeline tests for workflow logic.
2. Failure paths are tested (invalid input, missing file, denied permission,
   provider error), not only the happy path.
3. Nothing is reported as verified without the command, its output, and the
   date it was run.

## 10. Definition of done (per feature)

1. Backend implemented in the owning module.
2. Frontend reachable and wired to the real route.
3. Persistence where the feature implies state.
4. Errors handled with actionable messages.
5. Permissions enforced at the route.
6. Tests added and passing.
7. Docs updated (`FEATURE-STATUS.md`, `PROGRESS.md`, changelog).
8. Platform impact assessed (Linux/Windows/macOS/Android/VS Code).
9. End-to-end behaviour demonstrated (recorded command or run).
10. No working feature removed without a documented replacement.

## 11. Working agreement for agents

- Audit before editing; reuse before writing.
- Small, reviewable changes; never reformat unrelated files.
- Preserve line endings (LF) — a CRLF rewrite of manifests already broke a
  release once.
- One version bump per shipped commit (repo convention), changelog entry
  included.
- Report failed or unrun checks as limitations, never as passes.
