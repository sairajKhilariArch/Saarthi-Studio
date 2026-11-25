# Mobile Feature Parity Audit — Saarthi Studio Android

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`  
Compared: desktop application in `app/src` (Rust/Tauri) vs Android client in `android/`.

## Method

Every Android screen and every desktop Host API route was inspected. A feature is
marked **FULLY IMPLEMENTED** only when the mobile control reaches real code and
real data (local SQLite, the authenticated Desktop Host API, or the configured
router). Visible buttons without behaviour, hardcoded sample data and fake
success messages were treated as failures.

The Android app is a programmatic single-activity client (`MainActivity.java`).
It has two transports:

- **Direct Router** — OpenAI-compatible `/v1` endpoint.
- **Desktop Host** — authenticated `/api/host/v1/` API on the desktop app.

Desktop-only operations (native terminal, arbitrary filesystem access, native
debugging) are intentionally not reproduced on the phone and are listed as such.

## Feature matrix

| Feature | Desktop | Android before | Android now | Status |
|---|---|---|---|---|
| New conversation | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Conversation list and switch | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Search titles/messages | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Rename/delete conversation | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Pin/favorite/folder/tags | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Streaming responses | Yes | Yes | Yes, SSE | FULLY IMPLEMENTED |
| Stop generation | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Regenerate/retry | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Edit and resend | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Copy message/response | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Markdown rendering | Full | Subset | Subset | PARTIALLY IMPLEMENTED |
| Syntax-highlighted code + labels | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Code block copy | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Text/code attachments | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Image attachments | Yes | Yes | Yes (vision models) | FULLY IMPLEMENTED |
| Model selection | Yes | Settings only | Header model selector + Settings | FULLY IMPLEMENTED |
| Provider selection | Multiple profiles | One Direct Router + Host | One Direct Router + Host | PARTIALLY IMPLEMENTED |
| API key configuration | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Model discovery | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Favorite/fallback models | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Provider connection test | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Provider health/diagnostics | Yes | Host diagnostics | Host diagnostics | PARTIALLY IMPLEMENTED |
| Voice dictation | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Text-to-speech | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Prompt presets | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Usage/budget panel | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Agent run list | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Agent run detail/approve/cancel | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Agent creation/configuration | Yes | No | No | NOT IMPLEMENTED |
| Agent model/skill assignment | Yes | No | No | NOT IMPLEMENTED |
| Project list | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Create/activate project | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Project files/preview/git | Yes | No | No | NOT SUPPORTED ON MOBILE |
| Skills list | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Enable/disable skill | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Install/update/remove skill | Yes | No | No | PARTIALLY IMPLEMENTED |
| Skill detail/version/error | Yes | No | Version/compatibility shown | PARTIALLY IMPLEMENTED |
| Skill assignment to agent/workflow | Yes | No | No | NOT IMPLEMENTED |
| Workflow list | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Run/cancel workflow | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Workflow run status | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Workflow run detail/log steps | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Workflow create/edit/schedule | Yes | No | No | NOT IMPLEMENTED |
| Memory list | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Add/delete memory | Yes | No | Yes (Host) | FULLY IMPLEMENTED |
| Edit memory | Yes | No | No | PARTIALLY IMPLEMENTED |
| Extensions inventory | Yes | No | Yes (Host) | PARTIALLY IMPLEMENTED |
| Extension install/configure/run | Yes | No | No | NOT SUPPORTED ON MOBILE |
| MCP/plugin tool execution | Yes | No | No | NOT SUPPORTED ON MOBILE |
| Dark theme | Yes | Yes | Yes | FULLY IMPLEMENTED |
| Light theme | Yes | No | Yes, persisted | FULLY IMPLEMENTED |
| Bottom navigation | Desktop nav | No | 5 destinations | FULLY IMPLEMENTED |
| Backup/export/import | Yes | Yes | Yes | FULLY IMPLEMENTED |
| About/version | Yes | No | Yes | FULLY IMPLEMENTED |
| Notifications | Desktop | No | No | NOT IMPLEMENTED |
| Terminal/filesystem/native debugging | Yes | No | No | NOT SUPPORTED ON MOBILE |

## Real backend work added for parity

The desktop Host API previously exposed only health, capabilities, models,
conversations, messages, chat and agent runs. That made Projects, Skills,
Workflows and Memory impossible to implement honestly. The authenticated host
router now also exposes, using the existing desktop handlers:

```
GET/POST /api/host/v1/projects
POST     /api/host/v1/projects/:id/activate
GET      /api/host/v1/skills
POST     /api/host/v1/skills/:id/state
GET      /api/host/v1/workflows
POST     /api/host/v1/workflows/:id/run
GET      /api/host/v1/workflow-runs
GET      /api/host/v1/workflow-runs/:id
POST     /api/host/v1/workflow-runs/:id/cancel
GET      /api/host/v1/extensions
GET/POST /api/host/v1/memory
POST     /api/host/v1/memory/:id/delete
```

Every route is behind the existing Host bearer wall and body limit. No mobile
screen invents local substitutes when a host is not configured: it shows a
Desktop-connection requirement instead.

## Remaining gaps (honest classification)

1. Provider profiles are still one Direct Router configuration plus one Host.
   Desktop supports more provider configuration than this.
2. Agent creation/configuration and skill assignment are desktop-only.
3. Workflow authoring and schedule editing are desktop-only; run, cancel and
   log inspection are available on mobile.
4. Project file browsing, previews and git status are not exposed by the Host
   API and are not invented on mobile.
5. Skill installation/update/removal and extension execution remain desktop
   operations.
6. Markdown is a dependency-free subset, not a full CommonMark renderer.
7. Notifications and background scheduled-run alerts are not implemented.

## Verdict

Mobile now has **high practical parity for daily use**: chat, history and
organisation, model choice, attachments, voice, agents, projects, skills,
workflows, memory, extensions inventory, themes and backup. It does **not**
claim 100% desktop parity; the gaps above are real and deliberately documented.
