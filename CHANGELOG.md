# Changelog

## 1.75.0

- Phase 9's remaining safety item: **a whole-tree checkpoint before a run can
  change anything**. The per-file checkpoints only cover the agent's own
  `write`, so a `command` step could change any file with nothing to go back to.
  Any run started with writes or commands now snapshots the tree before its
  first step and remembers it.
- `app/src/git.rs`: `snapshot_tree` builds that commit from a throwaway index
  (`GIT_INDEX_FILE`), so `git add -A`, `write-tree` and `commit-tree` never touch
  the user's staging area, worktree or history — a staged file stays staged.
  Untracked files are included, because a run can create them.
- `POST /api/runs/:id/checkpoint/restore` puts every tracked path that differs
  back to the snapshot and deletes files the snapshot never had; `git restore
  --source` does the writing, so binary files, modes and symlinks come back the
  way git recorded them. Untracked files are deliberately left alone (removing
  never-committed work would be guessing), a live run is refused with a 409, the
  restore is capped at 500 paths, and every path consults the `fs_write` rule
  before the first byte changes.
- The snapshot is kept in a small `run_checkpoints` table rather than on the run
  row, so the run record and its SQL mapping stay as they were. `INSERT OR
  IGNORE` means a resumed run keeps the snapshot it took when it first started.
- UI: a `Restore snapshot` button on a finished run beside `Restore all files`,
  with a confirmation that says later edits are discarded for those files.
- `cargo test --locked`: **407 passed, 1 ignored** (3 new, all against real
  repositories: a snapshot records edits and untracked files while the real
  index keeps its staged file; a restore puts modified and deleted files back,
  removes one the snapshot never had, and leaves nothing differing; a folder
  that is not a repository yields no checkpoint). Clippy `-D warnings` clean, no
  rustfmt diff in the touched files, `node --check app/static/app.js` clean.
- Live on a headless build at `:8954`: a run with writes enabled (and no router
  key, so it failed at its first model call) took the checkpoint; after `a.txt`
  was rewritten as a command step would, `checkpoint/restore` answered
  `{count: 1, restored: ["a.txt"]}` and the file held the snapshot text again; a
  second restore answered `{count: 0}`. In the smoke repository (host default
  `core.autocrlf`) the restored file came back with CRLF, so `git status` still
  showed it modified — the unit tests pin `core.autocrlf=false` and assert a
  fully clean tree.
- Not built: a worktree per run, line-level staging, rebase; the runs panel's
  new button was not viewed in a browser.

## 1.74.0

- Phase 9 could stage, commit, fetch, pull and push, but it could not **merge**:
  the Source Control pane named conflicted paths and left the rest to an
  external terminal. `app/src/git.rs` now runs a real merge into the checked-out
  branch, and a real abort.
- `POST /api/workspace/git/merge` (body `branch`) answers
  `{branch, current, fastForward, summary}` when git succeeds. When git stops on
  conflicts it answers **409** with `{conflicted: [paths], output, inProgress}`
  and leaves the markers in place — the conflict is an outcome the person acts
  on, not an opaque failure.
- `POST /api/workspace/git/merge/abort` runs `git merge --abort` and refuses
  with a 409 when nothing is in progress, so the control can never pretend to
  have worked.
- The tree must have no uncommitted **tracked** changes. Untracked files are
  deliberately ignored, as git ignores them: the first smoke run refused to
  merge because the server's own log file sat in the folder, which is the wrong
  rule, and that is now a test (a stray `notes.txt` does not block a merge).
- The branch name is narrowed before argv (no leading dash, no `..`, ASCII ref
  characters only), `--no-edit` is passed so a terminal-less git never waits on
  an editor, and both routes are gated on the `git` action (deny 403, ask 409
  until `approve: true`, audited).
- UI: a `Merge…` form in the Source Control pane, the conflicted files named in
  the toast, and an `Abort merge` control.
- `cargo test --locked`: **404 passed, 1 ignored** (5 new, all against real
  temporary repositories: a fast-forward with an untracked file present, a
  three-way merge, a conflict followed by an abort that leaves a clean tree and
  a second abort that refuses, a dirty tracked file refused with the edit left
  untouched, and a missing branch named instead of git's argv noise). Clippy
  `-D warnings` clean, no rustfmt diff in the touched files, `node --check`
  clean.
- Live on a headless build at `:8953`: `merge feature` → 200
  `{current: main, fastForward: true}` with git's own summary; `-x` → 400;
  `nope` → 400; merging `main` into `left` → 409
  `{conflicted: ["a.txt"], inProgress: true}`; abort → 200, then 409; a dirty
  tracked file → 409; `git status --porcelain` empty afterwards. The Source
  Control pane itself was not viewed in a browser.
- Not built: rebase, staging individual lines inside a hunk, and a worktree
  checkpoint before an agent writes.

## 1.73.0

- Phase 9's last reporting gap: **a test run now says what it found**. The
  Problems panel could already show `exit 101` and compiler diagnostics, but a
  failing suite had no counts. `app/src/runner.rs` parses the summary the tool
  itself prints and the pane header reads `· 1 passed · 1 failed`, with the
  failing test names in its tooltip.
- `parse_test_summary` sums libtest's per-binary `test result:` lines (the
  count/label pair is searched for, because libtest prefixes the segment with
  `FAILED.`), reads Jest's `Tests: a failed, b passed, c total`, Vitest's
  `Tests  a failed | b passed (c)` and node's TAP `# pass` / `# fail` /
  `# tests`, and collects libtest's indented `failures:` names and node's
  `not ok … - name` lines, capped at 20. The run response carries
  `tests: {framework, total, passed, failed, ignored, failures[]}`.
- Counts are never invented: a build run is not searched at all (`tests` is
  `null`), and a test run whose output has no summary line reports `null` too, so
  the panel stays silent instead of showing zeroes for tests that never ran.
- `cargo test --locked`: **399 passed, 1 ignored** (3 new: libtest counts summed
  across two binaries with its failure names, the Jest/Vitest/node summaries,
  and a build or compiler error reporting nothing). Clippy `-D warnings` clean,
  no rustfmt diff in the touched files, `node --check` clean.
- Live on a headless build at `:8952` over a scratch crate with one passing and
  one failing test: presets `cargo-build,cargo-test`; `cargo-test` answered
  `success false`, exit `101`, `tests {framework: cargo, total: 2, passed: 1,
  failed: 1, failures: ["tests::fails_on_purpose"]}`; `cargo-build` answered
  `success true` with `tests: null`, which is the point — a build never reports
  test counts. The server was stopped afterwards and the port verified closed.
- Not built: per-test timings, skipped-test names, a tests tree in the pane, and
  parsers for other runners (pytest, Go, JUnit XML). The pane header itself was
  not viewed in a browser.

## 1.72.0

- Phase 17's first slice: **one diagnostics timeline over what the app already
  recorded**. `app/src/diagnostics.rs` merges `request_log`, `permission_audit`,
  `agent_runs` and the failing `agent_steps` into a single time-ordered list,
  with a level per row and the id that ties related rows together (the run id,
  the project, the conversation). It is a read-only projection: no new table, no
  second logging pipeline, and nothing a panel shows can disagree with it.
- `GET /api/diagnostics?limit=&area=` returns the newest rows (default 100,
  capped at 500 and clamped server-side) plus counts from the same rows: provider
  requests and errors over 24 h, permission decisions by their own names, agent
  runs by status and how many are live. An unknown `area` is a 400 naming the
  three it accepts; both routes keep the loopback `Host`/`Origin` guard.
- `GET /api/diagnostics/bundle` produces the support bundle a bug report needs:
  app version, OS and architecture, the non-secret settings, credential
  **booleans**, the summary and the timeline. Run goals are deliberately left
  out, and every string is redacted and clipped before it leaves the process.
- Redaction is a value scan, not a schema: `sk-`/`ghp_`/`xoxb-`-prefixed tokens,
  anything after `Bearer`, `name=value` pairs whose name says key/token/secret/
  password/authorization/cookie, and long unbroken credential-shaped runs all
  become `[redacted]`. A public digest is redacted too, which costs nothing.
- UI: Settings → Diagnostics — the timeline with area/level colouring and the
  correlation id, a server-side area filter, a row count, the 24-hour summary
  line, and **Download support bundle**. The browser saves the server's JSON; it
  never assembles the payload itself.
- `cargo test --locked`: **396 passed, 1 ignored** (5 new: redaction of a key, a
  bearer token, a query parameter and a digest while prose survives; the merged
  timeline's order, levels, correlation and limit; a provider error redacted on
  the way into a row; counts read from the rows' own status/decision names; a
  bundle carrying credential booleans and no key, URL secret or run goal).
  Clippy `-D warnings` clean, `rustfmt --check` reports no diff in the two
  touched Rust files (five untouched modules keep their pre-existing
  formatting), `node --check` clean.
- Live on a headless build at `:8951` with a sandboxed `FREE_AI_STUDIO_HOME`: a
  chat against an unreachable router wrote a provider row, a granted rule wrote a
  permission row, and a real agent run wrote agent rows. The timeline reported
  areas `provider,permission,agent`, the summary read `1/1 requests failed ·
  deny 1 · failed 1`, the run's goal carrying `sk-live-smoke-secret` appeared as
  `smoke run with [redacted] inside`, `area=agent` returned only agent rows,
  `area=nope` answered 400, and the bundle carried `version 1.72.0`,
  `router_key_saved: true`, `base_url: [redacted]` and none of the three seeded
  secrets or the goal.
- Not built: a persistent log file and a console ring buffer, a request id for
  operations that are not agent runs, router-install and MCP rows in the
  timeline, and any preview of the bundle before it is saved. The panel itself
  was not viewed in a browser.

## 1.71.0

- Run restore no longer overwrites an edit made after the agent finished. Every
  write checkpoint now records a sha256 of the content the run wrote, and a
  restore compares it with the file on disk: a mismatch is skipped and returned
  as `changed`, so the response says why instead of silently losing the edit.
- The Runs panel names the changed paths and, after one explicit confirmation,
  retries with `force: true` for a deliberate overwrite. Checkpoints saved before
  this release carry no digest, so those paths keep the old behaviour; that
  limitation is recorded in `docs/KNOWN-ISSUES.md`.
- `cargo test --locked`: **391 passed, 1 ignored** (1 new: an edited file is
  skipped and left untouched, then restored only when forced; the existing
  checkpoint test now also pins the recorded digest to the written content).
  Clippy `-D warnings` clean, `rustfmt --check` clean, `node --check` clean.
- The guard is unit-tested through the same functions the route calls; the
  confirmation flow itself was not exercised over HTTP or in a browser.

## 1.70.0

- A finished agent run can be undone across every file it wrote. `POST
  /api/runs/:id/restore` (local and host prefix) reads the earliest pre-write
  checkpoint for each path the run touched and writes it back; the Runs panel
  offers **Restore all files** on finished runs, with a confirmation that says
  later edits are overwritten.
- Restore is deliberately bounded: only `succeeded`, `failed`, `cancelled` and
  `interrupted` runs qualify (a live or paused run is a 409), a run with no
  checkpoints is a 400, and more than 200 files is refused rather than partially
  applied. Every path consults the `fs_write` rule **before the first write**, so
  a saved deny is a 403 and nothing changes; a path that can no longer be
  written is reported and skipped while the rest still restore.
- The action is recorded as a `restored` step on the run and publishes the usual
  run-changed event, so the log stays an honest history instead of silently
  rewriting the past.
- `cargo test --locked`: **390 passed, 1 ignored** (2 new: the earliest
  checkpoint per path wins while a later checkpoint for the same file is
  ignored, a second file restores too, and the `restored` step is written; a
  running run is refused with a 409 and nothing is written). Clippy `-D
  warnings` clean, `rustfmt --check` clean on the touched files, `node --check`
  clean.
- Live smoke on `:8937` with two seeded checkpoints for `a.txt` (`one`, then
  `two`) and the file on disk at `three`: restore answered 200 `{count: 1,
  restored: ["a.txt"]}`, the file became `one`, the run detail gained `restored
  restore 1 file(s)`, a running run answered 409 with its status, and a saved
  `fs_write` deny answered 403 leaving `three` untouched. The button itself was
  not viewed in a browser.
- Not built: a whole-folder snapshot or worktree (only files the run wrote are
  restored, and only files whose old content was ≤200 000 characters are
  checkpointed), and a content guard against edits made after the run — the
  confirmation is the only protection there.

## 1.69.0

- The Source Control diff is no longer all-or-nothing: each hunk has a **Stage
  hunk** / **Unstage hunk** button, and history entries open the commit's own
  diff. Backed by `POST /api/workspace/git/stage-hunk`, `/unstage-hunk` and
  `GET /api/workspace/git/show` in `app/src/git.rs`.
- Hunk staging never trusts patch text from the browser. The server reads the
  real `git diff` for the one validated path, splits it at its `@@` headers, and
  keeps git's own `diff --git`/`--`/`+++` preamble for the chosen hunk; that
  patch travels to `git apply --cached` on stdin. Unstaging takes
  `git diff --cached` and reverses the same way. An index that moved under the
  hunk is a 409 with git's apply failure; an index past the end of the diff is a
  400 naming how many hunks exist.
- The commit view validates the hash before argv (4–40 hexadecimal characters,
  case-normalised), reads metadata and the patch with `git show`, and caps the
  diff at the same 128 KiB as the working-tree diff. An empty diff (for example
  a merge commit) says so instead of showing a blank box.
- Both hunk routes keep the existing rules: the path goes through
  `normalise_path` (relative only, no `..`, no drive letter, no control
  characters, no `.git`) and the `git` permission action with that path as the
  target (deny 403, ask 409 until approved, audited). Hunk indexes are bounded
  before they reach git.
- UI: the inline diff shows its real hunks, each with its own stage/unstage
  button; untracked files (which have no textual diff) still show their new-file
  body without a dead button. History rows are focusable buttons that open the
  commit's metadata and patch, with a Back-to-history control.
- `cargo test --locked`: **388 passed, 1 ignored** (3 new, against a real temp
  repository: only the first of two separated hunks staged, the second left in
  the worktree, then the staged hunk reversed out cleanly; a missing hunk index
  refused; a commit shown with its subject, short hash, author and `+line`
  patch, with non-hex input refused). Clippy `-D warnings` clean, `rustfmt
  --check` clean on the touched files, `node --check` clean.
- Live smoke on `:8934` with two changed lines 16 lines apart: the diff reported
  `2 hunks`; staging hunk 0 left only `line 2 changed` staged and only `line 18
  changed` in the worktree; unstaging it left the index clean; staging it again
  and committing produced `3cebfc4`; `show` returned subject `stage one hunk`,
  author `Smoke` and a patch with `+line 2 changed` but not line 18; hunk 99
  answered 400 with the real count; `main;rm -rf` as a hash answered 400; a
  saved `git` deny on `a.txt` answered 403; the uncommitted line 18 change
  survived. The pane was not viewed in a browser.
- Not built: merge/rebase, staging individual lines inside a hunk, image or
  binary diffs (no textual hunks), and worktree checkpoints before an agent
  writes.

## 1.68.0

- Phase 9's Git loop now reaches the remote: **Fetch**, **Pull** and **Push** in
  the Source Control pane, backed by `POST /api/workspace/git/fetch`, `/pull`
  and `/push` in `app/src/git.rs`. Nothing leaves the machine until the person
  clicks one of them.
- The remote is resolved, not assumed: a requested name must be configured,
  otherwise `origin`, otherwise the first configured remote. A repository with
  no remote says exactly that instead of failing deep inside git. Remote names
  are restricted to safe characters and cannot start with a dash, because the
  name reaches argv.
- Pull is **fast-forward only**. It needs a checked-out branch with an upstream,
  and when the histories have diverged git's own `Not possible to fast-forward`
  comes back as a 409 with nothing applied — no merge commit, no rebase, no
  conflict markers written behind the person's back. Push is never forced, and
  a first push sets the upstream once (`git push -u <remote> <branch>`) instead
  of printing git's "no upstream branch" paragraph.
- Remote operations are not local commands: each runs with a 90 s deadline,
  `GIT_TERMINAL_PROMPT=0` so a credential prompt cannot hang a request with no
  terminal, stdin closed, and stdout/stderr drained on their own threads, with
  64 KiB kept per stream. Authentication and unreachable-host failures are a
  502; a rejected non-fast-forward is a 409; a missing or invalid remote is a
  400.
- All three consult the `git` permission action with the real target
  (`fetch <remote>`, `pull`, `push`): deny 403, ask 409 until approved, audited.
  The pane gains Fetch/Pull/Push under the branch, Pull confirms what it will
  not do, and the state line shows git's last ref summary.
- `cargo test --locked`: **385 passed, 1 ignored** (4 new, against real temp
  repositories and a local bare remote: a first push setting the upstream and a
  second not, a clone's commit fetched and pulled into the source, a diverged
  pull refused with both histories intact, and remote-name/status mapping).
  Clippy `-D warnings` clean, `rustfmt --check` clean on the touched files,
  `node --check` clean.
- Live smoke on `:8933` against a bare remote: first push `upstreamSet true`,
  upstream `origin/main`; fetch reported `bd8b845..a4f25ca main -> origin/main`;
  `/vcs` read `ahead 0, behind 1`; pull fast-forwarded and wrote `b.txt`;
  after both sides committed, pull answered 409 `Not possible to fast-forward`
  and the remote file was not applied; a saved `git` deny on `push` answered
  403; `-x` as a remote 400; a repository without a remote 400 with the add-a-
  remote message. The pane was not viewed in a browser.
