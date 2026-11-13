# Feature status report — audit of `main` @ 2.1.0, revised through 3.15.0

Read against the master specification phase by phase. Status vocabulary:
**Working** (route + persistence + UI verified in code or by a recorded run),
**Partial** (exists but a stated requirement is unmet), **Missing** (no
implementation), **Broken** (exists and fails).

## Evidence from the 2.1.0 audit

| Check | Command | Result |
|---|---|---|
| Rust suite | `cd app && cargo test --locked` | see the count recorded in `PROGRESS.md` for this version (test functions in tree: 504) |
| Route registrations | `grep -c '\.route(' app/src/{main,agent,host_api,git}.rs` | 44 / 15 / 8 / 28 |
| Test functions in tree | `grep -rc '#\[tokio::test\]\|#\[test\]' app/src` | 504 |
| Last green release | run `41884439627` (`v1.26.6`) | all six jobs success, 9 assets |
| Release line | `8ff7c5e` … `2.27.0` | 1.26.4 → 2.1.0 all shipped, LF manifests |

These are the 2.1.0 snapshot, not a current count; later revisions are appended below and current release evidence is in `PROGRESS.md`.

**Revision 3.45.0 (2026-11-13):** Phase 7's explicit disconnect exists.
`POST /api/mcp/:name/disconnect` closes a warm session while keeping its
settings, reports whether there was one, and leaves an unknown server a 404. The
panel exposes it beside Test. A warm stdio process being killed by this action
was not exercised live.
**Revision 3.44.0 (2026-11-13):** Phase 17's workflow failures are on the
diagnostics timeline. Failed, cancelled and interrupted runs carry their run id,
graph name and stored error; the server accepts a `workflow` filter, and the
24-hour line counts them. Successful runs are deliberately left out.
**Revision 3.43.0 (2026-11-13):** Phase 11's per-agent usage is visible.
`/api/usage` rolls up `agent_runs` over 24 hours by profile with runs and the
run's own token total, and the Usage page shows it in a **By agent** card. Blank
profiles are grouped, and no per-run spend is invented from multi-call runs.
**Revision 3.42.0 (2026-11-13):** Phase 11's rate-limit monitoring is real.
Provider health counts a model's 24-hour refusals only when its stored error
mentions 429 or a rate limit, exposes `rate_limited` and `last_rate_limit_at`,
and the card prints the count. No rate limit is inferred from latency.
**Revision 3.41.0 (2026-11-13):** Phase 6's compatibility check is real. A
manifest may state `requires: X.Y.Z`; an older build lists it as incompatible,
never selects or attaches it, refuses an import, and a direct pick answers 422
with both versions. Install/update rewrites preserve the line, and malformed
requirements are load errors rather than silent comparisons.
**Revision 3.40.0 (2026-11-13):** Phase 11's per-project usage is shown. The
existing 24-hour roll-up now also sums `total_tokens`, and the Usage page has a
**By project** card fed by `by_project` (requests, tokens, estimated spend and
unpriced successes), with `(no project)` for unassigned conversations. The card
uses the roll-up's own 24-hour window, not the table filters, and says so.
Verified live against a seeded database.
**Revision 3.39.1 (2026-11-13):** Phase 16's locked and full-database cases
are covered. One test proves a second connection waits behind the 10 s
`busy_timeout` instead of failing while another holds `BEGIN IMMEDIATE`; another
uses `PRAGMA max_page_count` to make SQLite answer `SQLITE_FULL`, then checks the
error, the unchanged row count, `quick_check` and that deleting rows still works.
This is not a real filesystem ENOSPC; that privileged case and browser layout
remain open.
**Revision 3.39.0 (2026-11-13):** Install a skill from a local directory.
Import only cloned; a plain folder without `.git` is now copied through the same
staging and validation path as a clone (root `SKILL.md`, 200 files / 2 MiB, no
symlinks, slug rewrite, staging removed either way), gated on `fs_write` for the
skills folder. A folder holding `.git` still clones and keeps its refreshable
origin; a copied folder has no origin and a branch/tag with it is refused. The
import form names both sources.
**Revision 3.38.0 (2026-11-13):** The Windows portable uninstall finishes.
The emptied program folder used to stay until the person deleted it; the route
now schedules a detached, windowless PowerShell that waits for the process to
exit and retries `Remove-Item` for a minute, and reports `scheduled` beside
`pending` (with `complete` still false). Settings → About says the folder will be
deleted when the window closes, or keeps the manual instruction if scheduling
failed. The path is validated and quoted; the POSIX path is unchanged. Verified
on a staged portable copy: `scheduled: true`, and the folder was gone after the
server exited while kept data remained.
**Revision 3.37.0 (2026-11-13):** Refresh a Git-imported skill. Imports now
record their origin and ref, and `POST /api/skills/:id/refresh` re-clones that
recorded repository (never a URL from the request) under Import's bounds,
requiring the same name and keeping the replaced copy under `.removed/`; a
failed copy restores the old folder. A manual edit keeps the origin, the skill
list and detail expose it, and the detail pane offers Update from Git with a
confirmation. Skills imported before 3.37.0 have no origin and need one
remove-and-import; the clone remains shallow and only name/description/version
are carried into the installed manifest.
**Revision 3.36.0 (2026-11-13):** Skills can be imported from Git. The new
`POST /api/skills/import` clones an https URL or a local clone into a temporary
folder, requires a root `SKILL.md`, refuses more than 200 files or 2 MiB and any
symlink, and installs under the user skills folder, rewriting the manifest to
name that folder so discovery keeps the id. The clone is removed either way, an
installed id is refused, and the route is gated on `git`. Both skill roots now
follow `FREE_AI_STUDIO_HOME`. Updating a Git-imported skill still means removing
and importing it again (KNOWN-ISSUES 43).
**Revision 3.35.0 (2026-11-13):** Replace in file. The find bar now has a
Replace field: Replace (Enter) rewrites the match under the selection or the
bar's current one and moves to the next, All (Shift+Enter) rewrites every
occurrence and reports the count, and Ctrl+H opens the bar on that field. The
edit uses the completion popup's real input event, so the tab turns unsaved and
the highlight layer repaints. `replace.js` is a pure literal scan with the same
case-insensitive, non-overlapping rule as the find count. Not exercised in a
browser.
**Revision 3.34.1 (2026-11-13):** Clippy now lints Windows too. The `windows` job
in `desktop-ci.yml` runs `cargo clippy --locked --all-targets -- -D warnings`
after its tests, so a warning in a `#[cfg(windows)]` block fails the push instead
of surfacing at release; the step reuses the artifacts the job already built.
macOS is not linted yet, deliberately, because the step has never run there. The
issue is partly closed.
**Revision 3.34.0 (2026-11-13):** Recovery is visible. The Files pane lists a
folder's drafts whose file is not already open ("Recovered buffers", with path
and size); opening a row restores the text marked unsaved. Discard all clears the
folder through the new `DELETE /api/workspace/drafts[?prefix=]`, and deleting a
file or folder clears the drafts at or under it. A prefix is validated like a
draft path and matches whole components only. Not exercised in a browser.
**Revision 3.33.2 (2026-11-13):** A project switch keeps drafts. `applyProjectSwitch`
closes every tab silently, and 3.33.0 had that close delete each draft, so
switching projects discarded unsaved buffers without a prompt or a copy to recover
from; a silent close now keeps them under the old project's folder, while the
explicit close-all behind the folder-switch prompt still discards them. Not
exercised in a browser.
**Revision 3.33.1 (2026-11-13):** A rename no longer strands a draft. Renaming
an open dirty file moved the tab but the draft row was keyed by the old path, so
a shutdown before the next keystroke lost it; `moveDraft` now copies the row to
the new path and deletes the old one in both `remapOpenPaths` branches, and only
for a dirty buffer. Not exercised in a browser.
**Revision 3.33.0 (2026-11-13):** Crash recovery for unsaved buffers. A dirty
buffer is mirrored 1.2 s after the last keystroke into the app's own
`editor_drafts` table and cleared on save, revert or a discarded close; opening
the file offers the text back and marks the buffer unsaved, so the file is not
written until the reader saves. Drafts are scoped to the folder, capped at 1 MiB
each, 200 rows and 16 MiB per folder with the oldest pruned, and never write to
the workspace; the path check is lexical so a draft survives a deleted folder.
`GET`/`PUT`/`DELETE /api/workspace/draft` and `GET /api/workspace/drafts` sit
behind the editor's loopback guard, with no new contract id. Not verified in a
browser.
**Revision 3.32.0 (2026-11-13):** The Linux package metadata is now checked
against the build. The packaged AppStream component still advertised 1.26.6 at
3.31.0, which `appstreamcli validate-tree` accepts, so the .deb and AppImage
would have reported the wrong version; the component now says 3.32.0 with the old
entry kept as history. `scripts/ci/verify-linux-metadata.sh` checks, offline and
without appstreamcli, the component id against the file name, the summary and its
licence, the description, content rating, launchable, the newest release against
the build version and the desktop entry's Type/Name/Exec/Icon/Categories,
cross-checking `Exec` against the component's binary and the icon against its
committed PNG. `test-linux-metadata.sh` runs on every push and the deb/release
jobs validate before packaging. Phase 1 stays Blocked on the tagged run.

**Revision 3.31.0 (2026-11-13):** Phase 1's artifact gate now requires what
each release job promises. `verify-artifacts.sh` keeps its safety scan and checksum
verification and additionally fails, naming each one, when an expected file is
missing; every release job passes its own names, and `verify-release.sh` still
checks the whole published set. A new `test-artifacts.sh` runs on every push over
fixture directories (missing file, unsafe name, empty directory, tampered
checksum, the ten-file release set). Phase 1 stays Blocked on the tagged run.

**Revision 3.30.0 (2026-11-13):** Find references. `GET
`/api/index/references?name=&path=&limit=` reads the indexed files (the buffer's
own file first) and returns one row per line that mentions the name as a whole
word — character column, trimmed line, path, line, language — 200 by default and
500 at most with 32 MiB read per request, a `truncated` flag and a note. It is a
text search: calls and comments are found, scope is not known. Shift+F12 and
`Editor: Find references` jump to one mention or fill the chooser shared with
definitions, whose rows now show a mention's own line. `editor_references` is the
28th contract id with the `max_references` limit, false in the extension.

**Revision 3.29.0 (2026-11-13):** The editor's symbol outline. `GET
`/api/index/symbols?path=&limit=` lists one indexed file's declarations in source
order (name, kind, line, language; 200 by default, 2000 at most), 400 for a blank
path and an empty list with a note for a file the index has not seen. `Ctrl+Shift+O`
and `Editor: Go to symbol in file` jump to one symbol or open the chooser for
several, reusing the definitions list's keys and labels and the declaration
placement for the caret. Both choosers now share `fillChooser`. `editor_symbols`
is the 27th contract id with the `max_symbols` limit, false in the extension.

**Revision 3.28.0 (2026-11-13):** Go to definition. `GET
`/api/index/definition?name=&path=&limit=` answers with every declaration of the
exact name (kind, path, line, language; 20 by default, 50 at most), ranked exact
spelling first, then the buffer's own file, its folder, the shorter path and the
earlier line; an unknown name is an empty list with a note and an empty name is a
400. `F12` and `Editor: Go to definition` look up the identifier under the caret:
one declaration opens its file with the caret on the name, several open a chooser
at the caret, and a stale answer is dropped. `editor_definition` is the 26th
contract id with the `max_definitions` limit, false in the extension. What Phase 9
still lacks is a debugger and any language-server type information.

**Revision 3.27.0 (2026-11-13):** The editor keeps one highlight layer. `ide.js`
already wrapped the textarea in `.code-wrap` and painted `#codeLayer` with its own
regex engine since 1.26.1; 3.26.0 added a second overlay instead of using it, and
3.27.0 removes that duplicate and deletes the old engine. `highlightHtml` now calls
`highlightCode` from `app/static/highlight.js`, so the 22 language names, the
escaping rules and the 200,000-character cap come from one tested module, while
the workbench still owns the layer, its metrics, wrapping, find count and scroll
sync. `tok-prop`, `tok-tag`, `tok-attr` and `tok-punct` were added to `ide.css` for
both themes; `tok-com`/`tok-key` are the names the palette already used. Still
unverified: the layer in a browser (KNOWN-ISSUES 42).

