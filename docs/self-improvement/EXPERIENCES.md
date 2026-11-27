# Experience and learning system

A lesson is written only when it is earned. The rule is deliberately narrow:

> A mission is terminal, it succeeded, and it left evidence: a file write, a command or a
> verifier check. Otherwise nothing is learned from it.

A failure is learned too, but as a `failure` record with a normalised error key, never as a
strategy. User preferences are the only records trusted without a mission, because a person
stated them.

## Record shape

| Field | Meaning |
| --- | --- |
| `kind` | `strategy`, `failure` or `preference` |
| `title`, `detail` | What worked or what broke, quoted from the mission report |
| `evidence` | Counts and labels from the report, for example `2 file(s) written, 1 check passed` |
| `key` | Dedupe key: goal digest, or `failure:<role>:<normalised error>` |
| `confidence` | 0.35 at first use, +0.15 per additional use, ceiling 0.9 for strategies and 0.8 for failures; preferences start at 0.9 |
| `validated` | 1 for successful missions and preferences, 0 otherwise |
| `uses` | Times the lesson was recorded or retrieved as relevant |
| `project` | Workspace scope; retrieval never crosses workspaces |
| `created_at`, `expires_at` | Retention from settings (default 90 days) |

## Validation rules

- Terminal states only: `running`, `planned`, `awaiting-approval` and `cancelled` teach nothing.
- Success alone is not a strategy: an answer with no write, command or check is not knowledge.
- Failures never become strategies, even if a later task in the same mission succeeded.
- Unvalidated records (a record imported with `includeUnverified`) are hidden by default and
  labelled in the API, so the UI cannot present them as established fact.

## Deduplication

The key is derived, not free text: the goal digest (or a keyword list when goal text is not
stored) plus the kind, so the same lesson from the same goal increments `uses` and confidence
instead of adding a row. Failure keys normalise line numbers, paths and quoted values, so
`error[E0308] at src/a.rs:12` and `error[E0308] at src/b.rs:90` count as one recurring failure.

## Privacy

- Goal sentences are stored only when **store goal sentences** is on. With it off, only a
  digest key is kept and the title is generic.
- Every row is scoped to one workspace path, and retrieval filters on it.
- Retention sets `expires_at`; `POST /api/improve/purge` removes exactly the expired rows.
- Deleting a lesson is immediate and permanent (`DELETE /api/improve/experiences/:id`).

## Retrieval

`GET /api/improve/experiences?q=…&limit=…` scores keyword overlap against title and detail,
adds confidence and a small recency term, and returns relevance with each row. An empty result
is presented as "no validated lesson matches", not as a fabricated answer.

## API

| Endpoint | Purpose |
| --- | --- |
| `POST /api/improve/learn` | Observe one mission id now (the supervisor already does this on completion) |
| `GET /api/improve/experiences` | Search this workspace's lessons |
| `POST /api/improve/experiences` | Record a user preference |
| `DELETE /api/improve/experiences/:id` | Forget one lesson |
| `POST /api/improve/purge` | Remove expired lessons |
