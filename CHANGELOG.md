# Changelog

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