**Revision 3.26.0 (2026-11-13):** Phase 9 gains syntax highlighting. *(Corrected in 3.27.0: the workbench already had a highlight layer, so 3.26.0 added a second one; 3.27.0 removed it and feeds the existing layer instead.)*
`app/static/highlight.js` tokenises the buffer behind a transparent textarea for
Rust, JS/TS, JSON, Python, Go, Java, Kotlin, C, C++, C#, Ruby, PHP, Swift, SQL,
Bash, YAML, TOML, CSS/SCSS, HTML/XML and Markdown, escaping every piece before it
wraps it; an unknown language, or a buffer past 200,000 characters, is shown
escaped and plain. Comments are read before strings, a Rust lifetime is not a
character literal, SQL folds case and Markdown fences close. No route, dependency
or contract id changed. A language server is still absent, so there is no type
information and no inline diagnostics; a debugger is the phase's remaining item.

**Revision 3.25.0 (2026-11-13):** Phase 9 gains symbol completion.
`GET /api/index/complete` reads the existing code index and ranks names near the
caret (prefix above contains, the buffer's own file above its folder, one entry
per name); the editor's Ctrl+Space listbox inserts one as an ordinary edit, so the
permission-checked save still decides what reaches disk. Names are matched in Rust
so a `%` is a character, not a wildcard, and the answer caps at 20 by default,
50 at most. The contract gained `editor_complete` (25 ids) and `max_completions`;
the VS Code embedded server reports it false. Syntax highlighting and a debugger
remain the phase's open work.

**Revision 3.24.0 (2026-11-13):** Phase 8's semantic index can embed through the
runtime on this machine. An embedding model named `local:<model>` goes to the
configured Ollama endpoint instead of the router, so search by meaning works with
no router key; `/api/embed` carries the whole batch and an older runtime's
one-prompt `/api/embeddings` is its fallback. The cache key is the model string as
typed, so the two backends never share vectors, and every failure (missing model,
short answer, unreachable endpoint, runtime refusal) is reported rather than
turned into a word search. The Index pane's field and note name the prefix; the
route, its answer shape and the contract are unchanged.

**Revision 3.23.0 (2026-11-12):** Phase 9 gains formatting. The editor's Format
button and `File: Format buffer` send the current buffer to
`POST /api/workspace/format {path, content}` and receive text only; the normal
save path still writes it, so permissions and file claims are unchanged. Fixed
formatters are selected by extension — rustfmt (stdin, edition 2021, project
config), gofmt, and Prettier from the folder's `node_modules/.bin` then PATH with
`--stdin-filepath`. Unknown extensions, missing tools, parse errors, timeouts and
empty output are 400s that name the reason and leave the buffer alone; input and
output are capped at 2 MiB. The path is workspace-relative and escapes/absolute
names are refused; a new unsaved buffer is allowed. The contract gained
`editor_format` (24 ids), false in the extension's built-in server. What remains
in Phase 9 is completion/diagnostics in the editor and a debugger.

**Revision 3.22.0 (2026-11-12):** Phase 14's local contract now drives the UI.
The contract gained `permissions`, `deep_search` and `watch` (23 desktop ids),
and the shared UI fetches `/api/capabilities` at boot. Only an explicit `false`
hides a control: Source control (`git`), Index (`code_index`), Memory (`memory`),
Watch (`watch`), Saved commands (`saved_commands`) and the Settings sections for
Deep search, OpenAI Platform, Permissions, MCP, Diagnostics and Extensions. An
older server, a missing table or a future contract keeps everything visible, and
a hidden remembered section falls back to Connection. The built-in server in the
VS Code extension reports the three new ids `false`, so those controls no longer
sit there failing against routes it does not serve. Android's Host API handshake
is unchanged.

**Revision 3.21.0 (2026-11-12):** Phase 9's watch mode ships. The Problems pane
can watch one preset and rerun it when the open folder changes: the server
start resolves the id from the same preset list (never a command line), asks the
`command` permission once for the session, holds the runner's only slot, and
polls the existing tree fingerprint every 700 ms. A run's own generated files
become the new baseline, so it cannot retrigger itself. Stop kills the command
(runner poll plus `taskkill /T` on Windows) and the watch clears after the
thread exits. `GET /api/workspace/watch` reports the last real run, and changing
folders stops the watcher. Phase 9 is left with the editor engine and a debugger.

**Revision 3.20.0 (2026-11-12):** Phase 14's local negotiation gap narrows. The
local API answers `GET /api/capabilities` with a versioned contract: 20 feature
ids it implements, plus limits read from the constants the routes enforce. The
VS Code extension's embedded server answers the same shape with its own subset
(and `false` for what it lacks), and Show server status reports the match, a
mismatch or an older backend instead of assuming. Android's existing v1 Host API
handshake is unchanged. What remains is making every screen read the contract
instead of gating by hand.

**Revision 3.19.0 (2026-11-12):** The formatting gap closes. `cargo fmt` over
`app/` reformatted 17 files without touching behaviour, `desktop-ci.yml` runs
`cargo fmt --check` before its build, and `rust-host-api-ci.yml` now checks the
whole manifest instead of four host modules. KNOWN-ISSUES #10 is fixed; clippy on
Windows/macOS (#11) is the remaining quality gate gap. No phase row moved.

**Revision 3.18.0 (2026-11-12):** Phase 9's last Git gap closes: a history
rewrite can be taken back from the pane. The reflog route lists the newest
`HEAD@{n}` entries and reports an unfinished merge or rebase; the undo route
accepts only a selector from that list, resolves the commit itself, resets behind
the `git` gate and refuses a dirty tracked tree or unfinished history work. The
reset is its own reflog entry, so the list also redoes it. What remains in Phase
9 is the editor engine, a debugger and watch mode.

**Revision 3.17.0 (2026-11-12):** Phase 9's arbitrary-command gap closes. A
command the user saved in Settings runs from the Problems pane beside the
detected build and test commands: `command_presets` is configuration (never
request text), the runner lists detected then saved presets, and the run route
still resolves an id against that list behind the same `command` gate. A saved
line goes through the platform shell because it is a command line. What remains
in Phase 9 is the editor engine, a debugger, watch mode and in-pane reflog undo.

**Revision 3.16.0 (2026-11-12):** Phase 13's canvas gap narrows to layout. The
test harness now records element listeners and attributes, so it can dispatch the
pointer events the real card handlers registered and read what they wrote: a drag
moves the card and redraws its edge before release, the release commits `x`/`y`
to the graph, a press under the 3px threshold writes no position and the click
still selects, and a drag past the canvas width clamps. A deliberate mutation of
the drag handler makes the new test fail, so it is not a tautology. What is still
not automated is a real browser's layout and pixels.

**Revision 3.15.0 (2026-11-12):** A workflow cancel stops what the graph started.
`agent::stop_run` is now the one cancel path — wake a live loop's flag, persist
`cancelled`, notify — shared by the per-run route and by `workflow::cancel`, which
cancels every child agent run the run's steps recorded and answers
`stoppedAgentRuns`. Phase 13's remaining work is the automated canvas test.

**Revision 3.14.0 (2026-11-12):** Phase 15's compatibility row closes: a
dependency that itself needs a newer build now blocks the folder that needs it
(`incompatibleDependencies` plus a named reason), and `inspect_for` proves a
listing re-judges a folder against the running build, so an upgrade frees it with
nothing stored to go stale. Provider adapters and UI extensions are what remain
in the phase.

**Revision 3.13.0 (2026-11-12):** Phase 4's last gap closes: a platform step also
declares the action vocabulary as native functions and translates a
`function_call` back into the same action object, so Phase 4 moves from Partial
to Working. The only thing left in it is a live call against the real platform,
which needs an owner key; every request shape is proven against a mock.

**Revision 3.12.0 (2026-11-12):** Phase 4 gains structured output on the platform
path: an agent step over an `openai:` pick sends `text.format` with a
`json_schema`, retries once without it when a model refuses, and remembers the
refusal — the same pattern as the reasoning parameter. That also corrects a
stale claim repeated here and in the roadmap: agent steps have routed to the
platform since 1.29.0, so only *native tool calling* remains open in Phase 4.

**Revision 3.11.0 (2026-11-12):** Phase 1's missing artifact coverage closes: the
VSIX and the APK/AAB each get an install smoke in their own release job
(`scripts/ci/smoke-vsix.sh`, `scripts/ci/smoke-apk.sh`), regression-tested in
`test-release.sh` (now 13 checks). What still keeps Phase 1 Partial is a green
tagged run with the 11-asset set, not a missing check.

**Revision 3.9.0 (2026-11-12):** Phase 15 gained dependency-cycle detection —
discovery reports `dependencyCycle` and blocks a run whose chain closes on
itself — so that named gap closes. The same commit repairs two type errors that
3.8.0 shipped without a build (E0618 in `router_registry`'s test, E0597 in
`agent::reorder`), accepts a plain-HTTP remote router on `[::1]`, and makes
`audit-releases.sh` strip a CR from a listed tag. `cargo test --locked` is 557
passed, 1 ignored and clippy is clean; the phase states below are otherwise
unchanged, and the old percentage arithmetic stays retired.

**Revision 3.8.0 (2026-11-11):** documentation correction. The ordered backlog
now lives in `MASTER-ROADMAP.md` under "Remaining work at 3.8.0"; this report
replaces the stale 2.1.0 percentage table with a qualitative phase state and
calls out the work that has shipped since it was written (MSI, macOS `.app`,
portable uninstall, free-position canvas and extension tools). No code or test
count changed.

**Revision 3.7.0 (2026-11-11):** Phase 1's deb-only failure has a concrete
cause: the headless tree links `keyring`'s Secret Service backend, whose
`libdbus-sys` build script needs `libdbus-1-dev`, and the deb job installed only
`mold`; the Tauri jobs get the headers through GTK. The job now installs
`libdbus-1-dev` and `pkg-config`, and `test-release.sh` guards that line (5/5).
No figures changed; a CI rerun is still required. The old zero-asset tags and
the rest of Phase 1 remain open.

**Revision 3.6.0 (2026-11-11):** Phase 1 gained a scheduled backstop:
`scripts/ci/audit-releases.sh` audits every published release at or above 3.4.0
with the 11-asset gate, and `release-audit.yml` runs it daily plus on manual
dispatch. The older zero-asset tags stay below the floor and are not re-reported.
`test-release.sh` covers a passing audit and a missing-VSIX failure (4/4). No
figures changed; this is release plumbing. The deb headless test exit 101 still
blocks a complete green release.

**Revision 3.5.0 (2026-11-11):** Phase 1's trigger gap closes for future releases:
`release: types: [published]` now runs Desktop Release when a release is created
through the API/UI without a tag-push event, with the existing tag-scoped
preflight, build and asset-gate path. The workflow's own `GITHUB_TOKEN` release
creation does not loop. No live release event was published here; the deb
headless tests still exit 101, so a complete green release remains blocked. No
figures changed because this is release plumbing.

**Revision 3.4.0 (2026-11-11):** Phase 1 gained a tag-targeted recovery path and
a post-build release gate. `workflow_dispatch` now accepts `tag`; `preflight`
checks out that tag, refuses a mismatch with `app/Cargo.toml`, and scopes
attachment and concurrency to it. `scripts/ci/verify-release.sh` requires all 11
promised assets with non-zero size, and the final `verify-release` job fails a
tag run when one is missing. `smoke-package.sh` now recognises the
AppImage/deb UI under `share/sarthi-studio/static` (and the legacy
`share/free-ai-suite` path), matching `static_dir_candidates`; that layout false
negative is what failed the AppImage smoke in run `42908396329`. The same run's
deb job still fails `cargo test --no-default-features` with exit 101 on `3.0.2`,
so a complete release remains blocked; no assertion was weakened. No figures
changed because this closes release plumbing, not a feature row.

**Revision 3.3.0 (2026-11-11):** Phase 12's last named gap closes:
`POST /api/runs/:id/queue` moves a queued run one place (`move: "up"|"down"`),
rewriting the queued priorities into one explicit descending order inside a
transaction so the card's order is the admission order. Only queued runs move,
the ends refuse with 409, and the card offers Move up / Move down. Phase 12
stays Working and no row was added, so the figures are unchanged at 92%.

**Revision 3.2.0 (2026-11-11):** Phase 9's named rebase gap closes:
`POST /api/workspace/git/rebase/skip` drops the commit being replayed, gated on
`git` like Continue, refusing when no rebase is in progress and returning the
dropped subject read from `REBASE_HEAD`. Phase 9's row stays Working and no row
was added, so the figures are unchanged at 92%.

**Revision 3.1.0 (2026-11-11):** Phase 2 closes: a router can be addressed at a
remote endpoint. `POST /api/routers/:id/remote` stores a validated base URL
(https anywhere, http only on loopback; no credentials, query, fragment or
path) behind the `settings` rule, an empty string clears it, and a value edited
into the file by hand is re-validated on read. The list probes every saved
endpoint concurrently with the router's own health path, the connection tester
uses it, and a remote router is `running` only when it answers and is never
owned. The middleware routes for start/stop/restart/install/update/uninstall
refuse `409` naming the endpoint and the startup sweep skips it. Phase 2 is 13
rows, all Working — the panel's update/uninstall row loses Partial because the
panel already calls both routes end to end, and remote routers lose Missing.
The sum moves 1,659 → 1,661 of 18, so the overall mean stays 92%. Not verified:
a real remote router across a network, the Android path, and the new editor
rendered in a browser.

**Revision 2.50.0 (2026-11-10):** a P0 in the IDE shell, not in the thread
code: the conversation lived in the AI dock's column, which the legacy "right panel
closed" default collapsed to zero width, so a fresh install showed a blank workspace
with an 8 px composer. The panel default is now one rule (docked shell opens the
dock), panels are applied after the dock exists, opening a thread reveals it, the
open thread survives a reload, and the status-bar connection label follows its dot.
`/api/usage`, which every fresh home answered with 500, is fixed. Figures are
unchanged at 92% (1,659/18); this repaired existing rows rather than adding any.

**Revision 2.49.0 (2026-11-10):** the canvas was driven in a real browser
for the first time, closing Phase 13's last verification gap. The check found one
real defect (an unguarded pointer capture that aborted the drag) and confirmed the
gesture, the clamping, the live edges and the round trip through save and reload.
Phase 13 rows stay Working and no row was added, so the figures are unchanged at
92% (1,659/18).

**Revision 2.48.0 (2026-11-10):** scheduled windows are now arbitrated
between processes — the claim lives in SQLite and only moves forward, so a desktop
app and the headless server over one home start one run per due time. The Phase 13
row stays Working and no row was added, so the phase and overall figures are
unchanged at 92% (1,659/18).

**Revision 2.47.0 (2026-11-10):** Phase 13's visual editor gains free
positioning — a node may carry `x`/`y`, honoured by the layout and persisted through
save-time validation — so the canvas is no longer limited to dependency columns.
The Phase 13 row moves no category (it was already Working), so the phase and
overall figures are unchanged at 92% (1,659/18).

**Revision 2.46.0 (2026-11-10):** a CI fix, not a feature. Worktree removal
compared paths as strings, so on macOS a worktree reached through `/var/folders`
never matched the `/private/var/folders` path git reports and was refused as
another repository's. The comparison now resolves both sides through the
filesystem, and git is handed its own spelling of the validated path. Phase counts
are unchanged; the overall mean stays 92% (1,659/18).

**Revision 2.45.0 (2026-11-10):** the editor now saves through the run lease:
`filelock::claimed_by`/`command_claim` let a save, create, rename or delete ask
which live run holds the path or the folder, and refuse with `409` naming it
instead of racing it. The agent's write already refused the other direction, so a
conflict now fails loudly on both sides. Phase counts are unchanged — this closes
an open edge in row 20, not a feature row. Overall mean stays 92% (1,659/18).

**Revision 2.44.0 (2026-11-10):** a portable (tarball/zip) install can now be
uninstalled from Settings → About: `POST /api/uninstall` removes the program
folder, the settings folder and the data folder after an `approve` plus a
confirmation that is the data folder's path, keeps either tree on request, and
refuses a source checkout outright. On Windows the running binary is renamed
aside and the folder is reported as pending, because a running program cannot be
deleted. Phase 18 stays 10 rows — 9 Working + 1 Partial = 95% — because signing
is still missing, and the overall mean stays 92% (1,659/18).

**Revision 2.43.0 (2026-11-10):** Phase 18's installer row is now Working for both
Windows and macOS: a per-user MSI (`app/packaging/windows/installer.wxs` built by
`scripts/ci/build-windows-msi.sh`, payload proven by an administrative install)
sits beside the macOS `.app`, and the release job smokes both packaged results.
What keeps the row Partial is signing: nothing is code-signed or notarized, and a
portable (tarball/zip) install still has no uninstall of its own. Phase 18 is 10
rows — 9 Working + 1 Partial = 95% — and the overall mean stays 92% (1,659/18).

**Revision 2.42.0 (2026-11-10):** Phase 18 gained the macOS application bundle:
`scripts/ci/bundle-macos-app.sh` builds `Sarthi Studio.app` from the staged tree
and the binary resolves `<exe>/../Resources/static`, so the bundle is a real
installable app rather than a renamed binary; the release job smokes its zip and
attaches it. Phase 18's install row moves from Missing to Partial (macOS bundled,
Windows MSI/EXE and uninstall still absent), so the phase is 10 rows — 9 Working +
1 Partial = 9.5/10 = 95% — and the overall mean stays 92% (1,659/18, rounded; the
sum was 1,654 before this row moved).

