# Pipelines and the workflow engine

A pipeline is a mission with a fixed, analysis-first plan. It is created through
`multi_agent::create_mission_with`, so it inherits the supervisor instead of replacing it.

## The plan

| Stage | Role | Depends on | Writes |
| --- | --- | --- | --- |
| `analyse` | architect | — | no |
| `plan` | planner | analyse | no |
| `implement` | coding | plan | yes |
| `verify` | testing | implement | no |
| `document` | documentation | implement | yes |
| `review` | review | verify, document | no |

The analyse stage is given the workspace facts the scanner measured (counts, languages,
frameworks, manifests, declared dependencies) and is told to report the build and test
commands, the structure, and the risks. Later stages read the summaries of the tasks they
depend on, so the plan is written against the analysis, not against the prompt alone.

Parallelism is 2: `verify` and `document` may run together because one is read-only, and the
supervisor's writer rule still applies to any two tasks that could touch the same file.
Retries are 1: a failed stage is retried once as a **fresh run** with the failure in its goal.
A retry that fails leaves the stage failed and blocks its dependents; nothing loops.

## Lifecycle

```
needs-input ──clarify──▶ planned ──▶ running ──▶ succeeded ──▶ delivered
                            │           │  └────▶ failed
                            │           ├──────▶ awaiting-approval
                            └───────────┴──────▶ paused / cancelled
```

- `needs-input` — the request was too vague to plan; no mission exists and no model was called.
- `planned`/`running`/`paused`/`awaiting-approval` — the mission's own states, read on every
  request.
- `succeeded`/`failed`/`cancelled` — terminal mission states.
- `delivered` — a commit recorded by `POST /api/engineering/pipelines/:id/deliver`.

Pause, resume, cancel and per-task approval are the mission routes; the dashboard links to
Mission control instead of duplicating them.

## Requirement intake

`analyse_requirement` asks at most two questions, and only when the sentence cannot be planned:

- fewer than four words, or
- a vague verb ("improve", "clean up", "modernize", "polish") with no target noun
  (file, page, test, API, module, function, endpoint, component, docs).

Everything else is planned. The acceptance criteria are fixed, deterministic statements:
behaviour implemented, check command succeeds, test command succeeds or the report says why it
could not run, documentation updated. Requests mentioning a bug add "reproduced before the fix
and no longer after"; requests mentioning an API add interface compatibility.

Risks are recorded, not resolved silently: no test files, no build manifest, a very large
workspace, a dirty worktree.

## API

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/api/engineering/project` | cached project profile |
| POST | `/api/engineering/analyse` | questions, acceptance and risks for a goal, without starting |
| GET/POST | `/api/engineering/pipelines` | list (last 30) / create |
| GET | `/api/engineering/pipelines/:id` | pipeline + mission + trace |
| POST | `/api/engineering/pipelines/:id/clarify` | answer the questions and start |
| POST | `/api/engineering/pipelines/:id/branch` | create a delivery branch |
| POST | `/api/engineering/pipelines/:id/deliver` | commit the pipeline's own files |
| POST | `/api/engineering/checks` | run the project's check command |
| POST | `/api/engineering/repair` | start a bounded repair pipeline |
| GET/POST | `/api/engineering/settings` | autonomy level |
| POST | `/api/engineering/stop` | emergency stop |
| GET | `/api/engineering/metrics` | measured quality numbers |
