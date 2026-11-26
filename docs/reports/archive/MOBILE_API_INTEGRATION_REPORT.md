# Mobile API Integration Report

Date: 2026-11-26  
Branch: `mobile/complete-redesign-2026-11-26`

## Existing transports

The Android client keeps its two real transports:

1. **Direct Router** — `DirectRouterTransport` maps OpenAI-compatible
   `/v1/models` and `/v1/chat/completions` (SSE streaming).
2. **Desktop Host** — `DesktopHostTransport` maps operations under
   `/api/host/v1/` and uses the host token, not the router key.

No API key or host token is embedded, logged, or moved to browser storage.

## New Host API surface

The Host API only had chat, conversations, messages, models and agent runs. The
following routes were added in `app/src/host_api.rs` by reusing the existing
desktop handlers:

| Method | Route | Purpose |
|---|---|---|
| GET/POST | `/api/host/v1/projects` | List/create projects |
| POST | `/api/host/v1/projects/:id/activate` | Switch active workspace |
| GET | `/api/host/v1/skills` | List installed skills |
| POST | `/api/host/v1/skills/:id/state` | Enable/disable a skill |
| GET | `/api/host/v1/workflows` | List workflows |
| POST | `/api/host/v1/workflows/:id/run` | Start a workflow |
| GET | `/api/host/v1/workflow-runs` | Recent runs (`limit` query) |
| GET | `/api/host/v1/workflow-runs/:id` | Run plus `stepList` |
| POST | `/api/host/v1/workflow-runs/:id/cancel` | Cancel a live run |
| GET | `/api/host/v1/extensions` | Extension inventory |
| GET/POST | `/api/host/v1/memory` | List/save memory |
| POST | `/api/host/v1/memory/:id/delete` | Delete a memory |

Capabilities now advertise `projects`, `skills`, `workflows`, `extensions` and
`memory` in addition to the existing flags. `attachments` remains `false` on the
Host API because the host chat contract does not accept them; the phone shows
that limitation rather than pretending.

## Security

- All new routes are merged into the existing host router, which applies the
  Host bearer middleware and request body limit to every v1 route.
- Reused desktop handlers keep their existing validation, path handling and
  database access.
- The new test `workspace_surfaces_require_the_host_token_and_are_reachable`
  proves every new route returns 401 without a token and 200 with the valid
  token.
- No route exposes filesystem contents, arbitrary paths or a generic SQL proxy.
- Project activation writes the same active-project state the desktop uses;
  project creation accepts only a name and a folder string, matching the
  desktop contract.

## Android request flow

- `hostRequest(method, path, body)` wraps `request()` with the host endpoint and
  token.
- All calls run on the dedicated `executor`; UI updates are posted to `main`.
- HTTP failures use the existing `errorText()`/`friendlyError()` sanitisation.
- JSON parsing is defensive (`optJSONArray`, `optJSONObject`, empty states).
- Project ids, skill ids, workflow ids, run ids and memory ids are URL-encoded
  with `Uri.encode` before use in a path.

## Endpoint shapes consumed by the phone

- Projects: `{ok, active, projects:[{id,name,folder,model,...}]}`
- Skills: `{ok, skills:[{id,name,description,version,source,enabled,compatible,requires}]}`
- Workflows: `{ok, workflows:[{id,name,nodes,skill,everyMinutes,...}]}`
- Runs: `{ok, runs:[{id,name,status,steps,succeededSteps,error,...}]}`
- Run detail: `{ok, run:{...,stepList:[{title,status,output,...}]}}`
- Extensions: `{ok, directory, extensions:[{id,name,version,runtime,valid,compatible,...}]}`
- Memory: `{ok, project, records:[{id,kind,title,body,global,...}]}`

## Tests

`cargo test host_api::tests` — **18 passed, 0 failed**. The suite includes
authentication, allow-list, malformed body, large body, model list, conversation
paging, agent runs and the new workspace surfaces.

## Known API limitations

- Host chat still does not accept attachments.
- No host route exists for project file browsing, previews, git status, agent
  creation, workflow authoring, skill installation or extension execution.
- Those gaps are documented as mobile limitations rather than replaced with
  fabricated data.
