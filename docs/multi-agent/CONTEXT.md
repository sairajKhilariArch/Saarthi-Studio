# Shared context

A mission needs a shared picture of the work without letting unrelated tasks
contaminate each other. Both are enforced by how a task goal is built.

## What a task's agent is told

```
Mission: <the user's goal>

You are the <Role> agent in a coordinated team. <role purpose>

Your task (<title>): <task goal>

This task declared these paths, so no other agent edits them while it runs:
<files>

What the tasks before you found (their work is already done — build on it, do not
repeat it):
- <Role> / <title>: <summary>
…

<retry note, when this is a retry>

Stay inside this task: other agents own the rest of the mission. Finish with done
and a summary a reviewer can check against the goal.
```

## Isolation by construction

`task_goal` receives only the **direct dependencies** recorded in
`mission_tasks.depends_on`. A task does not see:

* the summaries of tasks it does not depend on, including siblings running at the
  same time;
* another mission's goal, plan or summaries;
* the raw step logs of other runs.

The goal is capped at 20 000 characters. Mission goals are capped at 5 000.

This is what keeps a "document the change" task from being told about a test
failure that a parallel "verify" task is still investigating — it only learns what
its own dependencies produced.

## Mission context

Each mission stores a small JSON `context`:

```json
{ "workspace": "/path/to/workspace", "projectId": "…" }
```

It is copied into the report so a run can be traced to the space it happened in.
It contains no file contents and no conversation text; agent runs already read the
workspace through their read-only tools.

## What is persisted

* The mission goal and template.
* Each task: role, title, goal text, dependencies, declared files, status,
  attempts, run id, summary, error.
* The message log.
* The frozen report.

Nothing else. Step history, file checkpoints and token counts stay in the agent
tables they already belong to, under the same privacy rules as any other run:
conversation content is not copied into mission rows, and the mission API returns
only goals, plans, statuses and summaries.

## Context compression

Summaries, not transcripts, travel between agents. A finished run contributes its
own first `done` summary (capped at 2 000 characters) and nothing else; a long
implementation log therefore reaches the reviewer as the agent's conclusion plus
the actual files on disk, which is exactly what the reviewer's own tools read.