- Not built: merge/rebase, force push, adding or removing remotes from the UI,
  fetching every remote at once, and credential prompts (authentication must
  already be configured; the app will not ask for a password).

## 1.67.0

- Phase 9's developer loop: **Build** and **Test** buttons in the Problems panel
  run the project's own commands and turn the real output into clickable
  diagnostics. Detection is by manifest, not guesswork: `Cargo.toml` offers
  `cargo build` / `cargo test`; a `package.json` with `scripts.build` /
  `scripts.test` offers the matching npm, pnpm or yarn command from its
  lockfile. A folder with neither says so instead of showing a dead button.
- Backend `app/src/runner.rs`: `GET /api/workspace/runner` (detected presets,
  whether one is already running) and `POST /api/workspace/run` (runs exactly
  one preset id). The request names an id, never a command line, so the route
  cannot execute arbitrary text; it is still gated on the `command` action with
  the exact command as its target (deny 403, ask 409 until approved, audited).
  One run at a time; a second is 409 rather than two compilers sharing
  `target/`.
- Runs have a 300 s ceiling, capture stdout and stderr on separate threads (so
  a talkative build cannot deadlock on a full pipe), keep 1 MiB per stream and
  return the last 200 000 characters with `outputTruncated` when anything was
  cut. A failing build is a 200 with `success: false`, its exit code and its
  diagnostics — not an API error.
- Diagnostics are parsed from what the tools actually print: Rust
  `error`/`warning` lines with their `--> path:line:column`, TypeScript
  `path(line,col): error TSxxxx`, and the generic `path:line[:col]: error:`
  shape. Duplicates collapse, the list caps at 200, and a location outside the
  open folder is dropped rather than offered as clickable — including on
  Windows, where `/other/x` is rooted but not `is_absolute`.
- The Problems list navigates to the real position now (`path:line:col`, then
  the editor's line focus) and colours warnings differently; build output is
  shown above it with the runner status (`cargo build · exit 101 · 2 problems ·
  0.2s`) in the pane header.
- `cargo test --locked`: **381 passed, 1 ignored** (9 new: preset detection for
  Cargo and npm/pnpm with only declared scripts, Rust/TypeScript/generic
  diagnostic parsing, outside-workspace and duplicate filtering, the 200 cap,
  a real `cargo --version` run through the capture path, the one-run slot, and
  UTF-8-safe tail truncation). Clippy `-D warnings` clean, `rustfmt --check`
  clean on the touched files, `node --check` clean.
- Live smoke on `:8932` with a deliberately broken crate: presets
  `cargo-build`/`cargo-test`, the run answered `success false`, exit `101`,
  `src/lib.rs:2:25 error: mismatched types` and the real second error at `:3:5`
  in `245 ms`; an injected id (`rm -rf /`) was refused 400; a saved `command`
  deny on `cargo build` refused 403; after revoke the presets read `[]` on a
  folder with no manifest. The panel itself was not viewed in a browser.
- Not built: a debugger, watch mode, arbitrary command presets, and a Problems
  entry for compiler warnings that carry no file location.

## 1.66.0

- Phase 9's Git half: the Source Control pane can now change the repository
  instead of only watching it. Clicking a file shows its real `git diff`
  (`+`/`-`/`@@` coloured, an untracked file shown as a new file, a large diff
  marked truncated); `+`/`−` stage and unstage; `↺` discards after a
  confirmation; a message box commits (`Ctrl+Enter` too); `Branch…` creates or
  switches branches; `History` lists the last 20 commits with author and date.
  The pane groups merge conflicts, staged, changed and untracked files, and the
  branch row carries `↑ahead ↓behind` when the branch tracks an upstream.
- Backend `app/src/git.rs` — nine routes under `/api/workspace/git`, merged into
  the workspace router so they inherit the loopback `Host`/`Origin` guard and
  the request-body cap: `GET /diff`, `POST /stage`, `/unstage`, `/discard`,
  `/commit`, `GET /branches`, `POST /branch`, `/checkout`, `GET /log`. Every
  path is validated before argv — relative only, no `..`, no drive letter, no
  control characters, no `.git` — and arguments are passed as an array, never
  through a shell. Every mutation consults the existing permission store on the
  `git` action: a saved deny is a 403, a saved ask is a 409 until
  `approve: true`, and both land in `permission_audit`.
- Failure text is git's own sentence, not the first line of its output. In an
  empty repository `git commit` prints `On branch main` first; that was hiding
  `nothing to commit`, and a refused checkout (`would be overwritten`) now
  answers 409 instead of pretending to be a malformed request.
- `/api/workspace/vcs` gained per-file `conflicted`/`untracked` flags and
  `ahead`/`behind` from `# branch.ab`, so the pane can tell an unmerged path
  from an ordinary edit and a tracked branch from one with no upstream.
- `cargo test --locked`: **372 passed, 1 ignored** (10 new: path/message
  validation, log parsing with tabs in a subject, UTF-8-safe truncation, and
  five tests against real temporary repositories — stage → commit → diff →
  discard, untracked shown as a new file then removed, branch create/switch,
  empty history and nothing-to-commit, missing repository). Clippy
  `-D warnings` clean, `rustfmt --check` clean on the touched files,
  `node --check app/static/ide.js` and `app.js` clean.
- Live smoke on `:8931` against a real repository: `new file:` diff for an
  untracked path, stage of two files, commit `19141ae` with the expected
  subject, `log` returning it, `feature/smoke` created then `main` restored,
  a tracked edit producing a unified diff (`-beta`/`+changed`), discard
  restoring `alpha`/`beta`, `../evil` refused 400, a saved `git` deny on
  `b.txt` refusing stage with 403 (audited), revoke then stage succeeding.
- The pane's rendering was exercised only through the routes; its layout,
  colours and inline diff were not viewed in a browser.
- Not built: per-hunk staging, a commit-detail diff, merge/pull/push/fetch and
  worktree checkpoints. Merge-conflict resolution is therefore manual in the
  editor; the pane only identifies the conflicted paths.

## 1.65.0

- Browser check of 1.64.0's Index pane, against a live app on `:8916` with a mock
  embeddings endpoint. The pane works: the tab opens, the status reads `2 files ·
  4 symbols`, `login` + Enter answered `1 by words · login · src/auth.rs:1 · fn ·
  10`, saving `mock-embed` reached `/api/config`, "By meaning" answered `4 by
  meaning` with `login`/`logout` at 100 and `charge`/`refund` at 1, clicking a hit
  opened `auth.rs`, and Refresh re-walked the folder.
- One real defect found and fixed: the sidebar's segmented control was a fixed
  two-column grid, so the fifth tab pushed the control to three rows with an empty
  cell and `Index` sat in the first column of the last row. It is now
  `repeat(auto-fit, minmax(72px, 1fr))`: three columns and two full rows at the
  sidebar's width, measured in the running app (66px tall instead of 96px).
- Also confirmed in the running app: a narrow viewport scrolls the whole sidebar
  off-screen, which is pre-existing behaviour, not something this tab introduced.
- No Rust changed; `cargo test --locked` 362 pass, 1 ignored, clippy `-D warnings`
  clean, `node --check app/static/app.js` clean.

## 1.64.0

- The semantic search now has a home: a new **Index** tab in the sidebar. It shows
  what the index holds (`files · symbols`, with the languages in the tooltip),
  refreshes the walk, and searches the indexed symbols two ways — **By words**
  (`/api/index/search`, unchanged) and **By meaning** (`/api/index/semantic`).
- A hit names `path:line · kind · score` and opens the file in the editor when
  clicked, so a search result is a step towards the code rather than a dead line.
- The pane carries the embedding model field itself, since it is what the feature
  needs: saving it posts `embedding_model` to `/api/config`, and clearing it says
  plainly that searching by meaning is off. A semantic pass that has not embedded
  the whole index says `N embedded, N still to embed` instead of presenting a
  partial ranking as complete.
- Empty states are specific: an unread index, a word search with no hits, a meaning
  search with no hits (and why that can happen while embedding is still running),
  and a failed search each say what happened.
- No Rust changed; `cargo test --locked` still passes (362, 1 ignored) with the
  bumped version, clippy `-D warnings` clean, `node --check app/static/app.js`
  clean, and every element id the new code addresses exists in `index.html`.
- Not viewed in a browser: the pane's layout, the tooltip and the toast were
  checked only by reading the code and the id audit above.

## 1.63.0

- Phase 8's last piece: **semantic search over the code index**. `app/src/embeddings.rs`
  embeds each indexed symbol (`kind name in path (language)`) through the configured
  router, caches the vectors in `embedding_cache` keyed by model, symbol and the file's
  digest, and ranks them by cosine similarity. `GET /api/index/semantic?q=` returns
  path/line/kind/name/language/score plus `embedded`, `cached` and `pending`.
- Opt-in by construction: with no `embedding_model` set the route answers 400 and the
  word search is what every install already had. A provider failure is a 502 carrying
  its message — semantic search never quietly falls back to the word ranking.
- Bounded: one request embeds at most 32 documents, a single search at most 128 new
  ones and reports the rest as `pending`, so a large index fills in across searches
  instead of blocking on one long call. A model change, an edited file or a renamed
  symbol misses the cache, and a dimension change scores 0 rather than comparing
  vectors that no longer mean the same thing.
- Tests: 362 pass, 1 ignored. Four new: cosine alignment plus mismatched dimensions,
  ranking order with a key tie-break, cache hits keyed by model and digest with a
  re-store replacing the old vector, and documents carrying symbol, file and digest.
- Clippy `-D warnings` clean; `rustfmt` clean; `node --check app/static/app.js` clean
  (no UI changed). Live smoke on `:8916` against a mock embeddings endpoint: an
  unconfigured query → 400 "Set an embedding model before searching by meaning"; with
  `embedding_model: mock-embed`, "session handling" (words no symbol shares) →
  `embedded 4, cached 0`, hits `login`/`logout` at 100 and `charge` at 1; the second
  search → `embedded 0, cached 4`; "invoice amounts" → `charge`/`refund` at 100; and
  `/api/index/search?q=login` still answers exactly as before.

## 1.62.0

- Phase 8 follow-on: **a run now records what it was given**. The block that 1.61.0
  built is stored on the run when it is promoted (memory labels, code labels and
  the exact prompt text), and every step reads that stored block back instead of
  recalling again. A record saved while the run is working therefore cannot be
  smuggled into a prompt the card does not mention.
- `GET /api/runs` carries `context` (`{"memory": ["decision: Sessions use SQLite"],
  "code": ["src/auth.rs:1"]}`) and one run's detail adds `contextBlock`, the exact
  text the model saw. Fifty cards ship labels; only the open run ships the prompt.
- The run card appends `· given 1 memory + 1 code` with the labels in its tooltip,
  and the expanded run names them on a "Given at start" line. A run that was given
  nothing says nothing, and a run started before this column existed reports empty
  lists rather than inventing history.
- The column is additive (`context` on `agent_runs`), so an existing database is
  migrated in place on first start.
- Tests: 358 pass, 1 ignored. Two new: a promoted run stores `decision: Use SQLite`
  and `auth.rs:1`, hands the steps exactly that block, and ignores a record saved
  after it started; a goal matching nothing stores empty lists and an empty block.
- Clippy `-D warnings` clean; `rustfmt` clean; `node --check app/static/app.js`
  clean; the UI change was not viewed in a browser. Live smoke on `:8916` against
  a logging mock router: run "add a login session table in sqlite" → succeeded with
  `context: {"code": ["src/auth.rs:1"], "memory": ["decision: Sessions use SQLite"]}`,
  a non-empty `contextBlock` containing both blocks, the list request carrying the
  same labels, and a run with goal "reticulate the splines" → succeeded with
  `{"code": [], "memory": []}` and an empty block.

## 1.61.0

- Phase 8 follow-on: **agent runs start with the memory and code that chat has**.
  `agent::run_background` recalls the records the goal matches (project or global)
  and retrieves the index hits for it, and `build_messages` carries both as one
  labelled system message placed ahead of the goal, so the goal still reads as the
  task and the blocks stay background.
- Memory is read through `memory::recall_quiet`: a run asks on every step with the
  same goal, and counting each ask as a use would make the memory pane's history
  lie. Chat still bumps `uses`; a run leaves the counters alone.
- Both halves are optional and independent: no matching record means no memory
  block, no open workspace or nothing indexed means no code block. A goal that
  matches nothing is told exactly what it was told before.
- Known gap: a run card shows its steps, not the context it was given — there is no
  per-run context field yet, so the background is visible in the request the model
  received but not on the run itself.
- Tests: 356 pass, 1 ignored. Two new: a goal matching a saved record and an
  indexed `login` gets both blocks in one system message ahead of the goal while
  the record's `uses` stays 0, and a goal matching nothing produces no block and
  the unchanged two-message prompt.
- Clippy `-D warnings` clean; `rustfmt` clean; `node --check app/static/app.js`
  clean. Live smoke on `:8916` against a logging mock router: record "Sessions use
  SQLite" saved, `src/auth.rs` indexed, run "add a login session table in sqlite"
  → succeeded, and both model requests carried "Memory the user recorded for this
  work" and "Project code the index matched" with `src/auth.rs:1 — fn login`; the
  record's `uses` stayed 0.

## 1.60.0

- Phase 8: **RAG over the project** — a chat turn now carries the code the index
  matched to it. `index::retrieve` takes the ranked hits for the user's message,
  keeps one per file, and reads the declaration plus its opening lines from disk
  *now*, so the block can never quote a file that has since changed or been
  deleted.
- The block is labelled with `path:line — kind name` and tells the model to open
  the path before relying on it, so every claim has somewhere to point. A hit
  whose line no longer exists is dropped rather than guessed at.
- Bounded like everything else it sits beside: at most 4 snippets and 700 tokens,
  and the reply meta carries `retrieved` (`["src/auth.rs:3"]`) so the user can see
  which files the turn was given; the reply row shows `N code hits` and its
  tooltip names them.
- Retrieval only reads what the index already knows, so it never triggers a walk:
  the index is refreshed by its own route, and an unindexed project simply gets no
  block instead of a slow turn.
- Tests: 354 pass, 1 ignored. Two new: retrieval returning the matched
  declaration with its line, label and block wording, and one snippet per file
  with a deleted file producing nothing (the index's stale pointer is not
  trusted).
- Clippy `-D warnings` clean; `rustfmt` clean; `node --check app/static/app.js`
  clean. Live smoke on `:8916` against a logging mock router: after indexing a
  two-file folder (`added 2, symbols 3`), "where is login handled?" answered with
  `retrieved: ["src/auth.rs:3"]` and the request carried `src/auth.rs:3 — fn
  login` plus the real `pub fn login(user: &str)` lines; "what is the weather
  today" attached no block at all; "explain auth and logout" retrieved
  `src/auth.rs:7` for `logout`.

## 1.59.0

- Phase 8: **project code index**, the item that was listed as missing — file
  discovery, language detection, symbol extraction, an incremental index and
  ranked search, in `app/src/index.rs` with the `code_index_files` and
  `code_index_symbols` tables.
- The walk is bounded and refuses what it should not read: build and dependency
  folders (`.git`, `node_modules`, `target`, `build`, `dist`, `out`, `vendor`,
  `.venv`, `__pycache__`, …) are skipped by name, secret-shaped files (`*.pem`,
  `*.key`, `id_rsa`, `.npmrc`, `.netrc`, anything starting `.env`) are never
  read at all, the user's privacy exclusions are applied before the bytes are
  read, files over 1 MiB or containing a NUL byte are skipped, and the walk stops
  at 5,000 files and reports `truncated` instead of running on.
- Symbols are extracted per language family — Rust, Python, JavaScript/
  TypeScript, Go, Java/Kotlin/C#, Ruby, PHP, Swift, Bash — as path + line + kind
  + name. It is a line scan, not a parser: it cannot hang and a missed symbol
  costs a search hit, not a wrong answer.
- Refresh is incremental by content digest: unchanged files are left untouched,
  a changed file has its symbols replaced rather than appended, and a deleted
  file is dropped with its symbols. The report counts added / updated / removed /
  unchanged plus what was skipped and why.
- Search scores symbol names (exact 10, prefix 6, contains 4) and file paths (2),
  returns path, line, kind, name, language and score, caps at 50 hits, and
  refuses a query with no usable words — every hit can be opened and checked.
- Routes, behind the local-origin guard: `GET /api/index` (files, symbols,
  languages, `indexed_at`, folder), `POST /api/index/refresh`, `GET
  /api/index/search?q=`. Index state is a local SQLite file; nothing is sent
  anywhere by indexing.
- Tests: 352 pass, 1 ignored. Five new: per-language symbol extraction (including
  that a `const` is only a symbol when it holds a function and that markdown
  declares nothing), skipping secrets/build folders/oversize/binary, incremental
  refresh across change and delete, ranked search with line numbers, and the
  status report. Clippy `-D warnings` clean; `rustfmt` clean.
- No UI in this slice: the index is exercised through its routes, and the search
  results are already shaped for a results list.
- Live smoke on `:8916` against a scratch folder holding Rust, Python, a
  `node_modules` dependency, an `.env`, a `.pem`, a binary and an excluded
  `vault/`: the first refresh reported `added 3, symbols 4, skipped {secret 2,
  binary 1, excluded 1}`, status named `rust` and `python` and the search for
  `login` returned `src/auth.rs:1 fn login` while `secret` returned nothing and a
  blank query was refused 400. Rewriting one file gave `updated 1, unchanged 2`
  with the old symbol gone and the new one found; deleting another gave
  `removed 1` and its symbols disappeared with it.

## 1.58.0

- Phase 8: **project memory** — records the user chooses to keep, in
  `app/src/memory.rs` and the `memory_records` table. Four kinds (`decision`,
  `preference`, `task`, `fact`), a required title, optional details, and a scope:
  one project or every project. Nothing writes a record on the model's behalf.
- API: `GET /api/memory` (the global records plus the active project's — never
  another project's), `POST /api/memory` (create, or update in place when the id
  is known, keeping the creation time), `POST /api/memory/:id/delete`. All three
  are behind the local-origin guard; no write route touches the filesystem.
- Recall is scored, not dumped: a record earns 3 per title word and 1 per body
  word the user's message shares with it, needs 2 points, and the result is
  capped at 6 records / 900 tokens with ties going to the most recently updated.
  The block tells the model these are background facts, not instructions, so a
  stored preference cannot quietly become a command; recalled records have
  `uses`/`last_used_at` bumped and the reply meta carries `memory_used`.
- UI: a Memory mode in the sidebar beside Threads, Files and Skills — the
  project's records with kind, scope and use count, a form (kind, title,
  details, scope) and a forget button. The reply row says how many records the
  turn was given, and its tooltip names them.