**Revision 2.41.0 (2026-11-10):** Phase 18 gained the way back: `POST
/api/update/rollback` restores the backup a swap kept (the copy is hashed first) and
keeps the build it displaces, refusing without a record, a missing backup, another
binary's record or a backup name this app did not write. Phase 18 stays 10 rows at
90% — the install row is still Missing because there is no MSI/EXE/.app or
uninstaller — and the overall mean stays 92%.

**Revision 2.40.0 (2026-11-10):** the backups a swap leaves behind are cleared at
startup: only `<name>.old-<version>` names whose version is older than the running
build, never the file being executed and never a newer one. Phase 18 stays 10 rows
at 90% and the overall mean stays 92%.

**Revision 2.39.0 (2026-11-10):** Phase 18 gained the swap: `POST
/api/update/swap` copies the prepared binary beside the target, hash-checks the
copy, renames the old binary to `<name>.old-<version>` and restores it if the new
one cannot take its place. The running process keeps its image until a restart,
which the reply says. Phase 18 is 10 rows — 9 Working + 1 Missing = 9/10 = 90% — and
the overall mean stays 92%.

**Revision 2.38.0 (2026-11-10):** Phase 18 gained the prepare step: the extractor
reads gzip tarballs (this app's own Linux/macOS artifacts), `POST
/api/update/prepare` re-verifies the staged sha256, unpacks into
`updates/prepared/<version>/` and records the binary it found, refusing a tree
without one. Phase 18 is 9 rows — 8 Working + 1 Missing = 8/9 = 89% — and the
overall mean stays 92%.

**Revision 2.37.0 (2026-11-10):** Phase 18 gained the verified download: `POST
/api/update/stage` fetches the named asset through the verified-download path
(hash from the release's `digest` or `SHA256SUMS.txt`, refused when neither
exists), records it in `updates/staged.json`, and is gated on `update`. Nothing
applies it. Phase 18 is 8 rows — 7 Working + 1 Missing = 7/8 = 88% — and the
overall mean stays 92% (the table's own percentages sum to 1,654/18).

**Revision 2.36.0 (2026-11-10):** Phase 18 gained an update check (2.36.0):
`GET /api/update/check` reads the newest published release, compares versions
numerically and names this platform's asset, and Settings → Privacy & about
renders it. Nothing downloads or installs, so the install row stays Missing.
Phase 18 is now 7 rows — 6 Working + 1 Missing = 6/7 = 86% — and the overall mean
is 92% (the table's own percentages sum to 1,654/18).

**Revision 2.35.0 (2026-11-10):** Phase 18's last row moved from Missing to
Partial: the database is versioned (`PRAGMA user_version`), copied with
`VACUUM INTO` and verified before an existing one is migrated, restored if a
migration fails, and a database from a newer build is refused untouched
(2.35.0). Auto-update and uninstall are still absent. Phase 18 is 6 rows —
5 Working + 1 Partial = 5.5/6 = 92% — so the overall mean is 92% (the table's own
percentages sum to 1,658/18; re-derived here because the previous note's 1,630
did not match its own table). Superseded by the 2.36.0 note above, which adds a
row and re-derives the totals.

**Revision 2.34.0 (2026-11-10):** Phase 16's failure-injection row gained two more
cases (2.34.0): a corrupt database (the process must exit with a named error and
recover from a restored copy) and a read-only data directory (the readiness check
must go false and recover). A true ENOSPC case is still not run and is named as
such. No row changed status, so Phase 16 stays 100% and the overall mean stays 91%.

**Revision 2.33.0 (2026-11-10):** Phase 16's frontend row gained a render smoke
(2.33.0): the workflow canvas and the `ide.js` shell are now executed in the
harness and their elements inspected, so the "no render smoke, no `ide.js`
harness" caveat is gone and the frontend suite is 21 cases. No row changed
status, so Phase 16 stays 100% and the overall mean stays 91%.

**Revision 2.32.0 (2026-11-10):** Phase 16's Windows row moved from Missing to
Working — `desktop-ci.yml` now builds and tests the desktop on `windows-latest`
(2.32.0) — so the phase is 9 of 9 rows = 100%, and the overall mean is 91%
(1,630/18). Its named caveats are the `ide.js` render smoke and the remaining
injections, not a missing layer.

**Revision 2.31.0 (2026-11-10):** Phase 16's install-smoke row moved from Missing
to Working: `scripts/ci/smoke-package.sh` unpacks each release artifact (tarball,
zip, `.deb`, AppImage), boots the packaged binary and checks its reported version,
and the release workflow runs it on all five artifacts (2.31.0). The phase is 8 of
its 9 rows = 89%, and the overall mean is 90% (1,619/18).

**Revision 2.30.0 (2026-11-10):** Phase 16's failure-injection row moved from
Missing to Working — the process-level harness now kills the provider and kills
the process mid-run and asserts what the restart says (2.30.0) — so the phase is
7 of its 9 rows = 78%, and the overall mean stays 89% (1,608/18).

**Revision 2.29.0 (2026-11-10):** Phase 12's named gap — a live step view while a
model call is in flight — closed in 2.29.0: the loop writes a `thinking`
placeholder with `status: "running"` before each call and the step that resolves
it replaces that row. The phase moves 90% → 95% (reordering beyond a numeric
priority is what is left), which moves the overall mean 88% → 89% (1,597/18).

**Revision 2.28.0 (2026-11-10):** Phase 15's custom-tool row now enforces the
declared `inputSchema` before a module loads (2.28.0), so its largest open gaps are
provider adapters and UI extensions. The phase stays 82% (9 of 11 rows) and the
overall mean stays 88% — no row changed status, so no percentage moved.

**Revision 2.27.0 (2026-11-10):** Phase 15's custom-tool row moved from Partial to
Working — the agent's tool loop now offers installed tools (2.27.0) — so the phase
is 9 of its 11 rows = 82%. Re-deriving it exposed two arithmetic slips, corrected
here: the 2.26.0 note counted 12 rows and 71% where the table has 11 rows and the
figure then was 77% (8 working + 1 partial = 8.5/11), and the stated overall mean
of 82% did not match the mean of this table's own phase percentages, which is 88%
(1,592/18). The method is unchanged; only the arithmetic was re-run.

**Revision 2.26.0 (2026-11-10):** Phase 15's custom-tool row moved from Missing to
Partial: a manifest can declare tools, the sandbox has a tool ABI
(`env.arguments`/`env.result`), and `POST /api/extensions/:id/tools/:tool` runs
one behind the new `extension_tool` permission. It is not in the agent's tool
loop yet and `inputSchema` is not enforced, so the phase is re-derived from its
now 12 rows (8 working + 1 partial = 8.5/12 = 71%); the overall mean stays 82%.

**Revision 2.25.0 (2026-11-10):** Phase 15 gained a compatibility row —
`requiresStudio`, `apiVersion` and `dependencies` are declared, reported and
enforced, so a folder written for a newer build is refused by name instead of
half-read. Phase 15's percentage is re-derived from its current rows (8 of 11
working = 73%), which also moves the overall mean to 82%; the row-level verdicts
below otherwise stand.

**Revision 2.7.0 (2026-11-09):** Phase 7's last row closed — MCP calls are
validated against a bounded JSON Schema subset. The row-level verdicts below
otherwise stand as written at 2.1.0.

**Revision 2.1.0 (2026-11-09):** rows re-verified against the tree at `2.1.0`.
Closed since the 1.28.0 pass: Phase 2 configuration editor and dependency
probing (1.43.0), Phase 3 circuit breaker and provider health (1.50.0), Phase 10
credential store (1.42.0) and global emergency stop (1.41.0), Phase 17 timeline
and bundle (1.72.0). This file is the per-requirement verdict; `PROGRESS.md` is
the shipped-version checklist, `MASTER-ROADMAP.md` the ordering and
`KNOWN-ISSUES.md` the defect list. When they disagree, this file and
`PROGRESS.md` are corrected together.

## Verdict in one paragraph

The core is largely built. Router management, the agent engine, MCP, permissions,
token/cost, observability, the workflow engine and the testing layers are
functionally complete; their remaining notes are caveats, not missing
subsystems. The real remaining work is concentrated in Phase 1 release
reliability, Phase 9 editor/debugger depth, Phase 14
client parity, Phase 15 provider/UI extensions, and Phase 18 signing and
packaging. Phase 3/6/8/12/13 keep only the smaller gaps below and in
`MASTER-ROADMAP.md`.

## Current state by phase

The old percentage table is retired because several rows predated shipped work
(the MSI, macOS `.app`, portable uninstall, free-position canvas and extension
tools). This is a qualitative status, not a release gate; the per-phase tables
below carry the evidence.

| Phase | Area | State | Largest open gap |
|---|---|---|---|
| 1 | CI/CD and release | Blocked | green tagged run with 11 assets; deb test confirmation. The artifact set gate (3.31.0) now requires each job's promised files and runs on every push. The AppStream/desktop metadata is checked against the build version on every push and before packaging since 3.32.0 |
| 2 | Router manager | Working | remote routers are unauthenticated base URLs; no live network test |
| 3 | Model and provider | Working | native OpenAI picks carry no price; context only when supplied |
| 4 | Thinking + OpenAI Platform | Working | no live platform call yet; request shapes are proven against a mock |
| 5 | Agent execution | Working | named caveats only |
| 6 | Skills | Working | Git import in 3.36.0; per-skill permissions only matter if skills execute |
| 7 | MCP | Working | — |
| 8 | Memory and RAG | Working | semantic search embeds through the router or a local runtime (`local:`, 3.24.0) |
| 9 | Developer environment | Partial | a debugger; highlighting through the workbench layer (3.27.0), completion in 3.25.0, definition in 3.28.0, the file outline in 3.29.0, references in 3.30.0, formatting in 3.23.0, unsaved-buffer recovery in 3.33.0 and its Files-pane list in 3.34.0, search and replace in 3.35.0 |
| 10 | Safety and permissions | Working | — |
| 11 | Token, context, cost | Working | cost only where the provider reports usage |
| 12 | Live monitoring | Working | — |
| 13 | Workflow builder | Working | browser layout test only; rendering and the drag gesture are harness-tested (3.16.0), a workflow cancel stops the child agent runs it started (3.15.0) |
| 14 | Cross-platform | Partial | Android/VS Code subsets; contract-driven UI gating shipped in 3.22.0 |
| 15 | Extension ecosystem | Partial | provider adapters, UI extensions |
| 16 | Testing and quality | Working | browser layout, privileged ENOSPC |
| 17 | Observability | Working | — |
| 18 | Installation and updates | Partial | signing/notarization, macOS `.dmg`, per-machine MSI |

The old `2.1.0` row counts are historical; they are not a progress target.

## Phase 1 — CI/CD and error diagnosis

| Item | Status | Evidence / gap |
|---|---|---|
| One version parser for all OSes | Working | `scripts/ci/version.sh` strips `[:space:][:cntrl:]`, validates semver |
| Version regression suite | Working | `scripts/ci/test-version.sh`, wired into `desktop-ci.yml` |
| Artifact-name + checksum gate | Working | `scripts/ci/verify-artifacts.sh` runs before every upload |
| Release concurrency | Working | `concurrency: desktop-release-${{ github.ref }}` |
| AppStream metadata | Working | `appstreamcli validate-tree` clean; AppImage attaches |
| macOS job | Working | `macos-15-intel`, both Darwin targets, universal tarball |
| All six release jobs green | Partial | last fully green run `41884439627` (`v1.26.6`, 9 assets); no 11-asset run has passed |
| Deb headless tests | Partial | run `42908396329` (`3.0.2`) exited 101; 3.7.0 adds `libdbus-1-dev` and `pkg-config`, awaiting CI |
| Install smokes | Working (3.11.0) | deb/AppImage/tarball/zip/macOS were wired in 2.31.0; the VSIX and the APK/AAB are smoked in their own jobs since 3.11.0 (`smoke-vsix.sh`, `smoke-apk.sh`), each proven against a good and a broken fixture in `test-release.sh` |
| Dead tags `v1.26.0/1/2` | Open | documented as failed; no decision to delete |

Issue A (agent SQL syntax) is **not reproducible**: `agent.rs` compiles and its
query paths are covered by tests — left unchanged rather than rewritten blindly.
Issue F (macOS cancellation) was diagnosed as runner acquisition, not code.

## Phase 2 — Router manager

| Item | Status | Notes |
|---|---|---|
| Central registry with §5.1 fields | Working | `router_registry.rs`; only routers with a real lifecycle are listed |
| OS/arch gating, Android never claims a local install | Working | `capability()` + tests |
| Detection: installed/version/runtime/healthy/ownership | Working | `/api/routers`, probes run concurrently (1.26.14) |
| `healthy` means the probe answered | Working | unit-tested both ways |
| Approval-gated install with plan digest | Working | 1.26.18 |
| Pinned images, verified downloads | Working | 1.26.19/1.26.20/1.26.21 |
| Update + uninstall (data kept unless `purge`) | Working | 1.26.23 |
| Auto-start sweep, external routers never stopped | Working | 1.26.17, rule tested both ways |
| UI: list, health, start/stop, restart, tester, logs | Working | 1.26.13–1.26.16 |
| Configuration editor | Working (1.43.0) | `GET /api/routers/:id/config` lists whitelisted files in the install's config dir; `GET/POST /api/routers/:id/config/file?name=` reads and writes one of them through the same validator, 64 KiB cap, gated on `fs_write`; a router with no editable file explains why instead of showing an empty box |
| Dependency/package-manager probing | Working (1.43.0) | `/api/routers` reports the measured host: OS + version, arch, apt/dnf/pacman/zypper/apk/brew/winget/choco/scoop/port with versions, Docker and compose, read from PATH and the tools' own output |
| Update and uninstall from the panel | Working (1.26.23, re-read 3.1.0) | Update reads the status, prints the plan, asks, then posts `approve` + the plan's digest; Uninstall asks and posts `approve` with `purge: false`, and both reload the measured list. Re-read end to end in the code at 3.1.0; not click-tested in a browser this release |
| Remote service routers | Working (3.1.0) | `POST /api/routers/:id/remote` stores one validated base URL per router in `router-prefs.json` (empty clears; `settings` → `config` rule); https anywhere, http only on loopback, credentials/query/fragment/path refused, hand-edited values re-validated on read. `GET /api/routers` probes each saved endpoint concurrently on the router's own health path and the tester reports its status/latency; a remote router is `installed`/`running` only when it answers and is never owned. Start/stop/restart/install/update/uninstall answer `409` naming the endpoint, auto-start skips it, and the panel hides the local-only controls. The endpoint is a base URL only — no per-router credentials — and the probe is an unauthenticated GET |

## Phase 3 — Model and provider manager

| Item | Status | Notes |
|---|---|---|
| Provider presets | Working | `app.js` / `index.html` |
| Model discovery with cache | Working | `/models`, 1-minute cache keyed by base + key |
| Local Ollama models | Working | `local.rs` (status, tags, hardware, pull with progress) |
| Context window where a provider reports it | Partial | `main.rs:1181` reads `context_window`; `app.js:2704` displays it — nothing is inferred when absent |
| Capability table (modalities, tools, reasoning, structured output) | Working (1.28.0) | `capabilities.rs` from `/models` rows and Ollama `/api/show`; unknown stays `null` |
| Price table | Partial (1.28.0) | carried when the provider publishes `pricing.prompt`/`completion`; never inferred |
| Model scope (task > agent > project > global) | Working (1.46.0) | `model_scope::resolve` with a reported origin; `projects.model`, `agent_model`, and a per-run `model`; the composer pick stays the task scope |
| Fallback / retry | Working (1.50.0) | ordered, capped, deduplicated chain that retries only an unavailable provider and never replays a half-streamed answer; every attempt lands in meta. A circuit breaker (`breaker.rs`) opens after three consecutive unavailable answers, refuses the model for 45 s, then allows one probe; a skipped candidate is recorded as `circuit open: skipped` and the whole-chain refusal names each model's remaining seconds |
| Provider health monitoring | Working (1.50.0) | `GET /api/health/providers` merges the breaker's live state with 24 h of `request_log` (requests, successes, average latency, the provider's own last error) and states the opening rule; shown as a Provider health card on the usage page. `/api/health` remains the cheap liveness answer |

## Phase 4 — Thinking and OpenAI Platform

Backend in 1.29.0, composer and settings in 1.30.0.

| Item | Status |
|---|---|
| Native OpenAI Platform provider (Responses API) | Working (1.29.0) — `openai.rs`, own key, `openai:` model ids, buffered + streaming + agent calls |
| Thinking control (off/low/medium/high/model default) | Working (1.30.0) — composer Think control, levels from `/api/thinking/levels` |
| `@Thinking` shortcut + literal escape | Working (1.29.0/1.30.0) — stripped server-side before storage, hinted in the composer, `\@Thinking` stays literal |
| Scope precedence (message > agent > project > global) | Working (1.29.0) — `projects.thinking` column migrated in place, agent scope from config |
| Reasoning summaries / progress surfacing | Working (1.30.0) — collapsible card fed by the provider's summary frames |
| Platform key in Settings (write-only) + key test | Working (1.30.0) — `/api/openai/test`, no tokens spent |
| Structured output for agent steps | Working (3.12.0) — `text.format` `json_schema` on a platform pick, one retry without it and the refusal remembered; router and local calls unchanged |
| Native tool calling (`tools` / `function_call`) | Working (3.13.0) — seven core functions plus `mcp_call`/`extension_call` when the prompt advertised them, `tool_choice: "auto"`, each call translated into the same action object the text protocol uses |
| Tests | Working — 21 new cases, 204 total; live smoke on :8799 for the new routes |
| Tool calling / structured output through the platform path | Missing — the request shape allows it, no agent tool loop uses `openai:` yet |

## Phase 5 — Agent execution engine

| Item | Status | Notes |
|---|---|---|
| Real execution loop with tools | Working | `agent.rs`: think/list/read/write/command/done/fail |
| Approval pauses, checkpoints, diffs, restore | Working (1.70.0) | `awaiting_approval`, per-step checkpoint (200 kB cap), per-step restore and run-level restore of every file a finished run wrote; `fs_write` is checked per path before a bulk restore writes anything, and a checkpoint digest skips a path edited after the run unless the user forces it (1.71.0) |
| Cancellation, crash recovery (`interrupted`) | Working | `ACTIVE` map; startup sweep |
| Pause / resume between steps | Working (1.52.0) | `POST /api/runs/:id/pause` parks a live run at the next step boundary (never mid-action), writes a `pause` step, sets `paused` and releases its control entry; `resume` continues from the same step log. A pause is durable across a restart and is only refused when there is nothing to stop (a run awaiting approval) |
| Resume from the step log | Working | `POST /api/runs/:id/resume` + 2 tests |
| Team (executor → reviewer → repair) | Working | `review` step with verdict/findings |
| Crew pipeline + per-stage budgets/policies | Working | `crew` roster 1–6, `stage_max_steps`, fail/skip |
| Hybrid cloud-plan / local-execute | Working | plan step, bounded escalation |
| Multi-agent parity in VS Code | Working | `agent.ts` port |
| Task queue, priority, dependencies | Working (1.53.0) | every run is created `queued` and admitted by one dispatcher pass ordered by `priority` (-10…10) then age; `dependsOn` is validated at creation (unknown/self/loop/empty/>8 all 400) and a dependency is only satisfied by `succeeded`, while a dependent of a failed/cancelled/interrupted run is failed with the dependency's name. A queued run survives a restart and is admitted at boot; pause names it as queued, cancel and the emergency stop include it |
| Retry from scratch (`POST /api/runs/:id/retry`) | Working (1.55.0) | starts a clean run from a finished run's settings with `retry_of` linking back; the step log, tokens, duration, error and dependencies are not carried over, and a `retry` step names the original. Refused for a run that has not finished; a paused run is told to resume instead |
| Concurrency limit (`max_concurrent_runs`) | Working (2.1.0) | clamped 1–4, default 2, honoured above 1 only when the workspace is itself a git repository root (every run gets its own worktree); a non-git folder stays at one. A run waiting for approval still holds its slot, a paused or queued run does not |
| Conflict detection before a write | Working (1.54.0) | `app/src/filelock.rs`: a `write` is refused as a non-fatal `write-conflict` step when another live run has claimed the path or the file on disk is not what this run last read; the run re-reads and continues. Claims are dropped for any run no longer `running`/`awaiting_approval` and released by `finish` |
| Parallel execution + worktree isolation / file locks | Working (2.1.0) | each writing run works in its own worktree, a command claims its checkout while it runs, and the lease ledger lives in the database so more than one process sees it. An editor save or another program still writes behind the lease, caught on the run's next write |
| Verification step before `succeeded` | Working (1.56.0) | a run that wrote, ran a command or called a tool goes to the verifier, which must answer `APPROVE`; ISSUES hands the findings back for one repair round and a second ISSUES ends the run `failed` with the findings kept. Read-only runs and `team` mode are not verified twice. No human sign-off step: the verifier is a model on the same router |
| Ruflo | Not integrated (spec says optional) | Sarthi Core keeps ownership |
| Live-run checkout hand-back | Working (2.0.0) | a finished run's `run/<id>` branch is committed, the shared checkout is fast-forwarded and the worktree removed; nothing is applied automatically and an unapplied worktree stays on disk |

## Phase 6 — Skills

| Item | Status | Notes |
|---|---|---|
| Discovery across 5 roots | Working | `skills.rs` |
| Enable/disable persisted | Working | `POST /api/skills/:id/state` |
| Instruction injection with token cap | Working | rides the next message |
| Install / update / remove / version tracking | Working (1.49.0) | install writes `<user skills>/<slug>/SKILL.md` after validating it with the list parser (temp file + rename, refuses overwrite unless `replace`); update rewrites in place and keeps the id; remove moves the folder to `<root>/.removed/` instead of deleting; `skills.json` records version, install/update times and a content fingerprint, so “edited since install” is reported; only the user root is writable. Git import (3.36.0): `POST /api/skills/import` clones an https URL or a local clone, requires a root `SKILL.md`, caps the copy at 200 files/2 MiB, refuses symlinks, installs as the user root's `<slug>` and rewrites the manifest to that id; the clone is removed either way and the route is git-gated; Refresh from Git (3.37.0) re-clones the recorded origin, keeping the replaced copy under `.removed/`; a local folder without `.git` is copied through the same validation (3.39.0) and has no origin to refresh |
| Skill permissions / usage history | Partial (1.49.0) | usage history is real (`skill_usage`: id, time, conversation, tokens; `uses`/`last_used_at` per skill and `/api/skills/:id/usage`); writing routes are gated on `fs_write`. There are no per-skill permissions because a skill is prompt text and is never executed |
| Compatibility checks | Working (3.41.0) | optional `requires: X.Y.Z` in front matter (a non-version is a load error); an older build reports `compatible: false` with the requirement, skips it in auto-selection/index, hides it in the composer and workflow pickers, answers a direct pick 422 naming both versions, and refuses an import before copying; install/update rewrites keep the line || Automatic skill selection | Working (1.57.0) | `skills::select_for`: id/name words score 4, description words 2, noise words and numbers dropped; needs 2 points, at most two skills, id tie-break, 1,200-token budget, disabled and broken skills skipped. A manual pick always wins, and `auto_select_skills` (default on) turns it off. The reply meta names what was attached (`auto_skills`) |

## Phase 7 — MCP

| Item | Status | Notes |
|---|---|---|
| Server registry | Working (1.37.0) | `mcp.json` beside the config; `transport` (stdio/http/sse), `url` for remote servers, `env` for stdio and write-only `headers` for remote — values stored, only names returned; an entry without `transport` reads as stdio |
| Explicit disconnect | Working (3.45.0) | `POST /api/mcp/:name/disconnect` drops the warm session (stdio child killed, SSE reader aborted, HTTP session no longer reused) and keeps the settings; `{disconnected:false}` when none was warm, 404 for an unknown name, local-origin guarded. The MCP panel's Disconnect action says which happened || Connection lifecycle | Working (1.39.0) | a live session per server is reused across calls (handshake skipped, tool list re-read), warm for five idle minutes, evicted on save/remove and on a failed request, with a fresh-handshake fallback; a dropped stdio child is killed and an SSE reader aborted |
| Tool discovery | Working (1.33.0) | `tools/list`, schemas kept |
| Schema validation | Working (2.7.0) | Bounded JSON Schema subset: type lists, enum/const, nested properties and items, `additionalProperties`, bounds, combinators, boolean schemas and local `$ref`s, depth-limited; `pattern`/`format`/remote refs left to the server |
| Permission binding | Working (1.33.0) | `mcp_tool` on `server.tool`; ask/deny/allow, audited |
| Health, logs, reconnect | Working (1.36.0) | health recorded from a real handshake (`serverInfo.version`, protocol, tool count, latency, time), `unknown` before any contact; stderr and client events in a 200-line in-memory ring; one handshake retry after 250 ms, never a retried `tools/call`; `/api/mcp/health`, `/api/mcp/health/check`, `/api/mcp/logs?name=`; an idle supervisor pings each pooled session every 60 s with a 5 s budget and evicts only silence or a dead transport (1.45.0) |
| SSE / HTTP transports | Working (1.37.0) | streamable HTTP (one POST per message, JSON or SSE answer, `Mcp-Session-Id` carried forward) and legacy HTTP+SSE (GET stream, `endpoint` event, id-matched answers); both share handshake, catalogue, validation, gating and health/log state with stdio |
| Agent use of MCP tools | Working (1.35.0) | `Action::Mcp` with a per-run `allowMcp` flag; catalogue discovered once into `agent_runs.mcp_tools` and logged as `mcp-tools`; approval pause names the tool and arguments; `mcp_tool` rules on `server.tool` veto; execution goes through the same `mcp::invoke` as the route |
| Per-step tool rediscovery | Working (1.38.0) | a failed `mcp` step re-reads the catalogue before the repair step; the snapshot's `refreshes` counter bounds it to three per run and survives a resume |
| MCP manager UI | Working (1.34.0) | Settings → MCP: register/edit/test/remove, tool cards, argument template, approve-then-call, raw result |

## Phase 8 — Project knowledge and memory

| Item | Status | Notes |
|---|---|---|
| Conversation history persistence | Working | `conversations`/`messages`, Android mirror |
| Workspace text search | Working | `/api/workspace/search` |
| Embeddings *probe* | Superseded (3.24.0) | `media.rs` still proxies one `/api/embeddings` call for its own use; the semantic search below stores and reuses real vectors, through the router or a local `local:` model |
| Semantic search over the code index | Working (1.63.0, local backend 3.24.0) | `app/src/embeddings.rs`: indexed symbols are embedded through the configured router, or by the runtime on this machine when the model carries a `local:` prefix (`local::embed_texts`, `/api/embed` batch with an older per-prompt `/api/embeddings` fallback, no router key needed) and cached in `embedding_cache` by model + symbol + file digest, ranked by cosine similarity, served by `GET /api/index/semantic?q=`. Off until `embedding_model` is set (the route then answers 400), a provider error is a 502 rather than a silent fallback to the word search, and one search embeds at most 128 new documents, reporting the rest as `pending`. The Index sidebar tab (1.64.0) shows what the index holds, refreshes it, searches by words or by meaning, opens a hit's file, and carries the embedding-model field. Browser-checked in 1.65.0 (word and meaning search, model save, hit-to-file, refresh); its segmented control now auto-fits five tabs |
| Codebase indexing / symbols / incremental updates | Working (1.59.0) | `app/src/index.rs`: bounded walk skipping build/dependency folders, secret-shaped files never read, privacy exclusions applied before reading, 1 MiB and NUL-byte skips, 5,000-file ceiling reported as `truncated`; symbols per language family with path + line + kind + name; refresh by content digest (unchanged untouched, changed re-extracted, deleted dropped); ranked search over names and paths; `GET /api/index`, `POST /api/index/refresh`, `GET /api/index/search?q=`. No UI yet and symbols are line scans, not a parser |
| RAG over the project | Working (1.60.0) | chat turns carry the index's ranked matches: one snippet per file, the declaration plus its opening lines read from disk at request time, labelled `path:line — kind name`, 4 snippets / 700 tokens, reported as `retrieved` in the reply meta. Word-based, so no embeddings are involved; `deepsearch.rs` remains web research, a different thing. Agent runs open with the same two blocks through `agent::run_background` (1.61.0), with memory read quietly so a run's repeated asks leave `uses` alone, and the block is stored on the run at promotion (1.62.0): `context` on the run carries the memory and code labels, the card says `given 1 memory + 1 code`, the expanded run names them, and the steps read the stored block back instead of recalling again |
| Memory records + management UI | Working (1.58.0) | `app/src/memory.rs`: `memory_records` with kinds decision/preference/task/fact, a required title, optional details and a scope (one project or every project); `GET /api/memory`, `POST /api/memory`, `POST /api/memory/:id/delete`, local-origin guarded; recall by shared words (title 3, body 1, floor 2, 6 records / 900 tokens) with `uses`/`last_used_at` and `memory_used` in the reply meta; sidebar Memory pane with form, list and forget. The model never writes a record |

## Phase 9 — Developer environment

| Item | Status | Notes |
|---|---|---|
| File tree, editor, save, workspace search | Working | `workspace.rs` + `ide.js` |
| File tree follows the disk | Working (3.0.0) | `GET /api/workspace/fingerprint` hashes path/mtime/size (4000 files, depth 6, generated trees skipped); the Files pane polls every 4 s and re-reads the open folders only when the hash moves. A poll, not a watcher |
| Real PTY terminal (ConPTY/unix pty, SSE, resize) | Working | `terminal.rs`, `portable-pty` |
| Git status | Working (1.66.0) | `/api/workspace/vcs` reports the branch, `ahead`/`behind` from `# branch.ab`, and per-file `staged`/`unstaged`/`untracked`/`conflicted` flags parsed from `git status --porcelain=v2` |
| Git diff, stage, unstage, discard, commit, branches, log, fetch, pull, push, per-hunk and per-line staging (mouse and keyboard), commit detail, remotes (list, add, remove, change URL), worktrees (list, add, remove) | Working (1.91.0) | `app/src/git.rs`: fifteen routes under `/api/workspace/git`, each a real `git` invocation in the open folder. Fetch resolves a configured remote; pull is `--ff-only` and refuses a diverged history with a 409; push never forces and sets the upstream once. Remote commands have a 90 s deadline, `GIT_TERMINAL_PROMPT=0`, drained streams and 64 KiB per stream; auth/unreachable is a 502. A hunk is selected by index server-side and applied with `git apply --cached` (reversed to unstage); the client never supplies patch text, and `show` validates the hash before argv. The same two routes take optional changed-line indexes and rebuild the hunk from git's own diff — removals paired with their additions, unselected lines kept as the text the index holds, `@@` counts recomputed — so one line moves without its neighbour. Remotes are listed, added and removed from the pane through `/api/workspace/git/remotes`: names and URLs are narrowed before argv, and removing one never touches a file or a commit. A remote's URL can be changed in place through `/remotes/set-url`, which refuses a name that is not configured rather than creating a second remote. Hunk lines are keyboard-operable since 1.81.0: one tab stop per hunk, arrows between changed lines, Space or Enter toggles the focused one, and `aria-pressed` follows the selection. Since 1.82.0 `A` selects a whole hunk and Escape clears it, with `All`/`Clear` buttons on each hunk, and the hunk head reports a polite `aria-live` selection count since 1.83.0. A selection spans the hunks of its file since 1.84.0 and is moved in one action from the bar above them; since 1.85.0 it also spans files, with a pane bar that moves or clears them together. Since 1.86.0 a commit refuses content that still holds conflict markers, and since 1.87.0 it also refuses files that mix CRLF and LF. Since 1.88.0 the selection is stored per workspace and survives a reload, since 1.89.0 a Check button reports conflict markers and mixed line endings across tracked files, and since 1.90.0 the first render of a workspace reports them by itself. Since 1.91.0 worktrees are listed, created and removed from the pane through `/worktrees`, `/worktrees/add` and `/worktrees/remove`, though agent runs do not execute in one yet. Paths are validated before argv (relative, no `..`, no drive letter, no control characters, no `.git`) and passed as an array, never a shell. Every mutation is gated on the `git` action (deny 403, ask 409 until `approve: true`, audited). Failures surface git's own sentence with 409 for a refused checkout / nothing to commit. UI: per-row diff, stage/unstage/discard, commit box, branch create/switch, history list, conflict/staged/changed/untracked groups |
| Git merge and abort | Working (1.74.0) | `POST /api/workspace/git/merge` (body `branch`) merges a real branch into the checked-out one; `POST /api/workspace/git/merge/abort` runs `git merge --abort`. The tree must have no uncommitted tracked changes (untracked files are ignored, as git ignores them). The branch name is narrowed before argv (no leading dash, no `..`, ASCII ref characters only) and `--no-edit` is passed so a terminal-less process never waits on an editor. A clean merge answers `{branch, current, fastForward, summary}`; a conflict answers 409 with `{conflicted: [paths], output, inProgress}` and leaves the markers to resolve; a missing branch is a 400; abort is a 409 when nothing is in progress. Both routes are gated on the `git` action. UI: Merge… form, conflicted files named in the toast, Abort merge control |
| Git checkpoint before a run writes | Working (1.75.0) | any run started with writes or commands snapshots the whole tree before its first step: `git::snapshot_tree` builds a commit over a throwaway `GIT_INDEX_FILE`, so nothing in the user's index, worktree or history changes and untracked files are included; the commit is kept in `run_checkpoints`, first snapshot wins on resume. `POST /api/runs/:id/checkpoint/restore` restores tracked paths that differ, removes files the snapshot never had, refuses a live run (409), caps at 500 paths, and consults `fs_write` per path before writing; untracked files are left alone and the index is not rewritten |
| Git rebase, continue, skip and abort | Working (3.2.0) | `POST /api/workspace/git/rebase` (body `branch`) replays the checked-out branch's commits on top of another branch; `/rebase/abort` restores the branch where the rebase started. A dirty tracked tree and the already-checked-out branch are refused (untracked files are ignored, as git ignores them), the branch name is narrowed before argv, and both routes are gated on `git`. Success answers `{branch, current, summary}`; a conflict answers 409 `{conflicted, output, inProgress}` and leaves the tree to finish or abort; abort is a 409 when nothing is in progress. A resolved conflict is staged and finished with `/rebase/continue` (a 409 names any still-unmerged path, a second conflict is reported, and a finished rebase refuses; `-c core.editor=true` keeps a terminal-less git off an editor). Skip (`/rebase/skip`) runs `git rebase --skip` behind the same `git` gate, refuses with no rebase in progress, reads the dropped subject from `REBASE_HEAD` and reports the next conflict or a still-in-progress rebase like Continue. UI: Rebase… form, Continue, Skip commit and Abort rebase controls, conflicted files named in the toast |
| Git worktree per run | Working (2.1.0) | a writing run gets `<repo>-run-<id>` on a `run/<id>` branch, can be applied back and cleaned up (2.0.0); read-only runs and non-git folders keep the shared root |
| Git reflog undo | Working (3.18.0) | `GET /api/workspace/git/reflog` lists the newest 50 `HEAD@{n}` entries (`hash`, `selector`, `action`, `date`) and `inProgress`; an unborn branch answers an empty list. `POST /api/workspace/git/undo` takes a selector the server itself listed, resolves the commit from that entry (a raw hash, another ref's reflog or `HEAD@{0}` is a 400) and runs `git reset --hard` behind the `git` action. It refuses 409 while a merge or rebase is unfinished or tracked files are modified, because the tree is reset with the branch; untracked files are ignored. The reset is itself a reflog entry, so the same list redoes it. The pane's `Undo…` lists the entries, marks the current one, disables Restore during a merge or rebase, and confirms branch, selector, short hash and action |
| Format buffer | Working (3.23.0) | `POST /api/workspace/format {path, content}` returns formatted text only, never a write: rustfmt (stdin, edition 2021, project config), gofmt, or Prettier (folder `node_modules/.bin` first, then PATH, `--stdin-filepath`). Unknown extensions, missing tools, parse errors, 20 s timeouts, empty output and output over 2 MiB are 400s naming the reason; the buffer is marked unsaved and Ctrl+S uses the normal gated save. Workspace-relative paths only. The editor has a Format button and palette command; the contract id `editor_format` hides it where the server cannot serve it |
| Syntax highlighting, diagnostics, completion | Working (3.27.0) | Highlighting: the workbench's own `#codeLayer` (`.code-wrap` + transparent textarea, present since 1.26.1) is painted from `app/static/highlight.js`, a pure tokenizer for 22 language names that escapes every piece first; unknown languages and buffers past 200,000 characters stay escaped and plain. Its old regex engine was deleted in 3.27.0. Completion: `GET /api/index/complete?prefix=&path=` ranks indexed symbols (prefix above contains, +2 in the buffer's own file, +1 in its folder, one entry per name) and `Ctrl+Space` inserts one as an ordinary edit, 20 by default and 50 at most; a one-letter prefix is a 400 and names are matched as plain text, so `%` is a character. Diagnostics: the runner's Problems panel (1.73.0). No language server, so no type information, no completion for locals the index never saw and no inline diagnostics. Definition (3.28.0): `GET /api/index/definition?name=&path=&limit=` returns every declaration of the exact name, ranked exact spelling then the buffer's own file, its folder, the shorter path and the earlier line; `F12` jumps to one match or opens a chooser for several and drops an answer whose caret moved. Symbols (3.29.0): `GET /api/index/symbols?path=&limit=` lists one file's declarations in source order (200 by default, 2000 at most), and `Ctrl+Shift+O` jumps to one or opens the same chooser for several. References (3.30.0): `GET /api/index/references?name=&path=&limit=` reads the indexed files (the buffer's first) for whole-word mentions, one row per line with its column and text, 200 by default and 500 at most under a 32 MiB read budget with a `truncated` flag; Shift+F12 jumps to one or opens the chooser. It finds calls and comments but knows nothing about scope. Replace (3.35.0) rewrites the current match or every match through the find bar (Ctrl+H), case-insensitively, with `replace.js` a pure literal scan. Crash recovery (3.33.0): a dirty buffer is mirrored into the app's database 1.2 s after typing stops and offered back when the file is opened, never written until saved; drafts are scoped to the folder, 1 MiB each, 200 rows and 16 MiB per folder with the oldest pruned, listed in the Files pane for files that are not open and cleared per path when a file or folder is deleted (3.34.0) |
| Build/test runner + diagnostics in the Problems panel | Working (1.73.0) | `app/src/runner.rs`: detects `cargo build`/`cargo test` from `Cargo.toml`, and `build`/`test` scripts from `package.json` through npm/pnpm/yarn. `GET /api/workspace/runner`, `POST /api/workspace/run` (preset id only, never a command line; gated on `command`; one run at a time; 300 s ceiling; 1 MiB per stream, last 200 000 chars returned). Parses Rust, TypeScript and generic `path:line:col` diagnostics, dedupes, caps at 200, drops paths outside the folder. The Problems pane runs them, shows the raw output, and opens a diagnostic at its real line. Since 1.73.0 a test run also reports counts: `tests {framework,total,passed,failed,ignored,failures}` parsed from libtest's per-binary result lines, Jest, Vitest and node's TAP summary, shown in the pane header with the failing names in its tooltip. A build is never searched and a summary-less test run reports `null`, so zeroes are never invented. Since 3.17.0 the same list carries commands saved in Settings (`kind: custom`, ids `custom-<slug>`, at most 20, each a `name :: command` line run through the platform shell) and the pane has a picker and Run for them; the request still sends an id, never a command line |
| Watch mode | Working (3.21.0) | `POST /api/workspace/watch/start` takes a preset id resolved from `runner::all` (never a command line) and asks the `command` permission once with the full line; that decision covers the session because a background thread cannot prompt. The loop reuses `workspace::fingerprint_tree` every 700 ms and takes a new baseline after each run, so generated output cannot retrigger it. A watch holds the runner's only slot, so manual runs are refused meanwhile. `GET /api/workspace/watch` returns the preset, run count, last real run (exit code, duration, problems, bounded output tail, test summary), `truncated` and `error`; `POST /api/workspace/watch/stop` signals, and the runner kills the command at its next 120 ms poll (with `taskkill /T` on Windows) instead of waiting up to 300 s. Opening or closing a folder stops it. The Problems pane has Watch / Stop watching, polls every 1.2 s, reuses the manual run renderer and restores its state after a reload |
| Debugger | **Missing** | there is no debug adapter. A user-defined command preset (3.17.0) still covers the "run any command" case, and watch mode (3.21.0) reruns it on changes |

