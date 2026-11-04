# Changelog

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
