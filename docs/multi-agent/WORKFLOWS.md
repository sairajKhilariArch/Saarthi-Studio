# Multi-agent workflows

A mission's plan is either one of three built-in workflows or a task list you
write yourself. Every entry becomes a real agent run; the graphs below are the
dependency edges the supervisor honours.

## `feature` — plan, implement, test, document, review

```
plan (planner)
  └─▶ implement (coding)
        ├─▶ verify    (testing)
        ├─▶ document  (documentation)
        └─────────────┴─▶ review (review)
```

Requires `allowWrites` (the plan edits files). `verify` and `document` are
independent, so with `parallel: 2` the test run and the documentation run can
overlap — the writer and the reader do not conflict. **review** waits for both, so
it reviews the change and the evidence.

## `debug` — diagnose, fix, prove, review

```
diagnose (debugging)
  └─▶ fix (coding)
        └─▶ verify (testing)
              └─▶ review (review)
```

Read-only diagnosis first: the fix is told the root cause, not the symptom.

## `audit` — parallel investigation, then a synthesis

```
research (research)  ┐
review   (review)    ├─▶ synthesise (architect)
verify   (testing)   ┘
```

No writes; the three investigators have no dependencies, so all three start
together (`parallel: 3`). The architect only starts once every finding is in.

## `custom` — your task list

```json
[
  { "id": "schema", "role": "coding", "files": ["app/src/schema.rs"], "title": "Add the column" },
  { "id": "api", "role": "coding", "files": ["app/src/main.rs"], "dependsOn": ["schema"] },
  { "id": "docs", "role": "documentation", "files": ["docs/FEATURE-STATUS.md"] },
  { "id": "check", "role": "review", "dependsOn": ["api", "docs"] }
]
```

Fields:

| Field | Meaning |
| --- | --- |
| `id` | 1–32 lowercase letters, digits or `-`, unique in the plan |
| `role` | one of the eight role ids |
| `title` | short label shown in the dashboard |
| `goal` | what this agent must do (blank falls back to the title) |
| `dependsOn` | ids that must be `done` first; must exist and point backwards |
| `files` | paths this task may touch; unlocks safe parallelism |
| `needsApproval` | the mission waits for a person before this task starts |

Validation happens before any row is written: unknown role, duplicate id, unknown
dependency, cycle, more than 12 tasks, or a role switched off in Mission control
are all refused with a message naming the problem.

## Choosing a workflow

| Request | Workflow |
| --- | --- |
| "Add export to CSV and document it" | `feature` |
| "The importer crashes on empty files" | `debug` |
| "How risky is this module? What should we fix first?" | `audit` |
| "Two independent refactors, then one review" | `custom` with declared files |

## Parallelism comes from the graph, not from hope

A mission with `parallel: 4` still runs writing tasks one at a time unless each
declares disjoint `files`. That is the difference between a plan that is fast and
two agents editing the same file in different directions.

## Limits

* 12 tasks per mission.
* `parallel` 1–4.
* `maxRetries` 0–3.
* Each task's agent run is capped at 24 steps by the mission, on top of the
  agent's own hard limits.
* The project token budget applies to every delegated run, exactly as it does to
  a person's run.