## Phase 10 — Safety and permissions

| Item | Status | Notes |
|---|---|---|
| Path confinement, symlink escape prevention | Working | canonicalised, `MAX_FILE_BYTES` 1 MB |
| Loopback `Host`/`Origin` write guard | Working | `host_security.rs` |
| Outbound secret detection | Working | `privacy.rs` |
| Host API bearer auth + TLS + body caps | Working | `host_auth.rs`, `host_transport.rs` |
| Central policy store (once/session/project/deny) | Working (1.31.0) | `permissions.rs`; rules scoped once/session/project/always, resolved longest-target → deny → scope |
| Privileged UI routes gated | Working (1.40.0, 1.66.0) | terminal open, editor saves, workspace create/rename/delete, settings/key saves and — since 1.66.0 — git stage/unstage/discard/commit/branch create/switch consult saved rules on their own actions (`command`, `fs_write`, `settings`, `git`); deny → 403, ask → 409 until `approve: true`, both audited. No rule means no prompt — the click is the consent. UI routes pass an empty project, so `project`-scoped rules still apply to agent steps only. Updates have no in-app route |
| Enforcement of that store | Working (1.40.0, 1.66.0) | agent steps, router installs and MCP calls consult it (1.31.0–1.33.0); terminal open, editor saves, workspace create/rename/delete, settings/key saves (1.40.0) and the Git routes (1.66.0) do too. The only privileged path with no route to gate is an application update, because there is no in-app update |
| OS keychain for secrets | Working (1.42.0) | `keychain.rs` stores `api_key`, `search_api_key`, `openai_api_key` and MCP OAuth material in Windows Credential Manager / macOS Keychain / Linux Secret Service; startup migrates plaintext and blanks the file, a store failure falls back to the permission-restricted file, and the store mode is reported in `GET /api/config` |
| Privileged-operation audit log | Working (1.31.0) | `permission_audit` table + `/api/permissions/audit` |
| Global kill switch | Working (1.41.0) | `POST /api/emergency-stop` cancels every live agent run (flagged and finished, mirroring per-run cancel), reaches parked/paused runs, and closes every terminal session; deliberately not policy-gated, always audited |
| Permissions UI | Working (1.32.0) | Settings → Permissions: add a rule, list rules in force, revoke, read the audit trail |

