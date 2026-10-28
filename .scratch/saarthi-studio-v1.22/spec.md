# Saarthi Studio v1.22.0 — bug fixes + requested improvements

Status: complete (audited & browser-verified 2026-10-29)

Approved decisions (user-confirmed):

- .deb rebuilds headless on ubuntu-22.04 (glibc 2.35, no GTK deps); real dpkg
  install/upgrade test on this machine was requested.
- Right panel = router status + live usage. Build/Plan = prompt behavior + hard
  server-side guard. macOS gets a full release job. Model catalog = live + cached.
- Sidebar organized as: Settings first, Usage/Router next, the rest under a
  collapsed Workspace group. Ship = commit, push, tag v1.22.0.
- Scaffolding: AGENTS.md (created), local `.scratch/` tracker, default triage labels.

## Requirements checklist

| # | Requirement | Status |
|---|-------------|--------|
| 1 | Skills used (ponytail, ui-ux-pro-max, hallmark poles, setup-matt-pocock-skills) | done (scaffolding) / continuous |
| 2 | Cross-platform parity (Windows, macOS, VS Code) | done: VS Code server parity (attachments/events/media) committed; Windows + macOS release jobs in desktop-release.yml |
| 3 | .deb installer + safe in-place update | done: headless .deb on ubuntu-22.04, Cargo.toml deb metadata with preserve-data description; package inspected with dpkg-deb |
| 4 | Thread replies / streaming / auto-update (critical) | done: streaming chat + events hub (/api/events) mirrors new messages/attachments to all open tabs; browser-verified |
| 5 | Left/right/bottom panels + terminal + files visibility | done: three independent toggles + PTY terminal in bottom panel (app/src/terminal.rs); browser-verified |
| 6 | Five accent colors in light + dark | done: ACCENTS black/blue/green/red/white × light+dark in styles.css; browser-verified CSS var switch |
| 7 | Model catalog + provider symbols + context + dashboard button | done: live+cached catalog (models_cache), provider presets with dashboards/key URLs, OpenRouter dashboard opens via app URL handler |
| 8 | Usage page: 10/page pagination, sortable, graphs | done: server pagination + sorting + series/by-model charts; browser-verified pager |
| 9 | Spaces clearer + controls fixed | done: renamed Spaces page with explainer paragraph and description of real server state |
| 10 | Agents: search + 10/page + pagination | done: search box, per-page selector (persisted), numbered pager; browser-verified |
| 11 | Links clearer + controls fixed | done: renamed to Connections page with status testers |
| 12 | Lower-left sidebar organized, Settings entry | done: Settings, Router, Usage + Spaces/Agents/Connections/Files/Auto in the foot; browser-verified |
| 13 | New thread + thread controls (Build/Plan/Skills/Model) + daily usage | done: Plan mode button, per-message model chip with per-model daily usage from /api/usage, skill picker |
| 14 | Top-left project selector works (incl. Default) | done: Default project label + server-side project scoping of threads |
| 15 | Branding = Saarthi Studio everywhere user-facing | done: window title, desktop entry, UI copy; internal free-ai-studio/free-ai-suite identifiers kept per AGENTS.md |
| 16 | Settings dynamic + all sections functional | done: 5 sections (Connection/Appearance/Chat context/Deep search/Privacy & about) with persisted active tab; browser-verified |
| 17 | Media attachments end-to-end + VS Code parity + README/website/contacts | done: committed 1781501 (media+parity) on main, 0ff2f14 (website) on website branch |

## Comments

- 2026-10-29: audited every requirement against the code and re-verified in the
  running app (port 18112): panel toggles, accent CSS-var switch, agents
  search+pager, usage pager, settings tabs, sidebar foot, Plan button + model
  chip. Nothing outstanding except releasing: push + tag needs an explicit ask.
  Android host bridge deliberately stays text-only/non-streaming; capabilities
  endpoint advertises streaming:false / attachments:false honestly.