- Tests: 347 pass, 1 ignored. Seven new for memory: project scoping with global
  records, refusal of an unknown kind or an empty title (storing nothing),
  update keeping the id and creation time, delete removing only its record, recall
  by shared words with the usage counters moved, a title match outranking a body
  match with the labelled block, and the recall budgets.
- Clippy `-D warnings` clean; `rustfmt` clean; `node --check app/static/app.js`
  clean. Live smoke on `:8916` against a logging mock router: a real project was
  created and activated, three records saved (one for another project, one
  global), an unknown kind answered 400, and `GET /api/memory` listed only the
  global record and the active project's — the other project's stayed hidden.
  "how does auth keep sessions?" answered with `memory_used: ["decision: Auth
  uses local sessions"]`, that record's block was in the system prompt and
  labelled as background facts, and the unrelated preference did not leak in;
  "keep the replies short please" recalled the preference; both records showed a
  bumped use count afterwards.

## 1.57.0

- Phase 6: **automatic skill selection**, the gap where a skill only ever rode
  along if the user picked it in the composer. `skills::select_for` scores every
  enabled skill against the message: 4 for each word its id or name shares with
  the message, 2 for each word of its one-line description, noise words and
  numbers dropped, so "please review src/main.rs for bugs" selects `code-review`
  and "what is the weather today" selects nothing.
- Selection is bounded and repeatable: a message must share at least 2 points of
  score before anything is attached, at most two skills ride along, ties break on
  id, and the cropped instructions must fit 1,200 tokens together — the same
  message always selects the same skills. Disabled and broken skills are never
  selected.
- A manual pick still wins: matching only runs when the composer names no skill,
  and `auto_select_skills` (default on, stored in `config.json`) turns it off.
  The reply meta carries `auto_skills`, the reply row shows `skill <id>`, and the
  match is recorded in `skill_usage` so the skills panel's history is honest.
- Defect found by the live smoke and fixed here: the chat path stored the
  decorated prompt as the user's message, so a skill's instructions were written
  into the transcript and replayed as the user's own words on every later turn
  (and once matching ran, they would have compounded). The transcript now stores
  what the user typed plus the editor code they attached; the newest outbound
  turn is the decorated prompt, assembled on the way out.
- Tests: 340 pass, 1 ignored. Six new for selection: a name match, a
  description-only match, an unrelated message and a messageless one selecting
  nothing, disabled and broken skills skipped, the cap plus repeatability, and the
  token budget declining a second full-size skill. Clippy `-D warnings` clean;
  `rustfmt` clean; `node --check app/static/app.js` clean.
- Live smoke on `:8916` with two fixture skills (`code-review`, `changelog`) and a
  mock router that logs each request: "please review src/main.rs for bugs"
  answered with `meta.auto_skills: ["code-review"]` and the request carried the
  skill's instructions; a manual `changelog` pick sent `auto_skills: null` with
  only that skill's body; "what is the weather today" sent neither; "review the
  changelog for the release" selected both; the transcript held only the typed
  message, and usage counts rose for both skills.

## 1.56.0

- Phase 5 item 5: **verification is enforced**, closing the last Phase 5 gap. A
  run that changed the workspace — a `write`, a `command` or an `mcp` step in its
  log — is no longer marked `succeeded` because the model said `done`: its goal
  and full persisted step log go to the verifier (`call_reviewer`, the same
  shared model router every agent step uses), which must start its reply with
  `APPROVE`.
- Approval writes a `verify` step holding the verdict and then the closing `done`
  step. Anything else counts as ISSUES: a `verify` step holds the findings, a
  `handoff` step sends them back for one repair round, and a second ISSUES verdict
  ends the run `failed` with the findings kept verbatim and an error that says the
  repair round did not settle them — nothing is marked fixed that was not fixed.
- A verifier that cannot answer cannot approve: the call's error propagates and
  the run fails rather than being called complete. Two rounds is the ceiling, so a
  disagreement cannot loop.
- Runs that changed nothing skip verification — a read-only run's output is a
  report and there is nothing on disk to check — and `team` mode is not verified
  twice, because its reviewer already is the verifier.
- Tests: 334 pass, 1 ignored. New: a written file approved by the verifier (step
  list `write, verify, done` with the verdict stored), a verifier reporting issues
  twice (the run ends `failed` with the findings kept in both `verify` steps), and
  a read-only run ending without a verifier. Three existing tests changed because
  verification now runs: the crew handoff test and its resume test gain the
  verifier's approval reply and a `verify` step, and the write-conflict test's
  script gains the approval its write now requires. Clippy `-D warnings` clean;
  `rustfmt` clean; `node --check app/static/app.js` clean.
- Live smoke (headless build on `:8916`, scripted router on `:8917`): a run that
  wrote a file and claimed done, with the verifier replying ISSUES twice, ended
  `failed` with steps `write, verify, handoff, write, verify, done`, both findings
  kept in their `verify` steps, and the error "Verification still reported issues
  after a repair round; nothing was marked fixed that was not fixed." A second run
  whose verifier approved ended `succeeded` with steps `write, verify, done`, the
  approval stored as the `verify` step's output.
- The run board and detail panel need no change: verification shows up as the
  existing `verify` and `handoff` steps with their output, and a refused run is the
  ordinary `failed` state with its error in the card.

## 1.55.0

- Phase 5 lifecycle: **retry as its own control**, not a synonym for resume.
  `POST /api/runs/:id/retry` (also on the host prefix) starts a clean run from a
  finished run's settings — the same goal, profile, mode, crew, flags, step
  ceiling and priority, a new id, an empty step log — and links it back through a
  new `agent_runs.retry_of` column, so "same settings, fresh log" is a fact in the
  record rather than a convention.
- Two things are deliberately *not* copied. Dependencies are dropped: the
  original's have already run, and re-waiting on them would make a retry depend on
  history instead of on work. Tokens, duration and the error are not carried
  either, so a retried run's cost and timing are its own.
- The new run gets a `retry` step naming the original and the state it ended in,
  so "why is this run here" is answerable from the run itself. It is inserted
  `queued` and admitted by the same dispatcher as every other run, so a retry
  waits behind a busy workspace rather than starting a second agent.
- Refusals are specific: a run that has not finished is refused with "cancel it
  first if you want a clean retry", and a *paused* run is told to resume instead,
  because the two controls mean different things — resume continues this log,
  retry throws it away. `goal` and `priority` may be restated in the request body;
  permissions and budgets never can, they always come from the record.
- The model scope is re-resolved for the retry (`run_record_snapshot` with the
  original's task-scoped model), so the new run records the routing it will
  actually use instead of inheriting a snapshot from an older configuration.
- Runs panel: the detail controls now separate **Resume where it stopped** (quiet)
  from **Retry from scratch** (accent), the latter offered for failed,
  interrupted, cancelled *and* succeeded runs, and a card whose run was a retry
  says `retry of <short id>`.
- Tests: 331 pass, 1 ignored. New: a retry starting a clean run with the same
  settings (goal, profile, ceiling, priority, write permission, `retry_of`, empty
  error, `retry` as its first step, and the original left exactly as it ended), a
  retry restating the goal and priority, a retry not inheriting dependencies, a
  retry of a live run refused with no second row created, and a paused run told
  to resume instead. Clippy `-D warnings` clean; `rustfmt` clean; `node --check
  app/static/app.js` clean.
- Verified live on :8915 against a mock router scripted to fail once and then
  succeed: the first run ended `failed` with no `retryOf`, `POST /retry` answered
  `{status: "running", retryOf: "6d27dbab-…"}`, the retried run ended `succeeded`
  with steps `retry, done` and the same goal, and the original stayed `failed`. In
  the browser the failed run's detail offered `Resume where it stopped` and
  `Retry from scratch`; clicking retry opened the new run with card meta
  `2 steps · 0s · just now · retry of 6d27dbab · mock`, steps `retry, done`, and
  the toast “Retry started — the new run has an empty step log.”
- One design defect found and fixed in the same slice: the file-lease ledger from
  1.54.0 was keyed by path alone while liveness comes from a database, so a second
  database in the same process (a test fixture, or a headless server beside a
  desktop build) could decide this database's runs were dead and drop their
  claims. The ledger is now keyed by database *and* path, and `release` is scoped
  the same way — which is what made the suite deterministic.

## 1.54.0

- Phase 5 item 4, first half: **file leases between live runs**. Concurrent runs
  share one folder, and nothing on the filesystem stopped both writing the same
  file — last writer wins, silently. `app/src/filelock.rs` now records which live
  run has claimed a path, plus the fingerprint of the content each run last read.
  A write is refused when another live run holds the claim, or when the content
  on disk is not what this run last read, and either way it is refused **before**
  anything is written.
- A refusal is a `write-conflict` step, not a failure: the run keeps going, the
  model sees the reason and can re-read the file and rewrite from what is
  actually there. The two messages name the blocker explicitly — the owning run
  and its liveness, or “changed on disk since this run last read it — another run
  or another program wrote it”.
- The baseline is the file as it is, read from disk for the write's checkpoint,
  never the model's memory of it. A read records that content; a successful write
  records what the run just wrote, so a run's own edit is not mistaken for
  somebody else's change on the next write. That second half was a real defect
  found here: the team-review path's repair write failed after its own first
  write, and `team_run_repairs_after_review_findings` caught it.
- Leases are bounded and never permanent: at most 256 tracked paths per run, and
  a claim held by a run that is no longer `running` or `awaiting_approval` is
  dropped on the next claim, so a crash or a cancel cannot keep a file locked.
  A paused run holds no claim (it released its loop) but keeps its read
  fingerprints, so a resumed run still notices a file that moved while it was
  parked. `finish` releases everything a run holds, on every path that can end
  one — success, failure, cancel, emergency stop, exhausted ceiling.
- Scope stated honestly: only the agent's own `write` action is guarded. A
  command step, an editor save or another program can still change a file with no
  lease; the run's next write is caught by the fingerprint check instead. There is
  no worktree per run, and the ledger is in-process only, so
  `max_concurrent_runs` still defaults to 1 — the limit is now guarded rather than
  unguarded, but two agents in one folder remain the user's explicit choice.
- Tests: 326 pass, 1 ignored. New: a write refused when the file changed between
  the run's read and its write (a mock that edits the workspace on the request
  before the write) with the run then recovering by re-reading and finishing, a
  second live run unable to claim a claimed path (the refusal names the owner), a
  finished run's claim not outliving it, a cancelled run's claim released by
  `finish`, a paused run holding no claim while keeping what it read, and a file
  that appeared since the read counting as a conflict. Clippy `-D warnings` clean;
  `rustfmt` clean on `agent.rs` and `filelock.rs`.
- Verified live on :8913 with `max_concurrent_runs: 2` and a prompt-scripted mock:
  with run ONE still working after its write, run TWO's write was refused as
  `shared.txt is claimed by run f371ece2-…, which is still live.` while ONE was
  `running`, TWO continued to `done`, and the file kept ONE's content; run THREE,
  whose file was rewritten by the mock between its read and its write, got
  `shared.txt changed on disk since this run last read it` and left the other
  writer's content untouched. The first attempt at this smoke *looked* like a
  missing guard: the single-threaded mock serialised the second run's calls behind
  the first run's sleep, so the second write landed after the first run had
  already finished. Re-running against a threaded mock showed the refusal at
  0.38 s with the first run still live.

## 1.53.0

- Phase 5, the **task queue**: a run no longer starts the instant it is created.
  Every run is inserted as `queued` and admitted by one dispatcher pass, so
  "start now" and "wait your turn" cannot disagree about the rules. The pass
  honours a new `max_concurrent_runs` setting (1–4, default 1) because runs share
  the open workspace and nothing locks the files they edit; runs past the limit
  wait, in priority order, oldest first on a tie.
- `priority` (clamped to -10…10) and `dependsOn` (a list of run ids) are stored on
  the run (`priority`, `depends_on`, `started_at` migrated in place) and validated
  at the door: an unknown id, a self reference, a loop found by walking the stored
  dependencies, an empty id or more than eight entries is a 400 **before any row
  exists**, so a typo cannot create a queue entry that waits forever.
- A dependency only counts once it has `succeeded`. While it is queued, running or
  paused the dependent keeps waiting; when it ends `failed`, `cancelled` or
  `interrupted` the dependent is failed with a `queue` step and an error naming
  the dependency, rather than being left queued against work that will never
  arrive.
- `started_at` separates queue wait from real work: `elapsedSeconds` now measures
  from when execution began instead of when the row was created, and a queued run
  reports no duration at all rather than "0 s".
- One admission pass, with a status change conditional on the row still being
  `queued`: two passes cannot start the same run twice, and a run cancelled while
  it was being promoted is never started behind the user's back. The pass runs on
  create, on resume, after an approval, after a pause parks a run, after a cancel,
  at the end of every run loop, and at boot — a queued run survives a restart and
  is admitted once `mark_interrupted` has freed the slot it was waiting for.
- Pause names a queued run's real state (“queued, not running — cancel it to take
  it out of the queue”), cancel accepts one, and the emergency stop includes
  queued runs. Approval is a queue entry too: approving while another run holds
  the slot waits instead of starting a second agent, and `started_at` is kept so
  the run's own duration is not reset by the wait.
- Isolation is **not** done: there is no worktree or per-file locking and no
  conflict detection before a write, so the limit defaults to 1 and raising it is
  the user's explicit choice. That is Phase 5 item 4, still open and stated as
  such in `docs/FEATURE-STATUS.md`.
- Runs panel: a `queued` pill (dashed, quieter than running), `queue #N` and
  `waits on N runs` in the card meta, a Queued count on the dashboard, a “waiting
  in the queue (position N)” note with only Cancel while queued, Queue priority
  and “Wait for runs (ids)” fields on the run form, and “Agent runs at once” in
  Settings.
- Tests: 320 pass, 1 ignored. New: an unknown dependency refused before the row
  exists, the cycle guard against a stored loop, priority ordering under the limit
  (a later, higher-priority run is admitted first, and nothing runs beside the
  slot holder), a dependent waiting for its dependency and starting after it, a
  dependency that was cancelled failing its dependent with the named reason, a
  queued run cancelled out of the queue and never started, and a queued row
  admitted at boot with `started_at` recorded. Clippy `-D warnings` clean;
  `rustfmt` clean on `agent.rs`; `node --check app/static/app.js` clean.
- Verified live on :8911, not merely compiled. Against a mock router holding each
  reply 0.6 s: A `running`, B `queued` at position 1, C (priority 5) `queued` at
  position 1 with B moved to 2, then all three `succeeded` with C's `startedAt`
  before B's; a run depending on a cancelled run failed as
  `Dependency 8a2780f3-… ended cancelled.`; an unknown dependency answered
  `400 Dependency nope does not exist.`. With a slow mock, a second run created
  against the same workspace stayed `queued` and then started and succeeded on its
  own when the slot freed. In the browser the queued card carried class
  `run-queue` with meta `0 steps · just now · queue #1 · mock`, the board read
  `Queued 1`, its detail showed the waiting note with Cancel/Refresh only, and
  Settings read back `1`.

## 1.52.0

- Phase 12 close-out, part one: a run can now be **paused** instead of only
  cancelled. `POST /api/runs/:id/pause` parks a live run at its next safe point —
  the boundary between steps — so a model call or a command that is already
  executing finishes and is recorded, and nothing new is planned or run after
  it. The loop writes a `pause` step, sets the record to `paused` and releases
  its control entry, so a parked run holds no live task.
- Pausing is cooperative and says so: while a step is in flight the answer is
  `{"pauseRequested": true, "status": "running"}`, and the request re-reads the
  record before replying, so a run that finished or was cancelled in that window
  is reported as what it actually is rather than as paused. Pausing an
  already-parked run is a no-op (`alreadyPaused`), and a run waiting for approval
  is refused with the two choices the user really has — it is already parked with
  a question outstanding, so there is no step to stop.
