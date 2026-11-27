# Controlled self-modification

## From finding to proposal

`POST /api/improve/scan` runs deterministic checks over the current workspace and writes one
proposal per finding, deduplicated while a proposal with the same key is open:

| Finding | Detection |
| --- | --- |
| Duplicate top-level definition | The same `fn` (Rust) or `function` (JS/TS) name declared in two or more source files |
| Oversized file | A source file longer than 1200 lines |
| Documentation names a missing path | A backticked path in docs that does not exist |
| No test files | Source files exist and no test file was found |

Each proposal carries: problem, evidence lines (paths and counts), expected benefit, risk,
affected files, testing requirement, rollback strategy and a priority 1–3. Benefit and risk are
written by fixed rules from the counts — never by a model, so they cannot be inflated.

## Approval

`POST /api/improve/proposals/:id/approve`:

1. Refuses when learning is disabled, the proposal is not `proposed`, or the workspace is not
   a git repository.
2. Creates an engineering pipeline with `engineering::create_for_goal` on an isolated branch at
   the configured autonomy ceiling.
3. Records the proposal as `in-progress` with the pipeline id, and appends a `proposal-approved`
   governance event.

The pipeline then does what Phase 14 already does: plan, implement, check, document, review,
and deliver only the files it wrote, through guarded git operations. Nothing is committed to the
current branch and no release step runs.

## Review and merge

The diff is reviewed in the Engineering view. There is no automatic merge: delivery creates a
commit on the pipeline's branch, and merging that branch stays a user action.

## Rollback

`POST /api/improve/proposals/:id/rollback` with `{"approve":true}`:

- Requires a clean worktree and a committed HEAD.
- Reverts the delivered commit with `git::revert_checked` (a new commit, not a reset), so the
  history shows both the change and its reversal.
- Records the revert commit on the proposal and appends a `proposal-rolled-back` event.

## Refusal and rejection

Rejection records a reason and touches no code. Every refusal returns a stable error key
(`disabled`, `not-proposed`, `no-repository`, `dirty-worktree`, `no-commit`) so the UI can say
what happened instead of a generic failure.
