# Roadmap (2026-10-29) — approved for this round

Order: every milestone verified before the next. Parking lot at the bottom.

## M1 — Audit + persistent state ✅ (this directory)
## M2 — Privacy pass (shared core)
`privacy.rs`: secret scanner (generic API keys, bearer tokens, AWS creds, PEM
blocks, JWTs) + per-workspace exclusion list in config. Wire into context
packing + agent read step. Port scan into VS Code server. Blocked sends →
visible redaction notice. Tests per pattern. Acceptance: chat with a
secret-bearing/excluded file shows the notice and content never leaves.
## M3 — VS Code agent parity
lib.ts: /api/runs contract (list/create/detail/approve/cancel + 404-tolerant
detail), Runs webview panel mirroring app/static/app.js run UI (goal input,
perm checkboxes, diff-colored step log, approve/reject/cancel/restore).
Acceptance: approve a write inside VS Code, see diff, restore.
## M4 — Android parity (two half-milestones)
M4a DesktopHostTransport → streaming SSE host chat through the existing
native-emitter bridge. M4b agent runs: transport routes + Runs screen
(step log, diffs, approve/reject/cancel/restore), capability discovery via
/api/host/v1/capabilities for graceful degradation. Acceptance: approve a
write from the phone, see its diff, restore; token-by-token streaming.
## M5 — macOS packaging
macOS universal dmg job in the release pipelines reusing existing packaging
conventions; CI build + smoke. Signing/notarization = documented blocker (no
credentials), not faked.
## M6 — Adaptive routing + reproducible runs
Opt-in `adaptiveRouting` (off default): rule-based tier choice among
already-configured providers (cheap: triage/summary; strong: plan/write/
repair), visible failover, choice recorded per run. agent_runs gains
model/router_config columns (additive migration). Acceptance: adaptive choice
logged; flag off = current behavior byte-identical.

## Parking lot (explicitly not this round)
Git branch/worktree isolation (user chose in-place + per-file restore);
Living Project Brain / digital twin / impact map; capability store; website
platform-pages rework.