- `paused` is durable: the startup sweep still marks `running` and
  `awaiting_approval` runs `interrupted`, and deliberately leaves a paused run
  paused, because a pause is a decision the user made. `resume` on a parked run
  continues from the same step log and records that it is resuming after a pause.
- `cancel` accepts a parked run (there is no flag to wake, so the record is
  closed directly), and the emergency stop now also reads the record for paused
  runs: they have already released their loop, so an ACTIVE-only sweep would
  have left a paused run armed after the stop the user asked for.
- Runs panel: a **Pause** control while a run is working, **Resume** while it is
  parked (alongside Cancel in both), a Paused count on the dashboard, and a
  paused pill styled as deliberately parked rather than as live work or as a
  run blocked on a human. The pause route is registered on the host API prefix
  too, so VS Code and Android reach the same state machine.
- Two defects found while making pause work end to end, both fixed here. First,
  a parked loop released its control entry by run id, so a resume that registered
  the next loop first had its control deleted underneath it: every later pause
  answered `200` with `pauseRequested: true` and was never read, and the run just
  kept working. Release is now identity-checked (`Arc::ptr_eq`), the loop reads
  the pause flag from its own control, and the step-ceiling guard only fires when
  the loop really left through the ceiling — a resume reopening a run while a
  finished loop was still in its tail could previously fail the resumed run with
  "Run exceeded the step ceiling". Second, the runs panel never refreshed on
  agent events: the event handler gated on `state.view === "runs"`, but
  `state.view` is the left rail (explorer/search/…), never `runs`, so the board
  and cards only changed on a manual Refresh and an asynchronous pause could not
  appear at all. An `agent-run-changed` event now re-reads the list and the open
  run's step log (new `refreshRunsPanel`).
- Tests: 313 pass, 1 ignored. New: a live pause against a delayed mock router
  (parks without reaching `done`, releases its control entry, repeat pause is a
  no-op, resume finishes the goal), a pause refused while awaiting approval plus
  cancel closing a parked record, a pause with no live loop parking the record
  itself, an emergency stop reaching a parked run, a second pause right after a
  resume still being honoured (the control-lifetime regression), and the restart
  sweep leaving a paused row paused. Clippy `-D warnings` clean; `rustfmt` clean
  on `agent.rs`; `node --check app/static/app.js` clean.
- Verified live, not just compiled: the headless binary on :8905 against a mock
  router that holds each reply 400 ms — `POST /pause` answered
  `pauseRequested: true`, the run parked as `paused` with steps `think, pause`
  (the in-flight step finished first), a second pause answered `alreadyPaused`,
  `/api/runs/overview` reported `{paused: 1}, live: 1`, `resume` returned
  `running` and kept stepping, and `cancel` closed it as `cancelled`. In the
  browser on :8906 the Run views showed the paused pill (class `run-pause`), the
  `Resume`/`Cancel run` controls, then Pause → running → Pause → paused with the
  step log advancing one boundary at a time and no manual refresh.

## 1.51.0

- Phase 12: live agent monitoring. A cross-run dashboard now sits above the runs
  list: how many runs are live, how many are stopped waiting for a human answer,
  how many failed, and the steps and tokens the listed runs have used. It comes
  from one query — `GET /api/runs/overview` — that reuses the same rows as the
  list, so the board and the cards can never disagree about a run.
- Per-run totals that were missing: `agent_runs.tokens` (migrated in place) is
  incremented by the run's own model calls, and every run row now reports
  `tokens`, `elapsedSeconds` and `idleSeconds`. A live run's elapsed time is
  measured from its start rather than its last write, so a run that has been going
  for ten minutes no longer reads as “0 s”; once it stops, the duration is the
  real start-to-last-write span. Cards show the duration and the token total
  beside the step count and the last-activity time.
- Live updates use the event stream the runs panel already listens on, so the
  board refreshes when a run advances, pauses for approval or finishes; the
  dashboard is a read of the same state, never a second source of truth.
- Still honestly missing, and named rather than implied: no pause (a run is
  cancelled or waiting for approval, nothing in between) and no queue view,
  because Phase 5 has no queue.
- Tests: 308 pass, 1 ignored. New: the monitor payload carries a run's stored
  token total, its step count and both timings, and measures a live run from its
  start. Clippy `-D warnings` clean; `rustfmt` clean on `agent.rs`; `node --check
  app/static/app.js` clean. Not verified in a browser: the dashboard was
  syntax-checked only, and no live smoke was run for this slice.

## 1.50.0

- Phase 3 remainder, part one: a circuit breaker per model. New
  `app/src/breaker.rs` counts consecutive unavailable answers — only 429, 5xx,
  timeouts and transport failures; a 4xx refusal never trips it, or a mistyped
  model would hide behind a cooldown — opens after three, and refuses calls for
  45 seconds before allowing exactly one probe. The state is process-local and
  based on a monotonic clock (a system clock change must not extend an outage),
  keyed per workspace so two configs in one process do not share a cooldown.
- Both call paths use it: `call_router_with` and `open_stream` skip a model whose
  circuit is open instead of paying a timeout per try, close it on a success, and
  re-open it on a failed probe. When every candidate is cooling down the call
  fails with a message naming each model and its remaining seconds instead of a
  generic “unavailable”, and a skipped candidate is recorded in `fallbacks` as
  `circuit open: skipped`, so an answer is never credited to a model that was
  never asked.
- Health monitoring: `GET /api/health/providers` merges the breaker's live state
  with what `request_log` actually saw over 24 hours — requests, successes,
  average latency and the provider's own last error per model — and states the
  rule in the payload, so the UI does not have to invent it. `/api/health` keeps
  its cheap liveness answer for the extension.
- Usage page: a “Provider health” card showing each model's circuit state, the
  retry countdown while it is open, ok/requests, latency, and the last error on
  hover. A model that the breaker knows about but the log has no rows for is
  listed rather than implied healthy.
- Persisted model choice: `conversations.model` (migrated in place) records the
  model that answered a thread, written by both reply paths and returned by
  `/api/conversations`; opening a thread restores that choice instead of silently
  snapping to today's default. The app-wide default stays `model` in config.
- Tests: 307 pass, 1 ignored. New: the breaker's threshold, allow/refuse, success
  reset, per-scope isolation and refusal wording, plus a live mock-router test
  where an open circuit refuses before the router is called at all. Clippy
  `-D warnings` clean; `rustfmt` clean on the touched files; `node --check
  app/static/app.js` clean. Not verified in a browser: the Provider health card
  was syntax-checked only, and no live smoke was run for this slice.

## 1.49.0

- Phase 6 close-out: skills can now be installed, updated and removed, with
  version tracking and real usage history. Discovery, enable/disable and
  instruction injection were already working; these are the parts that were
  missing.
- Install (`POST /api/skills/install`) writes `<user skills>/<slug>/SKILL.md`
  from a name, an optional version and description, and plain instructions. The
  manifest is validated with the same parser the list uses **before** anything
  touches the disk, so a skill with no instructions is refused rather than
  installed broken, and the write goes through a temp file in the same folder so
  an interrupted install cannot leave half a manifest. An existing id is refused
  unless `replace: true`.
- Update (`POST /api/skills/:id/update`) rewrites the manifest in place and
  keeps its folder, so the id, the config's disabled list and the usage rows stay
  attached. It only touches skills in the user root: a project, built-in or
  `~/.agents` skill is refused with a message saying to edit its file where it
  lives. No version in the request means no version claim — the recorded version
  is kept instead of inventing a bump.
- Remove (`POST /api/skills/:id/remove`) moves the folder to
  `<user root>/.removed/<folder>-<timestamp>`, which is reversible. It never
  deletes the user's files, and never touches a root this app did not write.
- Version tracking: `skills.json` beside the config records the installed
  version, install time, update time and a content fingerprint (FNV-1a — called a
  fingerprint, never presented as a checksum). A mismatch with the file today is
  reported as “edited since install”, so a hand edit stays visible instead of
  being overwritten silently or mistaken for the installed version.
- Usage history: a `skill_usage` table records every pick — skill id, time,
  conversation and instruction tokens — from the chat path where the conversation
  id exists. `/api/skills` reports `uses`/`last_used_at`/`tokens` per skill and
  `GET /api/skills/:id/usage` returns the recent rows. Recording is
  fire-and-forget: a lost row never fails the turn it describes.
- Permission-gated: install, update and remove consult the same `fs_write` policy
  as every other write in the app (deny 403, ask 409 until `approve: true`), and
  the UI goes through the existing approval prompt.
- UI: an install form in the Skills pane, an editable instruction body in the
  detail view with Save and Remove, and version / uses / “edited since install”
  facts on each row. A skill outside the user root reads as read-only, with the
  reason, rather than offering buttons that would fail.
- Tests: 301 pass, 1 ignored. New: install writes and parses a manifest with its
  version; a second install is refused unless asked and the refused write leaves
  the original intact; an empty skill creates nothing; a hand edit changes the
  fingerprint; the registry keeps the install time across an update; slugs cannot
  escape the root; remove moves rather than deletes; usage counts, sums tokens and
  orders newest first. Clippy `-D warnings` clean; `rustfmt` clean on the touched
  files; `node --check app/static/app.js` clean. Not verified: the new Skills UI
  was syntax-checked but not opened in a browser, and the three new routes were
  exercised through their file-level operations rather than over HTTP.

## 1.48.0

- Phase 11 close-out: context compression that is more than head/tail cropping.
  `context.rs` now normalises whitespace (trailing spaces, runs of blank lines),
  drops an exact repeat of an earlier turn once, and — only when the request
  would exceed the budget — keeps the newest turns whole while they fit and folds
  older ones into labelled excerpts appended to the system prompt
  (`- [assistant] … (N characters in the full message)`). They are labelled
  excerpts, because packing makes no model call and will not pretend it
  paraphrased anything.
- A thread that fits is sent **untouched**: compression is a remedy for an
  over-budget request, not a style applied to every turn. The fitting path
  reports zero condensed, zero dropped and zero tokens saved, and the contents
  are unchanged.
- The digest gets at most a quarter of the budget, reserved before the recent
  turns are placed, so old context cannot crowd out the turn being continued.
  The thread's first message survives as a pointer when the window has moved past
  it instead of being silently forgotten.
- Anything that does not even fit as an excerpt is counted as **dropped** rather
  than quietly lost: `PackedRequest` reports `trimmed_messages`,
  `dropped_messages` and `tokens_saved`, and both reply paths put
  `context_trimmed`, `context_dropped` and `context_saved_tokens` in meta beside
  the existing `context_tokens`/`context_budget`. The reply row shows
  “N condensed · N dropped · ~N saved”, with a tooltip naming what was sent, the
  budget, what was kept out and how many messages were not sent at all.
- Tests: 293 pass, 1 ignored. New: whitespace normalisation that never edits the
  words, a fitting thread sent untouched with no compression language, old turns
  folded into labelled excerpts with tokens saved and nothing dropped, a repeated
  turn sent once and counted, and a huge current message kept (cropped only to
  its own cap) while older turns are condensed. Clippy `-D warnings` clean;
  `rustfmt` clean on the touched files; `node --check app/static/app.js` clean.
  The UI change was syntax-checked but not viewed in a browser, and no live smoke
  was run for this slice.

## 1.47.0

- Phase 11: spend is now derived from published prices only. New
  `app/src/cost.rs` turns a provider's own per-million prices (already in
  `capabilities::Price`, read from the same `/models` catalog the model menu
  uses) and its own reported token counts into an estimate. When either is
  missing the request is recorded as **unpriced** instead of costing zero — a
  missing number is reported as missing, never as a free call.
- `request_log` gains `cost_usd` and `cost_basis`, migrated in place on existing
  databases. Both chat paths write them: a non-streaming reply prices the meta it
  already has, and a streaming reply carries the price into `ReplyRecorder`
  (`with_price`), so the cost is computed from the tokens that actually arrived —
  including an aborted stream, because the recorder still saves what it got.
- Budget enforcement that stops calls: `daily_budget_usd` (Settings → OpenAI
  Platform → Daily budget) over a **rolling 24 hours**, because timestamps are
  stored in UTC and the app has no user timezone to cut a calendar day with. The
  gate runs once, before the fallback chain in `call_router_with` and before
  `open_stream`, so a fallback cannot walk around it, and the refusal is a
  non-retryable error naming the spend, the budget, and how many unpriced
  requests the figure leaves out. Zero still means “no limit”, so a fresh install
  never blocks its first turn. Local inference is not a paid provider call and
  never reaches the gate.
- `/api/usage` answers a `cost` block (`usd_24h`, `unpriced_24h`,
  `filtered_usd`, `filtered_unpriced`, `budget_usd`, `basis`), per-request
  `cost_usd`/`cost_basis`, per-model `usd`/`unpriced`, and the same for the last
  24 hours per project through `conversations.project_id`. Unpriced requests are
  counted next to the total rather than quietly dropped from it.
- UI: an “Est. spend (24 h)” stat card showing the budget and the unpriced count,
  a Cost column on the request table, and a context meter that now shows
  utilisation (`~12.3k / 24k · 51%`) and turns over at the budget. Replies carry
  `context_budget` in meta so other clients (the Android app, the extension) can
  show the same figure without guessing.
- Stated limitation, not papered over: native OpenAI Platform picks have no
  router catalog row, so they stay unpriced — the platform publishes prices, but
  this app does not mirror a price table and will not invent one. Attachment and
  compression effects on context are still estimated, and every cost figure here
  is an estimate, never a billed amount.
- Tests: 289 pass, 1 ignored. New: cost arithmetic against a published price,
  “no reported usage means no cost” (and one half reported is still a cost), the
  allow/refuse boundary at the budget with the unpriced floor note, a live mock
  catalog proving a published price is read and an unpriced model is not given
  one, and a live test where a spent budget refuses before the mock router is
  asked at all. Clippy `-D warnings` clean; `rustfmt` clean on the touched files;
  `node --check app/static/app.js` clean. The UI changes were syntax-checked but
  not viewed in a browser, and no live smoke was run for this slice.

## 1.46.0

- Phase 3 rewiring: model choice now has the four scopes the specification
  asks for and one documented precedence — task > agent > project > global >
  `auto`. New `app/src/model_scope.rs` resolves it and reports which scope won,
  so a reply can say where its model came from instead of leaving it implicit.
- Per-project model: `projects.model`, migrated in place on existing databases,
  accepted by create/update and returned by `/api/projects`. The project menu
  shows it and a "Model" action edits or clears it; empty inherits.
- Per-agent model: `agent_model` in config (Settings → agent runs model) is the
  agent scope. Agent runs also take a task scope — `POST /api/runs` accepts
  `model` — and the run record snapshot now stores `agent_model`,
  `project_model`, `resolved_model` and `resolved_scope` rather than repeating a
  single setting.
- Real fallback chain: `fallback_models` in config, tried in order, trimmed,
  deduplicated, capped at four, with `local:` picks dropped because local
  routing is decided by local mode rather than by a fallback. A fallback is
  attempted only when the provider was unavailable: 429, 5xx, timeouts and
  unreachable/connection failures. A 4xx refusal, a missing key or a Thinking
  refusal is returned as-is, because it would fail identically on every model.
- Streaming keeps that rule with a stricter edge: fallback happens only before
  the first byte, so a half-streamed answer is never replayed on another model.
  A fallback that does not advertise reasoning gets no `reasoning_effort`
  (dropping it for that attempt is honest; sending one the model may reject is
  not), while the chosen model still refuses with 400.
- Attempts are visible: meta carries `fallbacks: [{model, error}]` and the
  message row renders "N fallback(s)", so an answer from a backup model is never
  presented as if the first choice worked.
- Defect fixed in the same path: an answer that omitted `model` filled the meta
  with the global default, which mislabelled a fallback attempt; it now reports
  the model that was actually asked.
- Tests: 282 pass, 1 ignored (new: scope precedence and blank-inheritance,
  chain order/dedup/cap/local-drop, the retryable-vs-rejected matrix, and two
  live HTTP tests against a mock router — 429 falls through with both models
  asked in order and `fallbacks` recorded, 400 is returned as-is with the
  fallback never asked). Clippy `-D warnings` clean; `rustfmt` clean on the
  touched files; `node --check app/static/app.js` clean.
- Live smoke on :8946 (throwaway HOME, mock router that answers 503 once):
  `POST /api/config` stored `agent_model` and deduplicated
  `fallback_models: ["backup-model","backup-model"]` down to one; the first
  `/api/chat` asked `primary-model`, got 503, asked `backup-model`, and answered
  `answered by backup-model` with `model: backup-model`, `scope: global` and
  `fallbacks: [{model: primary-model, error: "Router returned HTTP 503: …"}]`;
  `POST /api/projects/:id/update {"model":"project-model-x"}` showed up in
  `/api/projects`; the next chat asked `project-model-x` and answered with
  `scope: project`. The mock's log recorded exactly `primary-model`,
  `backup-model`, `project-model-x` in that order.
- Version line: 1.46.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, `vscode-extension/package{,-lock}.json` and
  `android/app/build.gradle.kts`.

## 1.45.0

- Phase 7 close-out: a supervisor for wedged MCP sessions. `mcp::start_supervisor`
  runs once at boot (`OnceLock`-guarded, so every boot path may call it) and
  sweeps the session pool every 60 s: each idle session is pinged with a 5 s
  silence budget, so a session that stopped answering while nobody was calling
  is closed on the timer instead of the next user call paying the whole 20 s
  operation ceiling before the fresh-handshake fallback noticed.