## Phase 11 — Token, context and cost

| Item | Status | Notes |
|---|---|---|
| Token-budgeted packing | Working | `context.rs`, 1 k–32 k clamp, VS Code parity |
| Usage log + aggregates/series/by-model/by-project | Working (3.40.0) | `request_log`, `/api/usage`; the project roll-up is a 24-hour join over conversations/projects and is shown on the Usage page with tokens, estimated spend and unpriced successes |
| Pricing, cost estimates | Working | `cost.rs` × `capabilities::Price` from the live `/models` row × the provider's reported tokens; `request_log.cost_usd`/`cost_basis`, unpriced rows counted rather than zeroed. Native OpenAI Platform picks stay unpriced (no mirrored price table) |
| Budget enforcement that stops a run | Working | `daily_budget_usd`, rolling 24 h (timestamps are UTC), checked once before the fallback chain and before a stream; refusal is non-retryable and names spend, budget and unpriced count. Zero = no limit; local inference exempt |
| Compression beyond head/tail cropping | Working | `context.rs`: whitespace normalisation, repeat-once dedup, digest of labelled excerpts capped at a quarter of the budget, first message kept as a pointer; a fitting thread is untouched and every condensed/dropped message is reported in meta. No model call, so no invented summaries |
| Token usage per agent | Working (3.43.0) | `/api/usage` `by_agent`: 24 h over `agent_runs`, grouped by profile (blank reads `(no profile)`), with `runs` and the run's own recorded `tokens`, largest first; shown as a **By agent** card. No spend is derived from a run that spans several priced calls || Rate-limit monitoring | Working (3.42.0) | `/api/health/providers` counts per-model `rate_limited` and `last_rate_limit_at` over 24 h, only when the provider's stored error mentions `429` or `rate limit`; the Provider health row prints `N rate-limited`. Nothing is inferred from latency or a generic failure || Context-utilisation display | Working | context meter `~used / budget · %` with an over-budget state; `context_budget` in reply meta for other clients |

