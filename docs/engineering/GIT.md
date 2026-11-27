# Git delivery

## Operations

| Action | Route | Guard |
| --- | --- | --- |
| read branch/commit/dirty | profile endpoint | none (read-only) |
| create a branch | `POST /pipelines/:id/branch` | `Action::Git`, approve at levels 1–2 |
| commit the pipeline's files | `POST /pipelines/:id/deliver` | `Action::Git`, mission must have succeeded |

Branch names default to a slug of the goal plus the first eight characters of the pipeline id.
`git.rs` validates them (no leading dash, no control characters) and refuses when the name is
taken, exactly as the Source control pane does.

## What is committed

Only the paths the mission report lists as written, deduplicated. A pipeline that wrote nothing
cannot deliver, even when every stage succeeded — the response says "wrote no files" and the
trace explains which criteria stayed unverified. Files the user changed outside the pipeline are
never staged.

The message is the request's first line, one bullet per completed stage from its own run
summary, and the mission id. No attribution footer is added.

## What is not here

No merge, no push, no rebase, no branch deletion, no force anything. Those stay manual, or in
the existing Source control routes, where the user calls them knowingly. Release notes are not
generated; the changelog is written by hand for the change being shipped.