- The probe is per transport, and only silence is a wedge. stdio writes `ping`
  on the child's stdin and waits for the id-matched reply (a JSON-RPC error
  counts — the question was heard); streamable HTTP accepts any 2xx answer and
  reports a non-2xx as a dead transport; the legacy HTTP+SSE transport must see
  the id-matched reply on its event stream.
- The sweep takes the map (`std::mem::take`) before probing, so a session a call
  has checked out is never probed and a call cannot race the supervisor. An
  answering session goes back with its *original* `last_used` — a probe must
  never extend a session past its idle window — and `restore_pooled` keeps a
  fresher entry when a call reconnected while the probe was in flight.
- Every eviction is narrated in the server's own log (`stopped answering ping
  within 5s` / `is gone (<reason>)`, each naming the reconnect that follows), so
  the MCP panel explains a session that disappeared instead of showing nothing.
- Tests: 275 pass, 1 ignored (new: `the_supervisor_keeps_answering_sessions_and_
  evicts_wedged_ones` against a real streamable-HTTP mock — an answer keeps the
  original stamp, silence is evicted with its log line, an HTTP 500 is evicted
  with its reason, and an empty sweep is a no-op; plus `a_probe_does_not_replace_
  a_session_that_reconnected_meanwhile`). Clippy `-D warnings` clean;
  `rustfmt --check src/mcp.rs` clean.
- Version line: 1.45.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, `vscode-extension/package{,-lock}.json` and
  `android/app/build.gradle.kts` (Android was still on 1.43.0).

## 1.44.0

- Phase 7 leftover: sign in to OAuth-protected remote MCP servers. New
  `app/src/mcp_oauth.rs`: RFC 9728 protected-resource discovery, RFC 8414
  authorization-server metadata with issuer-location validation (a document
  that claims another issuer is refused), RFC 7591 dynamic client registration
  (public client, `token_endpoint_auth_method: none`), PKCE (S256,
  RFC-7636-verified vectors in tests), an in-flight map keyed by `state` in the
  same atomic as the keychain, token exchange and a pre-connection silent
  refresh with 30 s clock margin (an unknown expiry of 0 is never expired).
- The browser leg runs on the app's own port: `GET /api/mcp/oauth/callback`
  (read-only, outside the origin guard) redeems the redirect once — a second
  callback with the same state finds nothing — with a bound per flow of the
  verifier and server name so a forged callback cannot pair with a foreign
  verifier. Tokens plus registered client material live in the OS credential
  store under account `mcp_oauth/<server>`, never in `mcp.json` or a file; with
  the store off, sign-in surfaces the failure honestly instead of falling back
  to disk.
- Tools on remote servers now carry `Authorization: Bearer <token>`
  automatically (`attachable_header`), but only when the user did not write an
  `authorization` header themselves — a hand-written header wins. A pool
  fingerprint that includes headers reopens the session when the token changes.
- HTTP 401 on a `tools/call` is surfaced as "sign in again" rather than a
  retry, because re-running a side-effecting call would double its effect.
- Routes (mutations behind the local-origin guard, gated on `network` with
  `ask` → 409 default): `GET /api/mcp/:name/oauth/status` (booleans plus
  expiry and manual-header flag, no token values), `POST /api/mcp/:name/oauth/
  start` (opens a real browser and waits for redemption), `POST
  /api/mcp/:name/oauth/forget` (deletes the credential). A stdio server and a
  hand-written-header server are refused 400.
- Settings → MCP panel: a Sign in / Sign out control plus a status line per
  remote server (`Not signed in` / `Registered but not signed in` / `Signed in
  with OAuth; expires …`), fetched without blocking the panel.
- Defect found while smoking the flow: `cmd /c start <url>`-opened links died
  at the first `&`  — cmd split the authorize URL into separate commands
  (`client_id`, `state`, `code_challenge`, … executed as programs; the mock
  saw `state null`). The Windows opener is now
  `rundll32 url.dll,FileProtocolHandler <url>`, which hands the URL to the
  shell association without a shell in between (verified in the smoke below).
- Tests: 273 pass, 1 ignored (new: PKCE pair, well-known candidate spellings
  including trailing-slash dedup, authorize-URL shape, token response folding,
  expiry margin with unknown-never-expired, mock-server discovery →
  registration → exchange round-trip, one callback per armed state). Clippy
  `-D warnings` clean (also moved `terminal.rs`' router back above its test
  module for `items_after_test_module`); `node --check app/static/app.js`
  clean.
- Live smoke on :8884 (throwaway HOME and keychain service, real Windows
  Credential Manager) against a local node mock that serves RFC 8414 metadata,
  registration, `/authorize` and `/token` and plays the streamable-HTTP MCP
  endpoint: status `registered:false authorized:false`; OAuth start opened the
  real default browser, the mock logged the full query including `state` and
  the S256 challenge, redirected to `/api/mcp/oauth/callback`, and start
  returned `authorized:true expires_at:… has_refresh:true`; Credential Manager
  showed `LegacyGeneric:target=mcp_oauth/auth.SaarthiStudio-mcp-smoke144`; an
  approved `tools/call` answered and the mock's header log recorded
  `POST / -> Bearer access-smoke-1`; a saved `network` deny on the server URL
  then refused a new sign-in with 403 naming the rule; a stdio server's start
  was 400 (`needs no OAuth sign-in`); `forget` returned `OAuth session
  removed.`, the Credential Manager listing counted 0 and status was back to
  `authorized:false`. The mock's `/token` verifies the PKCE challenge, so the
  recorded authorization_code grant also proves the verifier round-tripped.
- Version 1.44.0 across `app/Cargo.toml` (+ `Cargo.lock`),
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 60).

## 1.43.0

- Phase 2 close-out, dependency half (spec §5.3): `/api/routers` now carries a
  measured `environment` — OS and OS version, architecture, every package
  manager found on PATH with its version (`apt-get`, `dnf`, `pacman`, `zypper`,
  `apk`, `brew`, `winget`, `choco`, `scoop`, `port`), Docker, and `docker
  compose` (or the legacy `docker-compose`). Presence comes from a PATH lookup,
  versions from the tool's own `--version`; nothing is guessed. The Router panel
  prints the same facts instead of leaving a blocked install unexplained.
- Phase 2 close-out, editor half (spec §5.6): a real configuration editor for
  the compose-based router. `GET /api/routers/:id/config` lists the whitelisted
  files (`docker-compose.yml|yaml`, `compose.yml|yaml`, `.env`) in the project
  the install recorded; `GET …/config/file?name=` reads one; `POST …/config/file`
  saves it through a temp file and a rename. Reads and writes share one
  whitelist validator, so a separator or `..` can never leave that directory,
  and the editor stops at 64 KiB. A router with no file says why: OmniRoute
  keeps its settings in its container, and the local runtime is configured
  through the Local runtime panel that validates it.
- Saves are gated on `fs_write` for the exact file, so a saved rule can freeze
  the router's compose file without freezing the workspace; the UI retries once
  after the person approves a 409.
- Defect found while smoking the gate: permission targets were compared as raw
  strings with `starts_with`, so a rule written with `/` never matched a target
  a Windows caller built with `Path::join` (`\`). A deny was silently inert.
  Targets and rules are now normalised (separators and trailing slashes) before
  matching, revoking and one-use consumption, with a regression test.
- Tests: 266 pass, 1 ignored (new: version extraction, whitelist validation,
  file round-trip and oversize refusal, router-without-file reasons, separator
  normalisation). Clippy `-D warnings` clean; `node --check app/static/app.js`
  clean.
- Live smoke on :8879 with a throwaway HOME and a fabricated compose project:
  `/api/routers` reported the real host (Windows 10.0.26200, docker 29.9.2,
  compose 5.6.1, winget) and FreeLLMAPI as installed from the compose dir; the
  editor listed and read `docker-compose.yml`; saving `.env` returned 200 and
  the file appeared on disk; `../x` and `config.json` were refused 400; an
  unknown router was 404; OmniRoute answered with its container reason; a
  forward-slash `fs_write` deny then refused the save with 403 even though the
  route passes a backslash path, and after revoking the rule the save succeeded.
- Version 1.43.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 59).

## 1.42.0

- Phase 10 close-out: saved API keys live in the OS credential store instead
  of `config.json`. New `app/src/keychain.rs` on the `keyring` 3 crate — Windows
  Credential Manager (`windows-native`), macOS Keychain (`apple-native`), Linux
  Secret Service (`sync-secret-service`). Service
  `io.github.sairajkhilariarch.SaarthiStudio` (the AppStream id), one entry per
  field (`api_key`, `search_api_key`, `openai_api_key`).
- `save_config_file` splits secrets out before writing: each non-empty one is
  stored and blanked from the JSON. `load_config` merges stored values over
  the file. A store failure keeps the value in the permission-restricted file
  (the pre-keychain behavior) instead of dropping a key.
- Upgrade path: one idempotent migration at startup moves any plaintext secret
  still in `config.json` into the store and blanks the file.
- Saved keys can now be removed deliberately: `POST /api/config/forget`, gated
  on the same `settings`/`config.key` rule as a key change (deny → 403), with
  a "Remove saved key" button under each key field. Blank inputs still mean
  "keep the saved key" — removal is its own action.
- `GET /api/config` reports `keychain: "os" | "mock" | "off"`, and the
  Settings labels say where a key actually lives instead of claiming a store
  the app is not using.
- Test isolation: the keyring mock store keeps data inside the credential
  object, so entries are cached per name for the process, and mock-mode
  services are derived from the config directory's hash — parallel fixtures no
  longer share credentials. `.cargo/config.toml` selects the mock store for
  `cargo test`/`cargo run`, so no test or dev run touches a real vault;
  installed binaries get the platform store. `FREE_AI_STUDIO_KEYCHAIN=off` is
  the escape hatch (secrets stay in `config.json`).
- New dependency, documented in `app/Cargo.toml`: `keyring` (one crate, three
  platform stores; no existing equivalent in the tree).
- Tests: 261 pass, 1 ignored (new: mode parsing/freezing, AppStream service id,
  secret round-trip through the store, per-location isolation, unstored name).
  Clippy `-D warnings` clean; `node --check app/static/app.js` clean.
- Live smoke on :8878 against the real Windows Credential Manager (temp home
  plus `FREE_AI_STUDIO_KEYCHAIN_SERVICE=…smoke`, so the production service and
  the real config were untouched): a seeded plaintext key migrated on startup
  (`config.json` field `''`), `cmdkey /list` showed the entry under
  `api_key.<service>`, a fresh process with an empty file still reported
  `has_api_key: true` (cross-process read), `forget` removed the entry and
  flipped it to false, and a saved `settings` deny on `config.key` refused
  removal with 403. The smoke credential was deleted; none remain.
- Version 1.42.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 58).

## 1.41.0

- Phase 10 remainder: a global emergency stop. `POST /api/emergency-stop`
  cancels every live agent run in one call (each live loop's cancel flag is
  woken and its record closed to `cancelled` with an `emergency-stop` step,
  mirroring the per-run cancel endpoint) and closes every terminal session in
  one call (the hub is drained through the same SIGHUP-on-drop path the
  per-session close and application exit use).
- The route sits with the permission routes behind the local-origin guard but
  is deliberately NOT policy-gated: a deny rule must never be able to hold the
  safety release closed. It is the one privileged operation the audit trail
  records with an unconditional allow, so every firing is visible in the
  Permissions panel even when it stopped nothing (`stopped 0 live runs and 0
  terminal sessions`).
- The Runs panel carries an "Emergency stop" button with a confirm; it also
  detaches the client's terminal attachment so the panel's Restart opens a
  fresh shell instead of talking to a dead session id, and it reports the
  server's own counts (an empty response honestly says nothing was live).
- Test-scaffold fix found on the way: `wait_for_terminal` did not treat
  `cancelled` as terminal, so a genuinely cancelled run looked like a hang.
- Tests: 254 pass, 1 ignored (new e2e: a running agent run, one stop →
  `cancelled` with an `emergency-stop` step, a second stop finds nothing; plus
  the empty-hub stop test). Clippy `-D warnings` clean; `node --check
  app/static/app.js` clean.
- Live smoke on :8877: one terminal open, the stop returned `[1, cwd]`, its
  stream then answered 404, a second stop returned `[]`/`[]`, and the audit row
  read `allow / emergency-stop · stopped 0 live runs and 1 terminal session`.
  The agent-run half is covered by the scripted-router e2e (a live smoke of a
  run needs a model reply, so the test drives the same helper the loop uses).
- Format-only re-wraps in `app/src/mcp.rs` ride along from the formatter; no
  behavior change.
- Version 1.41.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 57).

## 1.40.0

- Phase 10 remainder: the privileged routes a person drives now consult the
  permission store. Terminal open is `command` on `terminal`; editor saves and
  workspace create/rename/delete are `fs_write` on the path; settings saves are
  `settings` on `config`, with key changes on `config.key` so a rule can freeze
  only the key fields.
- A saved deny answers 403 and names the rule. A saved ask answers 409 with
  `needs_approval`; the request runs only when it carries `approve: true`. The
  reads, the UI's confirm, edit-save, delete and terminal-open calls retry once
  after the person says yes. A deny still wins over `approve`, because the rule
  is checked first.
- No rule means no prompt: an editor save or an auto-opened terminal cannot turn
  into a dialog nobody asked for. This is deliberately narrower than the agent
  path, which keeps the ask-by-default matrix; both share the same resolution
  order and audit table.
- Every refusal and every approved ask is written to `permission_audit`, with
  the route's own detail ("save a file from the editor", "change a saved key",
  "open a terminal").
- Not wired, because there is nothing to wire: git has only a read-only status
  route, and updates have no in-app route. `project`-scoped rules still apply to
  agent steps only, since the UI routes carry no project. The OS keychain and
  the global emergency stop remain open.
- Tests: 252 pass, 1 ignored (no new unit tests — `guard_user` needs an
  `AppState`; the behavior is covered by the live route smoke instead). Clippy
  `-D warnings` clean; `node --check app/static/app.js` clean.
- Live smoke on :8765: a save with no rule returned 200; a session `fs_write`
  deny on `blocked.txt` returned 403; revoking it allowed the save; a
  `settings` ask on `config.key` returned 409 + `needs_approval`, then 200 with
  `approve`; a `command` deny on `terminal` returned 403; revoking it opened a
  terminal; the audit trail held both decisions and the approval.
- Version 1.40.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 56).

## 1.39.0

- Phase 7 remainder, connection half: an MCP server now keeps a live session
  between calls. `invoke` checks out the server's warm session, re-reads the
  tool list on that same connection (so schema validation still describes the
  server as it is), calls the tool, and hands the session back by `close`.
- What reuse skips is the process start and the `initialize` handshake: a
  second call to the stdio smoke server measured 0 ms against 39 ms for the
  first, and a save that evicted the session measured 29 ms again.
- The pool is in memory only and bounded: a session is warm for five idle
  minutes, is evicted when the server's entry is saved or removed (so a new
  command, URL, header or token can never reach an old connection), and is
  dropped rather than reused when a request fails. A pooled session that stops
  answering falls back to one full handshake.
- Discovery, Test and the health probe still open fresh sessions on purpose:
  they are the paths that must report what the server offers right now.
- Dropping a session closes what it owns — a stdio child is killed by
  `kill_on_drop` and an SSE reader is aborted by `Drop` — so an eviction cannot
  leave a process or a stream behind.
- Concurrency is stated rather than implied: two simultaneous calls to one
  server can open a second session; the pool keeps one and the other is
  dropped. There is still no supervisor or health loop.
- Tests: 252 pass (3 new: the pool fingerprint follows transport, command, URL
  and header edits; the idle-window boundary; and eviction of a name that was
  never pooled).
- Live smoke on :8765: call 1 `connected … 39 ms`, call 2 `reused a pooled
  session … 0 ms`, re-saving the same entry forced call 3 to `connected … 29
  ms`; the log counted `connected=2 reused=1 calls=3`, and all three calls
  returned the text they were sent.
- Version 1.39.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 55).

## 1.38.0

- Phase 7 remainder, rediscovery half: a run's MCP catalogue is no longer
  frozen at run start. When an `mcp` step fails, the catalogue is read again
  before the repair step, because that is the step that has to fix the call —
  a server that restarted with a different tool list was previously invisible
  until a new run.
- Bounded per run: the snapshot carries a `refreshes` counter and a run gets
  three. Each refresh contacts every enabled server (discovery starts a stdio
  child or opens a remote session), so a miss loop must not be able to respawn
  them step after step. A resumed run keeps the allowance it used; a snapshot
  written before this field, or one that cannot be parsed, reads as zero.
- A refresh is visible: it records an `mcp-refresh` step summarised as
  "N MCP tool(s) after a refresh", whose output names the reason, the allowance
  used, the `server.tool` list, and an `unavailable:` line per server that
  still cannot be reached. It notifies listeners like every other step.
- Tests: 249 pass (2 new: a snapshot carries its refresh allowance including
  old and corrupt input, and a refreshed catalogue reaches the next prompt).
- Live run on :8765 with a scripted stand-in model: step 1 called
  `echo.missing_tool` and the step error named the tools that do exist; the
  refresh step recorded `(0 of 3 refreshes used)` with `echo.echo` and
  `echo.ping`; step 2 called `echo.echo` and returned `echo: after refresh`;
  the run finished `succeeded` with `mcpTools: 2` across 5 steps.
- Still missing, recorded rather than implied: a persistent connection pool
  (each operation still opens one session), and remote auth beyond the header
  values a user writes.
- Version 1.38.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 54).

## 1.37.0

- Phase 7 close-out, transport half: an MCP server is no longer always a child
  process. A registry entry now carries a `transport` — `stdio` (unchanged),
  `http` (streamable HTTP) or `sse` (the older HTTP+SSE transport) — plus a
  `url` and optional request `headers`.
- One conversation, three carriers. `Connection` opens whichever transport the
  entry names, and the handshake, catalogue, schema check, permission gate,
  agent integration and health/log bookkeeping are shared with stdio. The Test
  route, `invoke` and `discover_for_prompt` gained no second path.
- Streamable HTTP: one POST per message with `Accept: application/json,
  text/event-stream`; the answer is read as JSON or folded out of an SSE body,
  and an `Mcp-Session-Id` returned by the handshake is carried on later
  requests. A notification accepts any 2xx.
- Legacy HTTP+SSE: one GET stream carries server messages, its `endpoint` event
  is resolved (absolute or relative) and used for POSTs, and answers are
  matched by id on the stream. The reader task ends with the session and is
  aborted on close.
- Header values are secrets exactly as env values are: the API returns names
  only, an absent `headers` keeps what was saved so editing a URL cannot wipe a
  token, and a value containing a newline is refused. `env` still belongs to a
  stdio server.
- Validation: transport is a closed set (stdio/http/sse); a stdio server still
  needs a command; a remote server needs an `http(s)` URL shaped like one. A
  save clears the field the chosen transport does not use, so a switched server
  cannot keep a stale command or URL.
- A registry written before this change reads as stdio, tested explicitly, so
  an existing `mcp.json` keeps working without a migration.
- Panel: Transport select, URL field and Headers field; a remote row shows
  `http <url>` instead of a command, and lists saved header names, never values.
- Tests: 247 pass (8 new: an old entry defaults to stdio, the transport set,
  URL shape, SSE payload extraction, multi-line `data:` joining, endpoint
  resolution, header names without values, and header merge preservation).
- Live smoke on :8765 against two stand-in servers: streamable HTTP handshake
  in 2 ms and `echo` → `remote: hello over http`; legacy SSE handshakes in
  5/3/2 ms and `echo` → `remote: hello over sse`, `ping` → `pong`. Both report
  `healthy`, version `2.0.0`, protocol `2024-11-05`, 2 tools; after a restart
  both read `unknown` until contacted. An unknown transport and an `ftp://`
  URL are both 400.