## Phase 12 — Live agent monitoring

Working for monitoring (1.51.0), controls (1.52.0) and the live step view
(2.29.0).
`agent_runs`/`agent_steps`, SSE events and a runs panel with diffs,
approve/reject, cancel, restore and pause/resume are real backend state;
`GET /api/runs/overview` adds a cross-run dashboard (live, awaiting approval,
paused, failed, summed steps and tokens) built from the same rows as the panel,
and every run reports `tokens`, `elapsedSeconds` and `idleSeconds` with a live
run measured from its start. Pause parks a run at the next step boundary and a
paused run holds no live task, so start/pause/resume/cancel all act on real
state, and the emergency stop reaches parked runs too.

The queue is real as of 1.53.0: a run is created `queued`, takes a place in line
(`queue #N` on the card, a Queued count on the board, a waiting note with Cancel
only while it waits), can carry a `priority` and a `dependsOn` list, and starts
itself when a slot frees and its dependencies succeeded. Retry is a real control
as of 1.55.0 too, and it is distinct from resume: `POST /api/runs/:id/retry`
starts a clean run of the same goal and settings (linked by `retry_of`, logged as
a `retry` step), while resume continues an existing step log. There is no
drag-to-reorder: `POST /api/runs/:id/queue` moves a queued run one place up or
down (3.3.0), rewriting every queued priority into one explicit order inside a
transaction; the card offers Move up / Move down.

