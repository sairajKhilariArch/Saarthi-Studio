# Communication protocol

Agents do not chat with each other. Every message is a typed, stored record that
the orchestrator writes when something real happened, and every record is
readable in the dashboard and at `GET /api/missions/:id/messages`.

That choice is deliberate: free agent-to-agent conversation is how autonomous
systems loop, invent work and lose the thread. A mission's coordination is the
task graph; the messages are the audit trail of that graph.

## Envelope

```json
{
  "seq": 7,
  "mission": "…",
  "task": "implement",
  "kind": "assignment",
  "from": "orchestrator",
  "to": "coding",
  "body": "Implement the change → Coding (running) using pinned/model",
  "at": 1795777480.8
}
```

`seq` is monotonic per mission, so two clients reading the log see the same
order. `from`/`to` are role ids, `orchestrator` or `user`.

## Kinds

| Kind | Written when | Direction |
| --- | --- | --- |
| `request` | the mission is created; a task is retried; approval is needed | user → orchestrator, orchestrator → role, orchestrator → user |
| `assignment` | a task's run is created | orchestrator → role |
| `completion` | a task's run reached `succeeded` | role → orchestrator |
| `error` | a run failed, a task was blocked, a task could not start | role or orchestrator → orchestrator |
| `status` | pause, resume, cancel, approval | user → orchestrator |
| `report` | the mission settled | orchestrator → user |

## Rules

1. **Only the orchestrator writes** outside of a user action. An agent's own
   words reach other agents through its run summary, not through messages.
2. **Every body is real.** A completion body is the run's own first `done`
   summary. If a run finished without one, the body says "Run finished without a
   summary." — it is never filled in with something plausible.
3. **Bodies are capped at 2 000 characters** when stored, so one noisy summary
   cannot bloat the log.
4. **Messages are never edited or deleted** while the mission exists; removing a
   mission (`POST /api/missions/:id/remove`, terminal missions only) removes its
   log with it.

## Reading the log

The dashboard shows the whole log under the task list, with the kind and both
ends of each message. `TROUBLESHOOTING.md` lists the sequences that explain the
common outcomes:

* `assignment` → `error` → `request` (retry) → `assignment` → `completion`;
* `assignment` → `error` → nothing (dependency of a failed task) → `error`
  "Blocked because …";
* `request` "…needs approval" → (user approves) `status` → `assignment`.