- Still missing, recorded rather than implied: a persistent connection pool
  (every operation still opens one session), per-step rediscovery, and an
  OAuth flow for remote servers — remote auth is headers the user writes.
- Version 1.37.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 53).

## 1.36.0

- Phase 7 close-out, connection half: an MCP server now has health, its own
  captured logs, and one reconnect attempt. `GET /api/mcp/health` returns what
  this process has observed; `POST /api/mcp/health/check` runs a real handshake
  per enabled server; `GET /api/mcp/logs?name=…` returns the captured lines.
- Health is observed, never assumed. A server that was never contacted reports
  `unknown` — not `healthy`, and not `unhealthy`. A handshake records `healthy`
  with its latency; a failure stores the reason it failed. The state lives in
  memory next to the registry (a persisted "healthy" is a lie after a reboot)
  and is recorded by every path that connects: Test, a health sweep, a tool
  call, and the agent run's discovery.
- Version tracking comes from the handshake itself: `serverInfo.version` and
  the negotiated `protocolVersion` are kept and shown beside the tool count, so
  an upgrade is visible without reading a command line.
- Server logs are captured instead of discarded. stderr is piped into a bounded
  in-memory ring — 200 lines, 400 characters per line — together with a few
  client events (`connected: …`, `tools/call …`, a failed handshake). A server
  that refuses to start now has a reason to read, not just a 502.
- Reconnect: one failed handshake is retried once on a brand-new session after
  250 ms. Nothing has been called at that point, so the retry cannot duplicate
  an effect; `tools/call` is never retried, which is what stops a side effect
  from running twice. Every operation still gets a fresh child process — there
  is no pool, and the docs keep saying so.
- Panel: each server row carries a health badge (Not checked / Healthy /
  Unhealthy) and a line with server version, protocol, tool count, latency and
  check time; a Logs button opens the captured output under the row; Check
  health sweeps every enabled server once and reports how many answered,
  failed, or were skipped by a saved rule.
- Tests: 239 pass (5 new: unknown before contact, a failure keeps its reason,
  a handshake records version/protocol/tools, the log ring keeps only the
  newest lines, and a long line is capped and attributed).
- Live smoke on :8765 against a real stdio server: `unknown` before contact →
  `healthy`, `version 1.0.0`, `protocol 2024-11-05`, `2 tools`, 39 ms after
  Test; the sweep returned `{echo: ok}`; the log held `connected: 2 tools,
  protocol 2024-11-05, 39 ms`; a missing `name` on the log route is a 400. A
  server that exits at once reported `unhealthy` with `(one retry also
  failed: …)` and its own stderr `boom`, captured twice — once per attempt.
- Still not built, and recorded as missing rather than implied: SSE/HTTP
  transports, per-step rediscovery, and a persistent connection pool.
- Version 1.36.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 52).

## 1.35.0

- Phase 7 close-out, agent half: an agent run can now call MCP tools.
  `Action::Mcp { server, tool, arguments }` joins the run's action list, and
  the action line is advertised only when the run was started with `allowMcp`
  *and* discovery actually found a tool — a model is never told about an
  action that would be refused.
- One catalogue per run, not per step: discovery runs once at run start,
  records an `mcp-tools` step (`server.tool` per line plus `unavailable: …`
  for a server that would not start) and stores the snapshot in
  `agent_runs.mcp_tools`. Step prompts read the snapshot, so a long run never
  respawns servers and a resumed run keeps the list it started with.
  Caps: 30 tools in the prompt, 20 per server, 200 characters of description.
- Gating has the same shape as command execution. `allowMcp` off → the step is
  refused and the run fails naming the missing permission. `allowMcp` on with
  auto-approve off → the run pauses in `awaiting_approval`, and the approval
  row names the tool and the arguments it would send. A `mcp_tool` deny rule
  on `server.tool` is applied first and vetoes the step by name.
- One execution path for both callers: `mcp::invoke` was extracted from
  `POST /api/mcp/call` and is now shared with the agent loop, so the argument
  check against the tool's own `inputSchema` and the `mcp_tool` audit record
  are identical whether a panel or a run made the call.
- `POST /api/runs` refuses `allowMcp` when no enabled server is registered,
  instead of accepting a checkbox that can only ever do nothing.
- Runs panel: a "May call MCP tools" checkbox on the run form, and each run
  card tags `mcp: N tools` from that run's own snapshot (0 when discovery
  found nothing).
- Fixed a defect found while testing this: `list_runs` selected the run
  columns without `model`, so every row failed to map and the panel read
  "No runs yet" while the runs existed — which would also have hidden the
  approval button this feature needs. The query is now a named constant
  (`RUN_LIST_SQL`) and a test inserts a run, maps it through `row_to_run` and
  asserts every field and the step count, so the two column orders cannot
  drift apart silently again.
- Tests: 234 pass (7 new: mcp action parsing with and without arguments, the
  approval summary and its approval-route round trip, the gate matrix
  (denied / needs approval / allowed), prompt advertisement rules, the list
  query column map, and two run-level cases — a paused run that goes on to
  attempt the call, and a run started without `allowMcp` that is refused).
- Version 1.35.0 across `app/Cargo.toml`, `app/Cargo.lock`,
  `app/tauri.conf.json`, the extension manifest and lock, and the Android
  `versionName` (code 51).
- Not built yet, and recorded as missing rather than implied: persistent MCP
  connections with health state, SSE/HTTP transports, and per-step
  rediscovery (the catalogue is fixed at run start).

## 1.34.0

- Settings → MCP: the 1.33.0 routes now have a panel. Register a server (name,
  command, arguments, environment, enabled), Edit it in place, Test it for a
  real handshake, and Remove it. Servers list their executable, environment
  names and enabled state, and a disabled server stays visibly disabled.
- Tools discovered by a test render as cards with the description and the
  `required` arguments. Clicking one selects it and pre-fills a JSON argument
  object shaped from its own schema — required fields only, with empty strings,
  `0`, `false`, `[]` or `{}` per declared type.
- Calls use the store's answer instead of guessing: the first click sends
  without approval, and when the store says `ask` the button becomes
  **Approve and call** so the second click is a real approval, not a silent
  retry. Results print raw, preserving whitespace, in a bounded block.
- `POST /api/mcp/servers` now keeps a server's saved environment when the
  request omits `env`: the API never returns values, so an edit that only
  changes the command must not wipe them. Sending `env: {}` still clears it.
- Version 1.34.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 50).
- Tests: 227 pass (one new case for the environment merge).

## 1.33.0

- Phase 7, first slice: an MCP registry that actually runs servers. `mcp.rs`
  keeps servers in `mcp.json` beside the config (name, command, args,
  environment, enabled) and speaks the stdio JSON-RPC 2.0 transport to them:
  `initialize` → `notifications/initialized` → `tools/list` → `tools/call`,
  newline-delimited, with a 20-second ceiling per operation and the child
  killed on every exit path.
- One session per operation rather than a pooled connection: a server that wedged
  or exited is retried by the next call instead of poisoning one long-lived
  handle. The cost is a process spawn per call; the benefit is that nothing
  leaks and nothing needs a supervisor yet.
- Discovery keeps each tool's real `inputSchema`, and calls are validated
  against it before they are sent: a missing `required` field or a declared
  scalar of the wrong type is refused locally with the field named. Only
  `required` presence and declared scalar types are enforced — this is not a
  JSON Schema implementation and does not claim to be.
- Every route that can start a process asks the policy store first, as
  `mcp_tool` against `server.tool` (a test asks on the server name alone). A
  `deny` rule refuses with 403, the default `ask` returns 409 with
  `needs_approval`, an approved retry proceeds, and each outcome is written to
  `permission_audit`.
- Routes: `GET /api/mcp`, `POST /api/mcp/servers`,
  `POST /api/mcp/servers/remove`, `POST /api/mcp/test`, `POST /api/mcp/call`
  (mutations behind the local-origin guard). Environment values are stored and
  passed to the child but never appear in a response — only their names do.
- Tests: 12 new cases (line encoding, id matching that ignores notifications
  and stray stdout, JSON-RPC errors, namespaced targets, name validation,
  registry round-trip and corrupt-file safety, env-name-only rendering,
  required/type validation, text flattening, schema caps); 226 pass.
- Not built yet, and recorded as missing rather than implied: persistent
  connection pooling, SSE/HTTP transports, a manager UI, and agent use of MCP
  tools (the routes work; the agent loop does not call them).
- Version 1.33.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 49).

## 1.32.0

- Settings → Permissions: the policy store from 1.31.0 gets a face. Pick an
  action (each option carries its real description and default as a tooltip),
  an optional target prefix, a decision and a scope, and the rule is saved
  through `POST /api/permissions/grant`. Rules in force are listed with their
  decision badge, scope and project, each with a Revoke button.
- The panel also shows the audit trail (`/api/permissions/audit?limit=25`), so
  a refusal the agent recorded while the panel was closed is visible on the
  next visit rather than needing a log dive.
- Re-read on every visit to the section, never cached: rules change while the
  app runs (an agent refusal, a router install), and a stale list would show a
  rule as gone when it is not.
- Saving a project-scoped rule without an open project is refused in the UI
  with the reason, instead of silently saving it against no project.
- Version 1.32.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 48).

## 1.31.0

- Phase 10, first slice: a central permission policy store. `permissions.rs`
  replaces "per-run booleans only" with rules that carry an action (`fs_read`,
  `fs_write`, `command`, `network`, `git`, `router_install`, `update`,
  `settings`, `mcp_tool`), an optional target prefix, a decision (`allow` /
  `ask` / `deny`) and a scope (`once`, `session`, `project`, `always`). Rules
  live in `permissions.json` beside the config, written atomically with private
  permissions; `once` and `session` rules stay in memory.
- Resolution is stated once and tested: the longest matching target wins, then
  deny over ask over allow, then the narrower scope. Reads default to `allow`;
  anything that can destroy, spend or expose defaults to `ask`. A rule can only
  ever make an action stricter — the run's own flags remain the ceiling.
- Enforcement is real, not advisory. Agent steps consult the store before the
  per-run gate: a `deny` rule stops the step, records the refusal and fails the
  run with the rule named. Router installs consult it before the plan is even
  considered, so an in-request approval cannot override a rule the user wrote.
- Audit trail: every rule add/revoke, every agent refusal and every approved
  install goes to a new `permission_audit` table (action, target, decision,
  scope, project, detail, timestamp), readable at `/api/permissions/audit`.
- Routes: `GET /api/permissions` (actions with their defaults plus rules in
  force), `POST /api/permissions/grant`, `POST /api/permissions/revoke` (both
  behind the local-origin guard) and `GET /api/permissions/audit?limit=`.
- Tests: 10 new cases (wire names, unknown words, default decisions, longest
  target, deny at a tie, scope tie-break, project isolation, rules-file
  round-trip plus corrupt-file safety, command prefix matching); 214 pass.
- Version 1.31.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 47).

## 1.30.0

- Phase 4 frontend, wired to the backend from 1.29.0. The composer has a
  **Think** control (model default / off / low / medium / high) that names what
  the next message asks for and explains the level in its tooltip. Typing
  `@Thinking`, `@Thinking:high` or `@Thinking high` at the start of a message
  shows a hint and lets the server act on it; `\@Thinking` stays literal on
  both sides and an unknown level is never swallowed.
- Reasoning summaries stream into a collapsible "Reasoning summary" card above
  the answer, fed by the provider's own summary channel — never presented as
  hidden chain-of-thought, and collapsible so it never hides the reply.
- Settings gained an **OpenAI Platform** section: platform key (write-only —
  the UI only ever learns `has_openai_key`), base URL, the global Thinking
  default and the agent-run default, plus a "Test platform key" button backed
  by `/api/openai/test`, which spends no tokens.
- The model menu lists the platform's own models under `openai:` ids once a key
  is saved, labelled so a platform pick is never mistaken for a router pick,
  with whatever capabilities the platform published beside them.
- Two launch defects found while smoke-testing the headless binary: a build
  without the `gui` feature no longer treats itself as windowed (it sent the
  shutdown signal immediately, so `./free-ai-studio` served nothing at all),
  and a process without a console no longer stops because `ctrl_c()` failed —
  only a real Ctrl+C stops the server.
- Verified against a live instance: `/api/thinking/levels` returns the four
  levels with the effort each one sends, `/api/openai/models` answers 400 with
  "Save an OpenAI Platform key…" when none exists, `POST /api/config`
  round-trips both Thinking defaults without echoing a key, and
  `POST /api/projects {"thinking":"medium"}` persists in the new column.
- Version 1.30.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 46).

## 1.29.0

- Phase 4 backend: Thinking control plus a native OpenAI Platform provider.
  `thinking.rs` holds the four levels, the fixed precedence (message > agent >
  project > global > provider default) and the `@Thinking` composer directive;
  `openai.rs` talks to the platform's own `/responses` endpoint with its own
  key, kept apart from the OpenAI-compatible router preset.
- A level becomes exactly one documented parameter: `reasoning: {effort,
  summary: "auto"}` on the Responses API and `reasoning_effort` on Chat
  Completions. "Off" sends **no** reasoning parameter, because several models
  answer HTTP 400 to `effort: "none"` — so an off turn is byte-for-byte the
  request it was before the feature existed.
- A router model carries `reasoning_effort` only when the router's own catalog
  says it accepts reasoning (`capabilities.reasoning == true`). A model that
  says `false` is refused with a line that names the two ways forward; an
  unknown answer sends nothing rather than guessing at a parameter.
- A platform model that still rejects the parameter is retried once without it
  and the refusal is remembered for the life of the process, so the turn still
  answers and `meta.reasoning_dropped` says what happened. The retry only ever
  fires on a 400 that names reasoning.
- `openai:` model ids route to the platform in buffered chat, streaming chat and
  agent calls (`call_router_with`), so a run can use a platform model without a
  new mode switch. Usage maps onto the existing token columns, plus
  `reasoning_tokens` when the platform reports it.
- Reasoning summaries arrive as their own SSE frame (`type: "reasoning"`), taken
  from the API's summary channel — never presented as hidden chain-of-thought.
  The Responses stream is translated into the frames the shared stream machine
  already records, so stop, partial-save and usage accounting stay shared.
- Per-project Thinking default persisted in a new `projects.thinking` column
  (migrated in place on both fresh and existing databases, `''` = inherit),
  with the project's level sitting between the global default and any
  per-message choice.
- Routes: `GET /api/openai/models` (platform catalog, `openai:` prefixed),
  `POST /api/openai/test` (key check that spends no tokens), and
  `GET /api/thinking/levels` (levels, the effort each one sends, and a truthful
  description). `GET|POST /api/config` gained the platform key and base URL plus
  both Thinking defaults; the key itself is never returned, only `has_openai_key`.
- Tests: 21 new cases (level parsing and aliases, precedence, the directive and
  its escape, request-body shape, output/summary extraction, usage mapping,
  stream event parsing, upstream frames, rejection detection, error wording);
  204 pass, clippy clean.