## Phase 13 — Workflow builder

Started in 2.9.0: a workflow is a validated dependency graph, and a run is a
persisted execution of it.

| Item | Status | Notes |
|---|---|---|
| Task / agent / approval / condition nodes | Working (2.9.0) | `agent` starts a real agent run and waits for it, including at its approval gate; `approval` parks the workflow; `condition` asserts dependency outcomes; `task` records a note |
| Dependencies and branches | Working (2.9.0) | `dependsOn` ids or `{id, when: "failed"}`; an unmet edge marks the node `skipped`; a failure counts as handled only when its repair branch succeeded |
| Validation before execution | Working (2.9.0) | ids, kinds, profiles, missing deps, self references, cycles, per-kind requirements, a 50-node cap — each error names the node |
| Persisted run history | Working (2.9.0) | `workflow_runs`/`workflow_steps` carry status, output, timing and the agent run id; a boot sweep marks a run the process abandoned as `interrupted` |
| Retry and cancel | Working (2.9.0) | per-node retries (0–3); cancel stops the graph and, since 3.15.0, also cancels the child agent runs it started, naming them in `stoppedAgentRuns` — a child that already finished is left alone |
| Visual editor | Working (2.12.0) | canvas cards in dependency columns with SVG edges (dashed for `failed`), add-by-kind, rename/delete (dependencies rewritten), inspector for every node field, per-edge `when`; the JSON field remains as a synced advanced view. cards can be dragged and the position is stored on the node (2.47.0); graph helpers are unit-tested, but rendering and dragging are not, and the canvas was driven in Chromium once (2.49.0) — drag, live edges, clamping and a position restored after reload — though since 3.16.0 the browser-free harness drives the drag gesture too (card follows the pointer, the edge redraws live, the release commits
x/y, a 2px press writes nothing, a drag past the right edge clamps) while a real browser's layout and pixels are still unverified |
| Graph import/export | Working (2.13.0) | export is a versioned `sarthi.workflow` document; import runs the save-time validation before writing, assigns a new id and a unique name, and starts a fresh schedule window |
| Scheduled execution | Working (2.10.0) | `everyMinutes` 5–10080 (0 = manual), checked every 30 s; the anchor is the later of the last run and the window start, a live run is not stacked, and the window is claimed in SQLite (2.48.0) so two processes sharing one home cannot both start it |
| Resume an interrupted run | Working (2.11.0) | `POST /api/workflow-runs/:id/resume` keeps finished steps, reopens the rest, refuses an edited or deleted workflow; a resumed agent node starts a fresh agent run |

## Phase 14 — Cross-platform integration

| Item | Status | Notes |
|---|---|---|
| Desktop Linux/Windows/macOS | Working | deb, AppImage, tarball, zip, macOS universal tarball |
| Android client | Working (subset) | chat streaming, history, backup, remote agent runs via Host API; no router management, no local execution |
| VS Code extension | Working (subset) | embedded Node server, agent port, runs webview |
| Capability negotiation driving feature gates | Working (3.22.0) | The local API states 25 implemented ids plus enforced limits; the extension's embedded server states its honest subset and its status command reports the match. Since 3.22.0 the shared UI fetches the contract at boot and hides exactly the controls a known contract marks `false` (Source control, Index, Memory, Watch, Saved commands and six Settings sections), keeping everything on an older/unknown server. Android negotiates v1 over the Host API |
| Pairing / TLS / sessions | Working | host token lifecycle + TLS transport |

## Phase 15 — Extension and plugin ecosystem

Manifests are discovered and validated as of 2.15.0, listed in Settings as of
2.16.0, installed/removed as of 2.17.0, and executed as of 2.20.0 inside a
wasmi sandbox (ADR 0001). Since 2.21.0 that sandbox provides workspace
read/write host functions, scoped to the open folder and checked per call
against saved rules, and since 2.22.0 a per-extension data folder through
data_read/data_write; network, terminal and MCP capabilities remain unprovided.
Since 2.25.0 a manifest also declares the builds it works on and the extensions
it needs, and both are enforced rather than displayed.

