# Agent lifecycle

## Mission states

```
planned ──tick──▶ running ──all tasks done──▶ succeeded
   │                 │
   │                 ├──any failed/blocked──▶ failed
   │                 ├──any cancelled───────▶ cancelled
   │                 ├──pause───────────────▶ paused ──resume──▶ running
   │                 └──approval needed─────▶ awaiting-approval ──approve──▶ running
   └──cancel───────────────────────────────────────────────────▶ cancelled
```

`paused` and `awaiting-approval` are not terminal: a supervisor stops ticking but
the record, its tasks and its run ids stay exactly as they are.

## Task states

```
planned ──start──▶ running ──run succeeded──▶ done
   │                 │
   │                 ├──run failed, attempts left──▶ planned (retry, new run)
   │                 ├──run failed, no attempts───▶ failed
   │                 └──run cancelled────────────▶ failed
   ├──dependency failed/blocked/cancelled─────────▶ blocked
   ├──cancel─────────────────────────────────────▶ cancelled
   └──needsApproval──────────────────────────────▶ planned (mission waits)
```

`done` has exactly one cause: the task's run reached `succeeded`. A started run,
a written file or a model claiming success never set it.

## Attempts

`max_attempts = 1 + mission.maxRetries` (retries 0–3). Each attempt is a separate
run row, so the failed attempt stays visible in the Runs list and in the report
(`attempts` on the task). The retry goal contains the previous error and the
attempt number.

## Conversations and steps

An agent's own lifecycle inside its run is unchanged from single-agent work:
`thinking` → action → step, with `handoff`, `verify` and `failover` steps where
those apply. The mission only reads the run's terminal status and its first `done`
summary.

## After a restart

`agent::mark_interrupted` turns runs that died with the process into `interrupted`
(unless they were explicitly paused). The mission supervisor is restarted for
missions still `running`/`planned`; the next tick sees those interrupted runs,
applies the retry policy and records the outcome. Nothing is re-run just because
the window closed, and nothing is reported as done that did not finish.