- Version 1.29.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 45).

## 1.28.0

- Phase 3, first slice: a model capability table that only repeats what the
  provider published. `capabilities.rs` reads OpenRouter-shaped `/models` rows
  (`architecture.input_modalities`, `supported_parameters`, `pricing`) and turns
  them into explicit flags; a provider that publishes nothing leaves every flag
  `null`, so the UI says "unknown" instead of promising tools or reasoning.
- Context limits come from that same table, so the nested
  `top_provider.context_length` OpenRouter uses is read in one place instead of
  two — and a plain OpenAI-compatible row that reports nothing stays at 0
  rather than being guessed from the model name.
- `GET /api/local/model-capabilities?name=…` asks the local runtime's own
  `/api/show` about one installed model: its `capabilities` list (tools,
  thinking, vision) and the model's real `*.context_length` from `model_info`.
  A model the runtime does not list answers 404 instead of an invented answer.
- The model menu shows each model's reported marks (tools / reasoning / json)
  and one line under the list stating what the chosen model was reported to do
  — or that nothing was reported. Picking a local model fetches its description
  on demand, so the panel never shows a stale model's capabilities.
- Pricing per million tokens is carried when a provider publishes both halves,
  which Phase 11 can use instead of inventing a price table.
- Tests: 8 capability cases (OpenRouter row, metadata-free row, empty parameter
  list, nested context limit, Ollama `/api/show` with and without a capability
  list, vision vs text-only input, null-not-false serialisation); 183 pass.
- Version 1.28.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 44).

## 1.27.0

- Adds the specification audit: `docs/FEATURE-STATUS.md` now records, phase by
  phase, what the tree actually delivers and what the specification still asks
  for. Evidence is the state of `main` at this commit — `cargo test --locked`
  reports **175 passed, 0 failed, 1 ignored** — plus the route and test counts
  read from the source, not a projection.
- The audit's finding, in short: Phases 1 and 2 are effectively delivered
  (Phase 1 waits only on install smokes and the dead-tag decision; Phase 2 on a
  configuration editor and dependency probing), Phases 5, 9, 10, 11, 14 and 16
  are largely in place, and **Phase 4 (Thinking + OpenAI Platform) and Phase 7
  (MCP) have no implementation at all**. Phase 13 is a flat saved-prompt list,
  not a workflow graph; Phases 3, 6, 8, 12, 15, 17 and 18 are partial in the
  specific ways the report lists.
- Records the ordering that follows from that: the Phase 3 model capability
  table first (it is Phase 4's only hard prerequisite), then Thinking and the
  Responses API, then the Phase 10 policy store that unblocks MCP.
- Version 1.27.0 across `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  the extension manifest and lock, and the Android `versionName` (code 43).

## 1.26.23

- The install flow is no longer one-way. `GET /api/routers/:id/update` compares
  the recorded artifact (an image digest, or a release asset's sha256) with what
  is published now, and returns the plan an update would run so the approval can
  name it. Nothing is downloaded or replaced by the check.
- `POST /api/routers/:id/update` needs that plan's digest. It pulls the newly
  pinned image **before** touching the running container, so a failed pull leaves
  the deployment exactly as it was; the named volume is kept across the replace.
- `POST /api/routers/:id/uninstall` removes the container (or an extracted
  runtime's directory), keeps the data volume by default, and needs `purge: true`
  to delete it. It also forgets the ownership flag, so shutdown cannot try to
  stop something that no longer exists.
- Install records now carry the artifact they installed, which is what makes an
  update comparable; older records simply read as "cannot say".
- Tests: the update plan tolerates an existing container where install refuses,
  and an install's artifact is the digest or the asset hash (175 tests pass).

## 1.26.22

- OmniRoute's install publishes its port on loopback only. A bare
  `-p 20128:20128` lets Docker bind every interface, so a gateway this app only
  ever reaches over `127.0.0.1` was answering the whole network. The command is
  now `127.0.0.1:20128:20128`, the same shape FreeLLMAPI's plan already used.
- The "router is not running" hint no longer tells people to `npm i -g omniroute`
  now that Studio has an install path for it.
- Tests: the pinned argv assertion names the loopback binding (173 tests pass).

## 1.26.21

- FreeLLMAPI installs from the image its own compose file names. The plan
  resolves the tag's manifest digest from `ghcr.io` (an anonymous pull token
  first), names `…freellmapi:latest@sha256:…` in both the displayed and executed
  command, and blocks when the digest cannot be resolved.
- The container is the deployment the project documents: published on loopback
  only, `PORT=3001` inside, the `freellmapi-data` volume at `/app/server/data`,
  `NODE_ENV=production`. Install, Start, Stop, Restart, auto-start and the boot
  sweep now work with whichever deployment exists — the user's compose project
  when there is one, else the container Studio installed — and shutdown still
  stops only what Studio started.
- A stopped container now reads as installed rather than missing, so the panel
  offers Start instead of a second install.
- Each gateway's plan is built on its own port; FreeLLMAPI was briefly planned
  against OmniRoute's port, which would have published it where the app never
  looks for it.
- Tests: the pinned FreeLLMAPI argv, both block cases (no Docker, container
  already there), registry-digest validation and the per-gateway port (173 tests
  pass).

## 1.26.20

- Installs can now fetch. A plan carries its downloads explicitly — the release
  asset URL together with the sha256 that release publishes — and the installer
  streams the archive while hashing it, discards it on any mismatch, and only
  then unpacks it (`zip`, or `tar.zst` through zstd). What was approved is the
  bytes that get extracted.
- Ollama has a real in-app install path. The plan resolves the latest official
  release, picks this platform's asset, reads the hash from that release's own
  `sha256sum.txt`, downloads into the app's data directory, verifies, unpacks,
  and runs the binary once with `--version`. An archive already on disk that
  still hashes correctly is not downloaded again.
- When the release or its published hash cannot be resolved the plan blocks with
  that reason instead of installing unverified bytes. FreeLLMAPI keeps its
  documented manual route: it now publishes a `ghcr.io` image, but giving it a
  second lifecycle is not done, so its plan still refuses rather than half-work.
- Tests: a local HTTP round trip proves a matching download is kept and a
  mismatching one is discarded, plus traversal-safe zip extraction, checksum-file
  parsing, the asset table and the pinned-plan digest (171 tests pass, plus one
  `#[ignore]`d network test that was run separately: it verifies a real release
  asset against the hash that release publishes).

## 1.26.19

- Image installs are now pinned. The plan resolves the tag's manifest digest from
  the registry (`sha256:…`) and names it in both the displayed command and the
  executed one, so an install cannot silently fetch a different image than the
  one that was approved. If the digest cannot be resolved the install is refused
  with that reason instead of falling back to the moving tag.
- Start no longer creates containers. Creating one is an install, and installing
  is the reviewed act added in 1.26.18 — `docker run` now happens only through the
  approved plan, so Start either starts an existing container or says that
  OmniRoute is not installed yet and points at Install.
- Tests: `plan_pins_the_image_to_the_registry_digest`,
  `tag_digest_parsing_reads_the_manifest_list`, `image_split_defaults_to_the_latest_tag`
  (163 Rust tests pass).

## 1.26.18

- Phase 2 slice 6: installing a router is now a reviewed act. `GET
  /api/routers/:id/install-plan` returns what an install would do without doing
  any of it — the exact argv per step, what gets written, how the content is
  verified, and the prerequisites it measured (Docker on PATH, whether a
  container already exists).
- `POST /api/routers/:id/install` runs nothing unless the caller sends
  `{"approve": true}` **and** the digest of the plan it was shown. The plan is
  rebuilt first, so a machine that changed since the page loaded refuses instead
  of installing something else (409). Unsupported routers, unmet prerequisites
  and missing approval answer 409/409/400 before any process is spawned.
- Only OmniRoute has an in-app install path (the `docker run` argv is now built
  once and shared with Start, so the plan cannot name different commands than
  the ones that execute). FreeLLMAPI and Ollama have no Studio-side downloader,
  so their plans refuse and name the documented route instead of running an
  installer Studio cannot verify. Image tags are not pinned to digests yet, and
  the plan says so rather than implying a checksum it does not check.
- Installs are recorded per router in `router-installs.json` (method, source,
  digest, epoch time, outcome, message), including failures; `GET
  /api/routers/:id/install` reads the record back.
- Tests: `omniroute_plan_names_the_exact_command`,
  `omniroute_plan_is_blocked_without_docker_or_when_installed`,
  `plans_without_an_in_app_path_say_so`, `digest_is_stable_and_binds_the_steps`,
  `approval_must_match_the_plan_that_runs`, `install_record_round_trips_per_router`,
  `install_plan_serialises_every_field_the_ui_shows` (160 Rust tests pass).

## 1.26.17

- Phase 2 slice 5: per-router launch preferences, and a startup sweep that
  honours them. `POST /api/routers/:id/auto-start` stores the choice in
  `~/.config/free-ai-studio/router-prefs.json` (written atomically, owner-only),
  and `/api/routers` now reports `auto_start` per router, shown as a
  "Start with Studio" checkbox in the Router view.
- The sweep replaces the unconditional FreeLLMAPI start at boot. It starts a
  router only when the user enabled it, this build can start it, it is installed
  here and it is **not already answering** — so a router the user started is
  never adopted, and closing Studio still cannot stop it. FreeLLMAPI keeps its
  old default of starting at launch; the other entries default to off.
- A stopped OmniRoute container now counts as installed (`docker ps -a`), so it
  reads as "Stopped" instead of "Not installed" and auto-start has something to
  start. The inline duplicate of that `docker ps -a` check is gone.
- Tests: `registry_defaults_keep_freellmapi_launching`,
  `preferences_round_trip_and_keep_registry_defaults` (including a corrupt file),
  `auto_start_rule_never_adopts_what_already_answers`,
  `only_routers_with_a_lifecycle_can_auto_start`,
  `set_auto_start_refuses_an_unknown_router` (153 Rust tests pass).

## 1.26.16

- Phase 2 slice 4: the Router view can show a router's own recent output.
  `GET /api/routers/:id/logs?tail=N` reads `docker compose logs --tail` for
  FreeLLMAPI and `docker logs --tail` for OmniRoute. Both streams are kept,
  because a container's stderr is part of its log and dropping it would hide the
  failures people open logs to find.
- The tail is clamped (1–1000 lines, 200 by default) and the result is capped at
  64 KiB, so one read cannot pull an unbounded log into the response; `truncated`
  reports when newer lines displaced older ones.
- Asking for the local runtime's logs is refused with the reason rather than
  answered with an empty box: that output belongs to the user's own service
  manager, not to Studio.
- Tests: `tail_is_clamped_to_what_one_read_may_ask_for`,
  `tail_keeps_the_newest_lines_and_says_what_it_cut`,
  `logs_refuse_routers_without_a_managed_process`,
  `router_logs_serialise_what_the_panel_renders` (148 Rust tests pass).

## 1.26.15

- Phase 2 slice 3: the Router view can test one router on demand.
  `POST /api/routers/:id/test` sends a real request to that router's documented
  health path (`GET /v1/models` for FreeLLMAPI and OmniRoute, `GET /api/tags` for
  the local runtime) and reports the status code and measured latency, so "up but
  not answering" is distinguishable from "wants a key": a 401 counts as an
  answered endpoint and the message says so. Checked against
  `scripts/mock-router.py` — HTTP 200 in 1 ms — and against nothing listening on
  port 11434, which reports no answer after the 3 s timeout.
- `POST /api/routers/:id/restart` composes the existing explicit stop and start
  paths, so the ownership rules are unchanged: OmniRoute still refuses to stop a
  gateway Studio did not start, and the local runtime has no lifecycle here to
  restart (502 with the reason). An unknown id answers 404, and both routes sit
  behind the loopback-origin guard (a foreign `Origin` gets 403).
- The registry documented `GET /health` for OmniRoute while its probe has always
  called `GET /v1/models`. The tester sends exactly what the probe sends, so the
  entry now reads `/v1/models` and the two cannot drift apart again.
- Tests: `every_health_probe_is_a_get_path`, `connection_test_separates_an_answer_from_no_answer`,
  `connection_test_serialises_what_the_ui_renders`, `restart_refuses_what_this_build_cannot_own`
  (144 Rust tests pass).

## 1.26.14

- `GET /api/routers` probes FreeLLMAPI, OmniRoute and the local runtime
  concurrently instead of one after another, so the endpoint costs roughly the
  slowest probe rather than their sum (it took about 6.5 s with nothing
  installed). Each probe is still the very function its own endpoint calls, so
  the registry cannot report a state the individual endpoints disagree with.

## 1.26.13

- The Router view now lists every registered router from `GET /api/routers` with
  the state its probe measured: healthy, up but not answering, stopped, or not
  installed, plus endpoint, reported version and whether Studio started it.
  Start/Stop are offered only for routers this build can manage locally, and
  Stop only for one Studio started or one that is healthy, so an external
  runtime is never started or stopped from here.
- The panel reuses the console status card's probes, so the list and the card
  cannot disagree.

## 1.26.12

- Android CI: "Check committed whitespace" runs `git show --check HEAD`, which
  treats a carriage return as trailing whitespace. `android-ci.yml` (and
  `AGENTS.md` in 1.26.10) were committed with CRLF while git stores LF, so every
  line of those files counted as a violation and the release gate failed on an
  otherwise fully green job. Both are LF now, and `git diff --cached --check`
  is run before committing so the same mistake cannot ship again.

## 1.26.11

- Fix the desktop CI regression: clippy's `manual_contains` lint rejected the
  registry's `lists()` helper, so `Run clippy` exited 101 on 1.26.9 and 1.26.10.
  `cargo build` and `cargo test` do not run that lint, which is why local runs
  were green. `cargo clippy --locked --all-targets -- -D warnings` now passes.
- Restore `AGENTS.md` to LF. The 1.26.10 edit committed the whole file as CRLF,
  turning a four-line addition into a 92-line diff.
- Android CI: the aggregate gate now names the failing step in an annotation
  instead of printing only its outcome string, because the job log is not always
  readable. The underlying gate failure is still open and is not yet diagnosed.

## 1.26.10

- Repository convention: commit subjects start with the version number, and no
  commit carries AI attribution (no `Co-Authored-By` or "Generated with"
  trailer). Recorded in `AGENTS.md` so it survives across sessions.

## 1.26.9

- Phase 2 slice 2: `GET /api/routers` returns every registered router with its
  measured state — installed, reachable, running, owned-by-app, endpoint,
  version and message — next to its capabilities and the host OS/architecture.
  The probes are the existing ones (`router::status`, `router::omniroute_status`,
  `local::fetch_version`), so the list cannot disagree with `/api/router`,
  `/api/omniroute` or `/api/local/status`.
- `healthy` is defined as "the health probe answered"; a container that is up but
  silent reports `running: true, healthy: false`. Both cases are unit-tested.
- Verified locally: HTTP 200 with three routers and real host values on Windows;
  140 Rust tests pass.

## 1.26.8

- Phase 2 slice 1: `app/src/router_registry.rs` adds the router registry the
  Router Manager builds on — description, supported OS and architectures,
  install methods, configuration location, HTTP health probe and docs per
  router, plus platform gating (Android never claims a local install, remote
  stays available) and architecture normalisation for real release-asset
  spellings. It is data plus tests only: lifecycle still lives in `router.rs`,
  which this module points at instead of replacing. 10 new unit tests.
- Not tagged: the registry is not reachable from the API yet (slice 2 adds the
  live probes and `/api/routers`), so there is nothing user-visible to release.

## 1.26.7

- Documentation only: Phase 1 is signed off in `docs/PROGRESS.md` with the run
  IDs and per-issue verdicts, `docs/KNOWN-ISSUES.md` records the real cause of
  the macOS cancellation and of the fatal AppStream check, and
  `docs/FEATURE-STATUS.md` matches. No behaviour change, so this commit is not
  tagged.

## 1.26.6

- AppImage (Linux) builds again. The v1.26.5 run failed `appstreamcli
  validate-tree`, which appimagetool now treats as fatal and which fails on
  warnings as well as errors: `cid-desktopapp-is-not-rdns` and
  `content-rating-missing`. The desktop entry, the AppStream file and the
  component ID now all use `io.github.sairajkhilariarch.SaarthiStudio`, and an
  OARS 1.1 age rating is declared, so the tree validates clean.
- The .deb now installs the AppStream metadata as well
  (`usr/share/metainfo/...`), so software centres can show the app.
- macOS: the job installs both `aarch64-apple-darwin` and
  `x86_64-apple-darwin` targets. On the new Intel runner only the x86_64 std was
  present, so the arm64 slice build exited 101.

## 1.26.5

- Linux AppImage: the AppStream file is now valid AppStream 1.x (`<developer>`
  instead of the deprecated `<developer_name>`, a `<provides><binary>` entry and
  real screenshots), and the AppDir is a complete freedesktop tree, because
  `appimagetool` runs `appstreamcli validate-tree` over it and aborts on a
  non-zero result. The check now runs first and echoes each finding as an
  annotation, so a failure is diagnosable from the run page.
- macOS: `runs-on: macos-15-intel`. The v1.26.4 job was never acquired by an
  arm64 hosted runner ("not acquired ... after multiple attempts", with GitHub's
  macOS-arm64 capacity notices) and was cancelled; the job cross-builds the
  aarch64 slice anyway, so the universal binary is unchanged.