| Item | Status | Notes |
|---|---|---|
| Manifest (id, name, version, description, author, entry, permissions) | Working (2.15.0) | `GET /api/extensions` scans `<data>/extensions/*/manifest.json`; the id must equal the folder, the version must be `1.2.3`, `entry` must stay inside the folder and exist, and permissions are a closed list |
| Management UI | Working (2.16.0) | Settings → Extensions shows the directory, each folder's validity, description or error, and a Refresh action |
| Install / uninstall | Working (2.17.0) | install copies a folder through staging with validation and rollback, symlink refusal and a 500-file/16 MiB cap; uninstall moves the folder to `.removed/` after a canonical containment check |
| Execution and sandboxing | Working, workspace + own data (2.22.0) | ADR 0001: no in-process execution. `app/src/extension_run.rs` runs a module in a wasmi 0.31.2 interpreter with no WASI: `run() -> i32`, 50,000,000 fuel, 64 MiB memory, 100,000 table elements, 8 MiB module cap. The whole host surface is `env.read`/`env.write` (open folder), `env.data_read`/`env.data_write` (`<extensions>/<id>/data`) and `env.last_error`, linked only when the manifest declares `workspace.read`/`workspace.write`. Paths use `workspace::resolve_inside` and writes `write_text_file`; saved rules apply per call (`ask` refuses because nothing can answer) and every call is audited. No network, terminal, MCP or config access exists |
| Runtime gating | Working (2.19.0, host functions 2.21.0) | `runtime: "wasm"` requires a `.wasm` entry; discovery exposes `runnable`/`blockedReason`; `POST /api/extensions/:id/run` refuses a capability with no host function behind it and otherwise runs the module, answering `{exitCode, fuelUsed, elapsedMs, hostCalls}` |
| Run control | Working (2.23.0) | One run per extension (409 while one is open); `POST /api/extensions/:id/cancel` stops it at the next host call (a pure-compute loop still ends at the fuel limit); a 5-second wall-clock budget is checked at each host boundary; uninstall cancels first; Settings shows `Running…` and `Cancel` |
| Run history | Working (2.24.0) | `extension_runs` records extension, project, start, duration, exit code, fuel, host calls and error for every run that reached the sandbox, including loader failures and cancellations; `GET /api/extension-runs` returns the newest 50 and Settings lists them |
| Compatibility and dependencies | Working (2.25.0) | `requiresStudio` (numeric per-part floor against `CARGO_PKG_VERSION`) marks a manifest `valid` but `compatible: false` when this build is older, and both run and install are refused with both versions named; `apiVersion` defaults to 1 and refuses a higher number at parse; `dependencies` (≤16, deduped, validated, no self-reference) are reported as `missingDependencies` and block a run, not an install. Discovery reads every manifest before judging any. Not covered: a dependency's own compatibility, dependency cycles, and an update path that would re-check after the app is upgraded |
| Custom tools | Working (2.27.0) | `tools` in the manifest (≤16; name, description, optional `.wasm` entry, optional `inputSchema`; the wasm runtime is required and each module is checked like the entry) and the `env.arguments`/`env.result` ABI; `POST /api/extensions/:id/tools/:tool` runs one in the same sandbox with the same budgets, grants, one-run rule and cancel, answering `{result, reported, run}`, gated by `extension_tool` (default ask; `ask` refuses with instructions because the route cannot prompt). Since 2.27.0 the agent's tool loop offers them too: the prompt carries a labelled catalogue of every valid, runnable folder's tools (none when nothing is installed), the `extension` step is gated by the same `extension_tool` permission on the same `extension/tool` target, and the call runs through the same sandbox, budgets, grants, one-run rule, cancel boundary, `extension_runs` row and host-call audit, with `env.result` becoming the step output. Since 2.28.0 the declared `inputSchema` is enforced on every call before the module is loaded (MCP's bounded keyword subset, boolean schemas honoured, an unreadable schema refused) and the prompt lists each tool's required argument names; not covered: keywords outside that subset |
| Provider adapters | Missing | — |
| UI extensions | Missing | — |

## Phase 16 — Testing and quality

| Layer | Status | Notes |
|---|---|---|
| Rust unit/integration | Working | 483 passed, 1 ignored at the 2.24.0 run; the count for each release is recorded in `PROGRESS.md` |
| VS Code extension | Working | `node --test` suites |
| Android | Working | JVM + emulator instrumentation + validation gate |
| Release-pipeline regressions | Working | `scripts/ci/test-version.sh` |
| Frontend | Working (2.33.0) | `scripts/ci/test-frontend.sh` → `node --test app/static/tests/*.test.js`: the real `app.js` is evaluated in a `node:vm` context with a stub DOM, and 21 cases cover escaping, the markdown subset (including a `javascript:` link staying text), byte and time formatting, path keys, the icon fallback, the workflow graph helpers (layout by dependency depth, add/rename/delete with edge rewriting, per-edge conditions), and a render smoke (2.33.0) that runs `renderWorkflowCanvas` (a card per node, an edge per dependency, an empty-graph note) and evaluates `ide.js` on top of `app.js` (chrome built, `api` wrapped, workbench inside the main pane). Wired into `desktop-ci.yml`. Still not a layout or pixel check — that needs a browser |
| End-to-end (launch → chat → agent → restart) | Working (2.8.0) | `scripts/ci/test-e2e.sh` launches the real binary against a scratch `FREE_AI_STUDIO_HOME`, boots the mock provider, runs a chat turn and reads the two stored messages, checks the process log and readiness, opens a workspace folder, starts an agent run that parks at its write approval gate, approves it, sees the write on disk and the `write, verify, done` step log, then kills the process and re-reads both the conversation and the run with its steps from the database; and finishes with two failure cases — a killed provider (the run must fail with a recorded error while readiness keeps answering) and a process killed while a run has a recorded step and its next call in flight (the restarted app must report `interrupted` with a readable log); 28 checks |
| Failure-injection matrix | Working (2.34.0) | Four process-level cases in `scripts/ci/test-e2e.sh`: (1) the mock provider is killed and the next run must reach `failed` with an error while `/api/ready` keeps answering; (2) the provider is slowed and the process killed mid-run, and the restarted app must mark that run `interrupted` with a readable step log; (3) the database is replaced with garbage — the process must exit with `file is not a database` in its log, nothing may answer on the port, and restoring the saved copy must bring it back ready; (4) the data directory is made read-only (POSIX only: Windows Git Bash ignores `chmod`, so the case prints `skip`) and the `data directory` readiness check must go false and then recover. SQLite's locked and full-database paths are unit-tested since 3.39.1 (a waiting writer, and `max_page_count`'s `SQLITE_FULL` with the rows intact and `quick_check` clean); a true filesystem ENOSPC (needs a privileged loop/tmpfs mount) and browser-level layout are still not run |
| Install smokes in CI | Working (2.31.0) | `scripts/ci/smoke-package.sh` unpacks a tarball, zip, `.deb` (`dpkg-deb -x`) or AppImage (`--appimage-extract`), boots the packaged binary headless against a scratch home, checks `/api/ready`, serves `/`, and compares `/api/diagnostics/bundle`'s `app.version` with `app/Cargo.toml`. `desktop-release.yml` runs it on the `.deb`, the Linux tarball and AppImage, the Windows zip and the macOS tarball; VSIX and APK are not covered. Verified locally on a zip and a tarball; the `.deb` and AppImage paths run only in the Linux release jobs |
| Windows Rust CI job | Working (2.32.0) | `desktop-ci.yml` builds the default `gui` feature and runs `cargo test --locked` on `windows-latest`; WebView2 and MSVC come with the runner and mold/`-Z threads` are Linux-scoped. Linux owns the formatting gate and the end-to-end smoke; since 3.34.1 this job also runs clippy, so Windows-only code is linted. Verified locally with the same commands on Windows; the GitHub job itself runs on push |

## Phase 17 — Observability and diagnostics

Partial, with the read model and the export shipped in 1.72.0. `app/src/diagnostics.rs`
is a read-only projection over the records the app already writes — it adds no
table and no second log pipeline, so it cannot disagree with the panels:

| Item | Status | Notes |
|---|---|---|
| One timeline with a correlation id per row | Working (1.72.0) | `GET /api/diagnostics?limit=&area=` merges `request_log`, `permission_audit`, `agent_runs` and failing `agent_steps` (`fail`, `mcp-error`, `write-conflict`), newest last, with a level (`info`/`warn`/`error`) and the run id, project or conversation as the correlation key. Limit defaults to 100 and is clamped to 500; an unknown area is a 400. Loopback `Host`/`Origin` guarded |
| Counts per area | Working (1.72.0) | From the same rows: provider requests/errors over 24 h, permission decisions by the decision names stored in the table, agent runs by status, live runs |
| Support bundle with redaction | Working (1.72.0) | `GET /api/diagnostics/bundle`: version, OS/arch, non-secret settings, credential booleans, summary and timeline. Run goals are excluded; every string is redacted (`sk-`/`ghp_`/`xoxb-`, `Bearer`, sensitive `name=value`, long opaque runs) and clipped to 400 chars |
| Diagnostics UI | Working (1.72.0) | Settings → Diagnostics: timeline with area/level colour and the short correlation id, server-side area filter, row count, 24-hour summary line, and a bundle download the browser saves without composing it |
| Structured log file + console ring | Working (2.2.0) | `app/src/oplog.rs` appends one redacted JSON object per line to `studio.log` beside the database, flushed per write, rotated at 1 MiB into one previous file, with a 200-entry ring in memory; `GET /api/diagnostics/log?limit=` serves the ring and Settings → Diagnostics shows it with the file's path. Written at boot, router install/update/uninstall/restart, permission ask/deny, provider failures and every run ending |
| Request id for non-run operations | Working (2.4.0) | `trace.rs` gives every local API request an id, kept in a task-local for the handler's lifetime and echoed as `x-request-id`; a caller's own id is accepted only if it is 8–64 characters of alphanumerics/`-_.`. `permission_audit` gained a `request_id` column (migrated in place, old rows keep `''` rather than a guessed id), so a terminal open, a Git stage or a settings save is now tied to the call that asked for it; the timeline and the process log both show it |
| Workflow failures in the timeline | Working (3.44.0) | `workflow_runs` rows in a bad terminal state join the same list: failed/cancelled/interrupted only, with the run id as correlation, the graph name and the stored error; cancelled is `warn`, the others `error`. The area is a server-side filter and a panel option, and `summary.workflow.failed_24h` feeds the 24-hour line. Successful runs are omitted || Router installs and MCP connections in the timeline | Working (2.3.0) | `timeline_value` adds one row per router from `router-installs.json` (the newest record, since that file keeps one per router) and one per MCP server that has actually answered, at its own `checked_at` — a server never contacted adds nothing rather than a row stamped "now". Areas `router` and `mcp` are valid filters, and the panel offers them |
| Health/readiness dashboard | Working (2.4.0) | `GET /api/ready` runs only cheap local checks — database opens and answers, the data directory accepts a write (a probe file, then removed), provider base URL and whether a key is saved plus the credential store mode, latest router install outcomes, and MCP servers by health. Only the first two are `required`; the rest change what works, not whether the app runs. Shown at the top of Settings → Diagnostics |

## Phase 18 — Installation and updates

| Item | Status | Notes |
|---|---|---|
| Linux `.deb` / AppImage / tarball | Working | AppStream validates clean |
| Windows packages | Working | portable zip and per-user MSI; no per-machine option |
| macOS package | Working (unsigned) | Gatekeeper right-click → Open |
| Android APK/AAB | Working | debug-signed release build |
| VS Code VSIX | Working | published |
| DB migration rollback, newer-build guard | Working | 2.35.0: versioned schema, `VACUUM INTO` copy verified before an upgrade, restore on a failed migration, byte-for-byte refusal of a newer schema |
| Update check (read-only) | Working | 2.36.0: `GET /api/update/check` reads the newest release, compares versions numerically, names this platform's asset; About card renders it |
| Download and verify into staging | Working | 2.37.0: `POST /api/update/stage` downloads the named asset through the verified-download path, refuses an unverifiable or non-https one, records it in `updates/staged.json` |
| Unpack and check a staged update | Working | 2.38.0: re-verifies the sha256, unpacks an escape-proof `.zip`/`.tar.gz` into `updates/prepared/<version>`, refuses a tree with no binary |
| Replace the installed binary | Working | 2.39.0: copies the prepared binary beside the target, hash-checks the copy, keeps `<name>.old-<version>` and restores it if the new one cannot take its place; a restart runs it, and 2.40.0 removes the leftover backup at the next start |
| Installer and uninstaller | Working (2.43.0 / 2.44.0) | per-user WiX MSI with Start Menu shortcut and major-upgrade code; `GET /api/uninstall/preview` + `POST /api/uninstall` remove the trees an uninstall owns unless kept, with the About card driving it |

## Remaining work at 3.8.0

The ordered backlog lives in `MASTER-ROADMAP.md` under the same heading. In brief:

1. **Phase 1 — release reliability.** Get a green tagged run with the 11-asset
   set, confirm the deb headless tests, pass the existing install smokes, add
   and decide the historical tags. This is the only release
   blocker.
2. **Phase 4 — OpenAI Platform.** Complete in the tree since 3.13.0 (structured
   output, then native tool calling); only a live platform call is unverified.
3. **Phase 14 — cross-platform.** Android router management/local execution,
   VS Code parity, and capability negotiation instead of hand-wired gates.
4. **Phase 15 — extensions.** Provider adapters and UI extensions    (dependency cycles, dependency compatibility and the post-upgrade re-judge
   shipped in 3.9.0/3.14.0).
5. **Phase 18 — packaging.** Code signing/notarization, a macOS `.dmg`, and a
   per-machine MSI choice.
6. **Smaller:** Phase 3 native OpenAI pricing/context, Phase 6 per-skill
   permissions, Phase 8 local embeddings, Phase 13 automated canvas coverage and
   child cancellation. Phase 9's editor engine/debugger/watch mode and Phase 3
   pricing are product decisions, not missing routes. Phase 9's watch mode
   shipped in 3.21.0; only its editor engine and debugger remain.

Already closed in substance: Phases 2, 5, 7, 10, 11, 12, 16 and 17; Phase 13's
graph engine, canvas, scheduling and resume; Phase 18's installers, updater and
portable uninstall.

## Identity and branding

| Area | State | Detail |
|---|---|---|
| Display name | Working (2.51.0) | `Sarthi Studio` in the window/tab title, About, settings, empty states, toasts and docs; no `Free AI Studio` or `Saarthi Studio` remains in tracked text |
| Package and installer metadata | Working (2.51.0) | `productName`, crate description, MSI product/shortcut/uninstaller names, AppStream name and desktop entry, Android `android:label`, VS Code `displayName`, artifacts `sarthi-studio_<version>_<platform>`; AppStream validation is still outstanding (KNOWN-ISSUES 33) |
| Mascot and icons | Working (2.52.0) | Tardigrade face, flat orange/black/white, from one geometry (`scripts/brand/build-brand-assets.py`); sidebar, welcome, favicon, Windows `.ico`, Linux desktop/AppImage/.deb, macOS bundle and Android adaptive launcher. Rasters rendered from the canonical SVG; the MSI/macOS/Android icons are unverified on their platforms (KNOWN-ISSUES 35) |
| Technical identity | Deliberately retained (2.51.0) | crate/binary, env vars, config/data paths, bundle and Android ids, conversation image marker, VS Code ids and repo slug keep old names so installs and stored data survive; reasons and migrations in `docs/BRANDING-AUDIT.md` |
