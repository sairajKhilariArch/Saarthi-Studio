# Debugging and repair

## Classification

`classify_failure` reads the output of the project's check command and returns one kind and a
suggestion. The list is deliberately small and literal:

| Kind | Signals |
| --- | --- |
| `dependency` | "failed to resolve", "no matching package", "module not found" |
| `compile` | `error[E…]`, "expected", TypeScript errors, `SyntaxError` |
| `runtime` | "panicked at", `Traceback`, "exception" |
| `environment` | "permission denied", "read-only file system", "command not found" |
| `network` | "connection refused", "timed out", DNS |
| `timeout` | the check exceeded its 180 s budget |
| `unknown` | anything else — read it before changing code |

A suggestion is text for a person or an agent; it is never executed automatically.

## The repair pipeline

`POST /api/engineering/repair` creates a four-stage mission:

1. `diagnose` (debugging) — reproduce and name the cause;
2. `fix` (coding, depends on diagnose);
3. `verify` (testing, depends on fix);
4. `review` (review, depends on fix and verify).

The same autonomy rule applies: at level 1 every stage waits, at level 2 the fix waits, at
level 3 the mission runs. Retries are 1 and the verifier must approve the review run, so a
failure is visible instead of being retried into silence.

## What "fixed" means here

Only the verifier's approval of the testing/review run and the mission report count. The
dashboard shows the check output and the run summaries; it never says "resolved" on its own.