## 1.26.4

- Phase 1 release reliability: one version parser for Linux, macOS and Windows
  (`scripts/ci/version.sh`) strips CR/LF/whitespace and validates semver, so a
  CRLF checkout can no longer corrupt an artifact name.
- New artifact gate (`scripts/ci/verify-artifacts.sh`) rejects control characters
  and spaces in artifact names and verifies `SHA256SUMS.txt` before each upload;
  `scripts/ci/test-version.sh` runs those regressions in Desktop CI on every push.
- Release runs are serialised per ref, and the AppImage now ships AppStream
  metadata (`app/packaging/free-ai-suite.appdata.xml`).
- Added the audit and engineering docs: `docs/ARCHITECTURE.md`,
  `docs/FEATURE-STATUS.md`, `docs/IMPLEMENTATION-SPECIFICATION.md`,
  `docs/MASTER-ROADMAP.md`, `docs/TESTING-STRATEGY.md`, `docs/KNOWN-ISSUES.md`,
  `docs/PROGRESS.md`.

## 1.26.3

- Repair the release workflow's version read. An earlier bump left a raw
  carriage return in the `sed` line, which made the YAML invalid, so v1.26.2
  produced no run and no artifacts. Same step is now a block scalar that
  strips whitespace, and 1.26.3 is the first tag built by the fixed pipeline.

## 1.26.2

- Fix the release pipeline: version reads now strip a trailing carriage
  return, and the tracked manifests are back to LF, so artifacts are named
  `free-ai-suite_1.26.2_*` instead of carrying an embedded `` that made
  `upload-artifact` reject them (v1.26.0 and v1.26.1 built and tested fine
  but never attached a single asset).

## 1.26.1

### IDE workbench shell (desktop)

- The UI is now a VS Code-style workbench: an activity bar (Explorer, Search,
  Source control, Run, Modules, Agents, Skills, Projects, Settings), breadcrumbs,
  tab context menus, a docked AI panel, a five-tab bottom panel (Terminal,
  Problems, AI runs, Tasks, Output) and a live status bar.
- Chat no longer competes with file tabs: the assistant lives in the right dock
  with an Assistant/Router tab pair, so code and conversation stay side by side.
- The editor gained syntax highlighting (Rust, JS/TS/Java, Python, shell, SQL),
  in-file find with a match count and F3/Shift+F3, jump-to-line from search, and
  font size / tab size / wrap preferences.
- Two new read-only routes: `/api/workspace/search` (bounded text grep that
  skips build output, binaries and symlinks) and `/api/workspace/vcs` (parsed
  `git status --porcelain=v2`, honest when the folder is not a repository).
- Problems records real failed requests and saves; Output logs real mutations.
  No pane fabricates data: folding, split editors, debugging and a test runner
  are absent because the backend has no such capability yet.

## 1.26.0

### Hybrid AI (Step 3): cloud plans, local executes

- New per-run agent mode `hybrid`: the cloud router writes a compact
  implementation plan (`plan` step) from the goal and the capped workspace
  outline, then every execution step runs on the configured local Ollama
  model, so edits and commands stay on this machine.
- Escalation: when a local step fails, one bounded cloud call returns concrete
  guidance — logged as an `escalate` step and appended to the retry prompt —
  and a second failure fails the run instead of looping. The cloud only sees
  the goal, the step log and the local error, never chat history.
- Runs record their `mode`; the run composer gained a "Model mode" picker and
  `/api/runs` rejects unknown modes and hybrid runs with no local model.

### Multi-agent team mode and local-model robustness

- New per-run agent mode `team`: the executor works, then an independent
  REVIEWER model checks the result. Findings land as a `review` step and ride
  back on one repair round; a run that never passed review is not reported as
  reviewer-approved.
- Local executor loop hardened for 1.5B-class models: placeholder echoes of
  the instruction examples (exact and prefixed paths/commands) are corrected
  in a single step instead of a drift spiral, a thrown-in `fail` verdict goes
  to review instead of silently ending the run, and the JSON repair chain
  rebuilds unclosed `"""` value fences (closing at the next code fence and
  undoing `\'` double-escapes) as its last fallback — a shape taken live from
  a real repair round.
- Local chat requests now pin sampling options (8k context, 2k output,
  repeat penalty 1.1, temperature 0.4) so Ollama's runner stops aborting long
  turns with "token repeat limit reached" on CPU.
- Verified end to end on the machine: a broken `greet.py` diagnosed by the
  local executor from the exit code, rewritten, verified (`python3 greet.py`
  → `Hello, Saarthi!`, exit 0), reviewer APPROVE, run succeeded in team mode.

## 1.25.0

### CI/CD hygiene (Step 1)

- Desktop `cargo clippy` and workspace `rustfmt --check` both pass locally:
  needless borrows and `unnecessary_mut_passed` in agent tests fixed, the stale
  MSRV declaration (1.75) corrected to 1.80 (required by
  `std::sync::LazyLock`), the unused `privacy::notice_text` helper and its test
  deleted, and one unnecessary `Ok(...?)` wrap replaced in `main.rs`.
- Static file serving gained an `image/svg+xml` MIME mapping so the new
  favicon is not served as an opaque octet stream.
- Full local verification recorded in the test report: desktop tests 112/112,
  extension 22/22, Android gradle tests + lint green.

## 1.24.0

- Local LLM support: Local AI page wired to a real Ollama adapter
  (`app/src/local.rs`), runtime detection, honest failure paths, model
  selection/pull, and hardware diagnostics — no silent fallback to remote
  providers.

## 1.23.0

- Privacy pass (shared Rust core + VS Code port), agent runtime parity for VS
  Code and Android, adaptive model routing, reproducible run records, and
  universal (arm64 + x86_64) macOS packaging in the release workflow.

## 1.22.0

### Usage page: full analytics

- The request log is now served with server-side pagination (10 per page,
  numbered pager over the whole history, not a 50-row cap), column sorting
  (time, status, provider, model, latency, tokens, details) and filters
  (model, provider, date range) that keep their state across pages.
- New charts computed from real data: requests per day (ok/failed stacked) and
  requests/tokens/latency by model. Stat cards follow the active filter.
- The VS Code built-in server implements the same query shape from its store.

### Agents page

- Search box, configurable page size (10/25/50, remembered) and numbered
  pagination; ten agents per page by default.

### Model, mode and skill controls actually reach the backend

- The composer's Build/Plan toggle and per-message model picker now bind,
  restore their saved choice at startup, and ride with every request; the
  backend honours `model` and `mode` (plan) in both chat routes.
- Saving Settings reloads the model catalog, so a changed endpoint shows up
  without restarting.

### Dashboard button

- The OpenRouter/router dashboard button derives its URL from the active
  backend (OpenRouter's own settings page, the gateway root for self-hosted
  presets) and falls back to asking the server to open it when the window
  blocks popups.

### Cross-platform

- New macOS job in Desktop Release: arm64 portable tarball (unsigned —
  documented in README with the Gatekeeper workaround).
- The VS Code extension's built-in server gains parity with the desktop API:
  streaming chat, skills (list/detail/enable), projects (create, rename,
  archive, activate, cascade-delete threads), workflows, model catalog,
  usage pagination, artifacts listing and URL opening. Known gaps (no PTY
  terminal, no gateway process supervision, folder scoped to VS Code's open
  workspace) are documented in README and answered with honest errors.

### Spaces and Connections copy

- The "Spaces" page now says what a space is; the "Links" entry is renamed
  "Connections" with a description of what its status rows show.

### Branding

- Every user-facing string says **Saarthi Studio** (window title, page title,
  folder pickers, VS Code display name and command titles, README). Internal
  identifiers (`free-ai-studio`, `free-ai-suite`) stay unchanged so existing
  installs and data keep working.

## 1.21.1

### Fixed: "no reply" from the Linux app

- The real cause: picking the OmniRoute backend preset rewrote the router URL
  to `localhost:20128` — and with OmniRoute not installed, every request
  failed. Selecting the OmniRoute preset now checks the gateway and offers a
  one-click start; when a self-hosted gateway is unreachable, the error says
  exactly how to start it (OmniRoute or the FreeLLMAPI Docker chain).
- The stored router URL was restored to `http://localhost:3001/v1`.

### Android: installable APK

- Release APK/AAB are signed with the auto-generated debug keystore instead of
  shipping unsigned (Android refuses unsigned APKs). Assets renamed
  `free-ai-studio-android.apk` / `.aab`.

### Docs: hosted website

- README gains an "Open it as a website (hosted mode)" section: the site URL,
  headless/port flags, phone access via SSH tunnel, and the Android Host API
  pairing steps.

## 1.21.0

### Twelve backend presets and OmniRoute control

- Settings offers a backend preset grid: FreeLLMAPI, OmniRoute, Hugging Face,
  Groq, Cerebras, Gemini, OpenAI, OpenRouter, Mistral, Cloudflare Workers AI,
  Together and Novita — picking one fills the URL, model and key hint, and
  links to that platform's key page.
- OmniRoute joins FreeLLMAPI as a managed backend: the Integrations view can
  start/stop it (`npm i -g omniroute`, gateway on `localhost:20128`), app-owned
  processes are shut down at exit, and its status card shows whether the app
  or the user started it.

### Deep search

- The composer gains a **Deep** toggle: research mode runs up to 3 planned web
  queries (Exa or Tavily keys in Settings → Deep search) and answers with
  cited `[1]` sources listed under the reply. Every step — planning, each
  query, each source — streams in as a live checklist next to the answer.
- Direct Gemini and OpenAI backends use their built-in web tools
  (`google_search` grounding, Responses `web_search`) with no search key.
- Hard-bounded: 3 queries, 6 results each, 12 sources, capped evidence.

### Freebuff-style shell

- Settings is grouped into Connection / Chat context / Deep search sections.
- User messages render as filled bubbles; thread rows show relative
  timestamps; the top bar carries a New-thread button.

### Fixes

- The VS Code extension build recurses into the shared UI tree so bundled
  skills land in the VSIX.

## 1.20.5

### Always-on skills from `~/.agents/skills`

- A third discovery root reads every `SKILL.md` pack under `~/.agents/skills`
  alongside the project folder and the user config dir.
- YAML descriptions written as folded blocks (`description: >` or `|`) now
  parse to their real text instead of a stray `" > "`, so auto-suggest matches
  on the whole summary.

### Composer skill picker and auto-suggest

- The composer gains a "Skill" picker, an attached-skill chip and a keyword
  auto-suggest: the first enabled skill whose id/name/description contains a
  five-letter-plus word of the draft message is offered and can be cleared
  before sending. The request body now passes `skill` through to the router.

### Five new views

- **Workspaces** — the server-backed project list with switch/rename/archive/
  delete and the open-folder controls collected on one page.
- **Agents** — every enabled skill as a runnable agent; Attach flips the same
  `activeSkillId` state the composer chip reads.
- **Integrations** — live cards for the router, the workspace and a chat
  round-trip, each with its own Test button.
- **Artifacts** — a depth- and count-bounded flat file listing of the open
  folder, grouped by extension (`/api/workspace/files`).
- **Automation** — saved prompt recipes with an optional skill, persisted in
  `~/.config/free-ai-studio/workflows.json`; running one loads the prompt and
  skill into a fresh chat round.

### Honest router pill

- On launch the status pill asks the server for the router's real state
  (`GET /api/router`) instead of forever showing "not tested".

Release **1.19.0** is the current version in `Cargo.toml` and `tauri.conf.json`
(and Android `versionName 1.19.0`, code `16`). It ships the window shell, the
token-budgeted context packer, the router lifecycle, the router console, the
Windows build and the VS Code extension.

Seven commits sit on top of the release (v1.19.1–v1.19.7); each message starts
with its tag and lists exactly what it delivers. v1.19.1–v1.19.6 are already on
`origin/main` — a push recorded under this checkout's git identity at
2026-10-24 11:21, after those six commits were made. **v1.19.7 is local and has
not been pushed.**

## 1.19.0 — the release itself

### Window shell instead of a browser tab

- Tauri 2 loads the local UI in the system webview (WebKitGTK on Linux,
  WebView2 on Windows), so there is no bundled Chromium. `--browser` keeps the
  browser flow and `--no-gui` runs headless for CI and the extension.
- Closing the window runs the same shutdown path as Ctrl+C: the local server
  stops, then the router is brought down if this run started it.
- `app/icons/` carries the sparkle artwork as PNG and ICO for the window and
  the Windows exe.

### Token-budgeted context (the "fewer tokens like Freebuff" part)

- `app/src/context.rs`: 6k default budget (1k–32k ceiling), a newest-N message
  window (2–24), per-message caps, head/tail cropping that keeps both ends, and
  one-line markers so attachments from earlier turns are not re-sent.
- A names-only outline of the open folder goes into the system prompt, never
  file contents.
- Settings expose budget, message window and the outline toggle; the composer
  shows a live `~used / budget` meter and the router's real `context_tokens`
  after each reply.

### Router lifecycle — stop only what the app started

- Studio starts FreeLLMAPI only when its port is not answering, records who
  started it in `router-owned.json`, and runs `docker compose down` on exit
  only when it was the one that started it.
- `free-ai-studio router status|start|stop` exposes the same logic, and the
  console has Start/Stop/Refresh.

### Router console — FreeLLMAPI dashboard features inside the app

- `app/src/router_console.rs` bridges the dashboard API over loopback with a
  desktop session (falling back to a one-time email/password sign-in), forwards
  only allow-listed admin families, and re-mints a stale session exactly once.
- Console tabs: providers (add, enable, delete keys), model catalog with "use
  this model", routing strategy with raw settings, analytics summary and
  by-model table, live logs with clear, and the free-tier budget table.
- `scripts/mock-router.py` is a dependency-free stand-in for the router so the
  console is testable without Docker.

### Windows + releases

- Portable `free-ai-studio.exe` build on `windows-latest`, Windows/macOS folder
  pickers, and cargo linker flags scoped to Linux so one config works both
  places.
- `Desktop Release` builds and attaches all four platforms to one GitHub
  release: Linux tarball/`.deb`/AppImage, the Windows zip, the unsigned Android
  APK/AAB, and the VS Code `.vsix`.

### VS Code extension

- Probes the desktop app; embeds it when it is running and embeddable,
  otherwise starts a built-in server inside the extension that serves the same
  UI and API on a free loopback port, scoped to the open workspace.
- 13 tests cover the packer, store, workspace rules, console path guard, probe
  and both server surfaces. The `.vsix` is about 56 KB.

## Follow-up commits

### v1.19.1 — add window icon assets so the Tauri shell can build

`tauri::generate_context!` panicked with "failed to open icon
`app/icons/icon.png`" because `bundle.icon` named nothing. Adds a 512px PNG and
a 256px PNG-compressed ICO of the four-point sparkle and lists both in
`tauri.conf.json`.

### v1.19.2 — never take ownership of a router this app did not start

`router start` claimed ownership even when the router was already answering, so
app exit could have stopped a router the user launched. `manual_start` now
leaves the flag alone when the port is already up, matching `ensure_started`;
the compose-task error mapping in `shutdown_cleanup` is fixed too. Verified
start/stop/exit against a stub `docker`.

### v1.19.3 — silence the router-console clippy gate

Replaces `assert_eq!(… .is_err(), true)` with `assert!(… .is_err())`, the one
`-D warnings` failure in the desktop CI gate.

### v1.19.4 — rustfmt the crate and remove two dead code paths

`cargo fmt --all` across the touched files, plus removal of the never-read
`Boot.gui_mode` field and the unused `context::outline_from_entries` helper
(the chat path uses `workspace::outline()`), so `cargo clippy --locked
--all-targets -- -D warnings` passes.

### v1.19.5 — keep the composer token meter in step with its inputs

The meter resets to a cheap estimate while a draft is being written and
refreshes on every path that attaches or clears a file, so it never shows the
previous reply's count while you are typing.

### v1.19.6 — this changelog

Documents the release and the follow-up commits above.

### v1.19.7 — align every release artifact on 1.19.0

- `README.md` and `android/README.md` still advertised `1.12.0` (version code
  `15`); both now say `1.19.0` (code `16`), matching `build.gradle.kts` and
  `tauri.conf.json`.
- The extension's `package.json` and its embedded-server `/api/health` payload
  both reported `1.0.0`; both are `1.19.0` now, so the `.vsix`, the desktop
  binary and the APK report one suite version.
- The `Desktop Release` header comment now mentions the Android job it was
  already running, and `/dist/` is git-ignored so a local `vsce package` does
  not dirty the tree.
- The release APK/AAB were rebuilt at `1.19.0` (code `16`) — confirmed with
  `aapt dump badging` — and `vsce package` produced the 56 KB `.vsix`.

## Verification

- `cargo fmt --all --check`, `cargo test --locked` (57/57), `cargo clippy
  --locked --all-targets -- -D warnings` — all clean.
- Browser pass against the mock router: console tabs, key add/enable/delete,
  model select, strategy save, analytics, logs, budget, message actions row,
  token meter, settings persistence.
- Router lifecycle: compose up on boot, `docker compose down` on SIGINT, and a
  router started by the user stays running after exit.
- Extension: `npm test` (13/13), `tsc --noEmit`, `vsce package` → `dist/
  free-ai-studio-vscode.vsix` (56 KB).
- Android: `gradle assembleRelease bundleRelease` succeeds; `aapt dump
  badging` on the release APK reports `versionCode='16' versionName='1.19.0'`.
