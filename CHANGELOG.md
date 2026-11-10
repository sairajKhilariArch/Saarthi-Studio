# Changelog

## 2.52.0

The mascot is a tardigrade (water bear), and it is now the application's icon
everywhere: sidebar, welcome state, favicon, Windows icon and MSI, Linux
.desktop/AppImage/.deb, macOS bundle and the Android adaptive launcher.

- The mark is the tardigrade's **face**, not its body, drawn flat in exactly
  three colours — orange `#FF7A1A`, black `#111111`, white `#FFFFFF`. No
  gradients, so it stays legible at 16 px and reads identically on light and
  dark surfaces (the tile behind it is solid black).
- One geometry in `scripts/brand/build-brand-assets.py` generates every SVG:
  the full-colour face, mono white, mono black, a 24 px mark, a full-bleed app
  icon, the favicon and a splash/wordmark. `docs/BRAND-ASSETS.md` records the
  inventory and the three-platform mapping.
- Raster assets are generated from the canonical `app-icon.svg`, not drawn
  twice: `app/icons/icon.png` (512), `app/icons/icon.ico` (16–256, seven sizes)
  and `app/packaging/icons/sarthi-studio-256.png`.
- The UI uses an inline `#i-tardigrade` symbol with fixed brand colours for the
  sidebar brand tile and the chat welcome state; feature icons are unchanged.
  The old spark glyph and the old favicon are gone.
- Android ships the same face as a `VectorDrawable` inside the 66 dp safe zone,
  over a `#111111` background, and reuses it for the monochrome (themed) layer.
- Packaging now points at the canonical SVG: the duplicate
  `app/packaging/icons/sarthi-studio.svg` is deleted and `Cargo.toml` plus both
  desktop workflows reference `app/static/brand/app-icon.svg`.
- Also fixed: the About card claimed the data lived in `~/.local/share/sarthi-studio`
  and `~/.config/sarthi-studio`; the real paths are `free-ai-studio`, as the
  rest of the app uses.
- Verified: 512 px render inspected, sizes 16/24/32/48/64/128/512 checked, the
  icon seen in the running shell in dark and light, `cargo test --locked` 547
  passed (1 ignored), clippy clean, frontend 43, version 20, bundle 13,
  installer 11, e2e 32. Not verified: the MSI's own icon on a clean Windows
  install, the macOS `.app` icon, and the Android launcher on a device.

## 2.51.0

Identity: the application is **Sarthi Studio** everywhere a person can see or
install it, and the old names are gone from the source, the installers and the
docs.

- Renamed 346 brand occurrences across 98 files: `Saarthi Studio` → `Sarthi
  Studio`, `Free AI Studio` → `Sarthi Studio`, and the identifier shapes
  (`SaarthiStudio`/`saarthi-studio`/`SAARTHI` → `SarthiStudio`/`sarthi-studio`/
  `SARTHI`). The audit that preceded it found 391 `saarthi*` tokens in 113 files
  and 31 `Free AI Studio`; `docs/BRANDING-AUDIT.md` records the result.
- Product metadata: `productName` is `Sarthi Studio`, the crate description names
  it, and the Windows installer's product/shortcut/uninstaller names, the
  AppStream `<name>`, the Android `android:label` and the VS Code `displayName`
  all read Sarthi Studio. Artifacts are `sarthi-studio_<version>_<platform>`.
- Packaging files renamed: `io.github.sairajkhilariarch.SarthiStudio.{desktop,
  appdata.xml}` and `icons/sarthi-studio{,-256}.{svg,png}`. The `.desktop` rename
  also fixed a real mismatch — `Cargo.toml` and both desktop workflows already
  referenced the new filename.
- The Linux UI tree now installs under `usr/share/sarthi-studio/static`. The
  runtime still resolves `usr/share/free-ai-suite/static` last, so a package
  installed before 2.51 keeps starting; a unit test asserts that order.
- Deliberately unchanged, because changing them breaks installs or stored data:
  the `free-ai-studio` crate/binary and `FREE_AI_STUDIO_*` variables, the config
  path `~/.config/free-ai-studio/`, the data directory
  `~/.local/share/free-ai-studio/`, the Tauri identifier
  `com.saarthi.freeaistudio`, the Android package `com.saarthi.studio`, the
  `[[SAARTHI_IMAGE:…]]` marker stored in conversations, the `freeAiStudio.*`
  VS Code command/setting ids, and the GitHub repository slug. Each has its
  reason and a migration path in `docs/BRANDING-AUDIT.md`.
- Workflow export writes `"format": "sarthi.workflow"`; import never read the
  field (it validates name/prompt/nodes only), so documents exported under the
  old value still import.
- No data migration is needed or performed: no path holding user data moved.
- Verified: `cargo test --locked` 547 passed (1 ignored), clippy
  `--all-targets -D warnings` clean, frontend 43, version 20, bundle 13,
  installer 11, e2e 32, and a second `git grep` audit leaving only the listed
  identifiers.

## 2.50.0

P0: in the IDE shell a fresh install opened a thread into a **zero-width** panel —
a blank workspace, an 8 px composer, nothing typeable. It was not the thread code:
threads were created, opened and stored correctly the whole time.

- Root cause: `app.js` defaults `panelShow.right = false`, because in the legacy
  shell that panel is only the router/usage rail. `ide.js` moves the chat view into
  the AI dock, which occupies the grid column sized by `--right-w`; `body.no-right`
  sets that to `0px`. So the conversation loaded into a 0 px column. A second bug
  hid it: `applyPanels()` ran before `buildDock()`, so the dock never received its
  `hidden` class and sat visible at 1 px.
- Fix: the default is now one testable rule (`defaultPanelShow(stored, hasDock)`) —
  the docked shell opens the dock on a first run, an explicit choice (closed
  included) still wins — and the IDE init re-applies the panels once the dock
  exists. Measured: chat `0 → 299px`, composer `8 → 253px`, and the whole app's
  `-224px` offset went to `0` once the column was sized.
- Opening or creating a thread now reveals the dock, so a thread never loads into a
  closed panel.
- The open thread is remembered (`freeai-current-thread`) and restored on reload,
  falling back to the newest thread when it no longer exists.
- The status bar's connection label now updates with its dot; it previously said
  "Router connected" while the dot was `bad`.
- Fixed `/api/usage` answering 500 on every fresh home: Rust string continuations
  swallow the next line's indentation, so two clauses had run together
  (`END)FROM request_log`, `conversation_idLEFT JOIN`). The query is now a named
  constant with a regression test that prepares and runs it.
- Tests: 1 Rust (the usage query parses and runs against its schema) and 4 frontend
  (thread restore, remember/forget, the panel default, the connection label).
- Verified in Chromium against a headless build with the mock router: new thread
  opens with the composer focused; a message sends and the mock replies; three
  threads keep separate histories and switching between them (by function and by
  row click) shows the right one; a reload restored the selected thread with its
  messages; an empty thread shows the welcome state; a long thread (3 turns, 6
  rows) matches storage exactly and scrolls; with the provider stopped the reply row
  reads "Could not reach router…", the dot turns `bad` and the status says "Last
  request failed"; `Problems` shows 0.
- Version 2.50.0 across `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- Not verified: the Tauri desktop window itself — this was the headless server plus
  Chromium, which is the same static UI and the same routes.

## 2.49.0

The canvas was unit-tested but never driven. This release drove it in Chromium,
which found one real defect and confirmed the rest.

- **Defect found and fixed:** a card's `pointerdown` called
  `setPointerCapture` unguarded. When the browser no longer had that pointer active
  — a tap that ended before the handler ran, or any synthetic event — the call
  threw and aborted the handler before a single move listener was attached, so the
  card did not move at all. The call is now guarded; without capture the drag still
  follows moves that land on the card.
- **Verified in a real browser** (headless binary, scratch home, Chromium at
  1440×900, pointer events dispatched through the same listeners a user's pointer
  reaches): dragging `build` moved it 264,24 → 344,74 with the card carrying
  `dragging` and the two connected edges redrawing during the move; dragging
  `repair` far past the edge clamped x to 553 (canvas width 767 − card 190 − 24)
  and grew the canvas to 4104 px instead of clipping it; releasing wrote the
  position into the synced graph JSON without touching the node's note.
- **Persistence checked end to end:** saving through the form put `build 344,74`
  and `repair 553,4000` into `workflows.json` (read back from disk and from the API
  with a cache-busting query), and after a full page reload, opening that workflow
  rendered the cards at exactly those offsets with the canvas at 4104 px, so the
  position survives save-time validation and drives the layout again.
- A workflow saved before the drag was fixed renders in dependency columns, as it
  should: positions are opt-in per node.
- Also observed, unrelated to this change: a fresh home answered
  `GET /api/usage` with 500 on first load. Recorded in `KNOWN-ISSUES`, not
  diagnosed here.
- Version 2.49.0 across `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- Verified: `node --check` clean; frontend 39; version 20. Rust unchanged since
  2.48.0, so its 546-test run was not repeated. Not verified: an OS-level drag
  (these were pointer events dispatched in the page, not a mouse moving across the
  screen) and any automated browser test — there still is none.

## 2.48.0

Scheduled workflows were guarded against stacking inside one process, but every
process runs its own timer. A desktop app and the headless server over the same
home would both see a workflow as due and start two runs for one window.

- A due time is now claimed in SQLite, the one place every process writes, with a
  single conditional upsert that succeeds only when it moves the stored window
  forward. The first process to reach a window owns it; the others skip it. Because
  the predicate is "later than what is stored", a clock that goes backwards cannot
  reopen an already-served window, and workflows claim independently.
- A run that fails to start hands the window back, so a transient error retries on
  the next tick instead of skipping a whole interval. A release for a window that
  has since moved on is ignored, so a late failure cannot free a served window.
- Deleting a workflow drops its claim; a claim only moves forward, so one left
  behind could otherwise suppress a later workflow that reuses the id.
- A process that cannot open the database starts no scheduled work at all: there is
  nothing to arbitrate with and nowhere to record the run.
- Tests: two Rust tests at the claim layer (one window claimed once across
  processes, forward-only; a released window is retryable and a stale release is
  ignored) and the schema's idempotent-creation test now covers the new table.
- Version 2.48.0 across `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- Verified: `cargo test --locked` 546 passed, 1 ignored; clippy `--all-targets
  -D warnings` clean; frontend 39; version 20; e2e 32. Not verified: two real
  binaries racing over one home — the arbitration is tested at the claim layer, and
  the end-to-end harness runs a single process.

## 2.47.0

The workflow canvas placed every node in dependency columns. A graph with two
unrelated branches had no way to say which one belongs where, and anything the
user rearranged would have been lost on save.

- A node can now carry a canvas position (`x`, `y`). `<code>graphLayout</code>`
  uses it when present and falls back to the dependency columns for a node that was
  never moved, so an existing graph opens exactly as before.
- Cards are dragged with the pointer: a card follows the pointer, the edges that
  touch it redraw as it moves, and on release the position is written back through
  the same path the rest of the editor uses, so the synced graph JSON — and the
  saved workflow — carries it. A press without movement still only selects.
- The position is validated server-side like every other node field: the pair is
  stored, a half or non-numeric coordinate counts as absent, out-of-range values are
  clamped to the canvas, and a graph saved before positions existed keeps its
  canonical form without invented coordinates.
- The canvas grows to fit a card dragged past its edge instead of clipping it.
- Tests: one Rust test (round-trip through save-time validation, clamping, a half
  position, a reload keeping the position) and three frontend tests (a stored
  position beats the layout, drag clamping and rounding, and a drag writing into the
  graph without changing the node's meaning).
- Version 2.47.0 across `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- Verified: `cargo test --locked` 544 passed, 1 ignored; clippy `--all-targets
  -D warnings` clean; rustfmt clean on `workflow.rs`; frontend 39; version 20; e2e 32.
  Not verified: the drag itself has not been driven in a browser — pointer events
  are not exercised by the frontend layer, so the interaction is reasoned and
  unit-tested at the position level only.

## 2.46.0

macOS CI failed at `git::tests::creates_lists_and_removes_a_worktree`. The test was
right and the code was wrong: worktree paths were compared as strings.

- The temporary directory on macOS is reached as `/var/folders/...`, while git
  reports `/private/var/folders/...` — one folder under an alias. Removal compared
  the two spellings with nothing but slash normalisation (plus case folding on
  Windows), decided the worktree belonged to another repository and refused.
  `same_worktree_path` now resolves both sides through the filesystem and falls
  back to the normalised spelling only when a path does not exist, so an alias, a
  verbatim Windows path, a trailing separator and a different case compare equal
  while a sibling folder still does not.
- `remove_worktree` keeps the ownership check and now hands git its own spelling of
  the validated path, so the alias that validated the target is the alias git
  removes. A path this repository does not report — another repository's worktree,
  or one that does not exist — is still refused before git is called.
- Three regression tests: alias, trailing-separator and case comparison (a
  symlinked alias asserted where the platform allows creating one), another
  repository's worktree refused and left on disk then removed by its owner through
  the resolved path, and a nonexistent worktree refused. The failing lifecycle
  test is unchanged and still runs.
- Version 2.46.0 across `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- Verified: `cargo test --locked` 543 passed, 1 ignored (3 new); clippy
  `--all-targets -D warnings` clean; rustfmt clean on `git.rs`; frontend 36;
  version 20; e2e 32. Not verified: macOS itself. The alias it exercises cannot be
  reproduced on this Windows machine, so the fix is unit-tested against a
  filesystem alias here and the macOS job has not been observed — it needs a push.

## 2.45.0

An agent run and the editor could write the same file at the same time, and the
last write won. The agent's side already refused a file another run held; the
editor now asks the same question, in the same ledger.

- `filelock.rs` gained `claimed_by` and `command_claim`: which live run, if any,
  holds a path — or the whole checkout, while it is running a command there.
- Saving, creating, renaming and deleting from the IDE refuse with `409` and a
  message naming the run while that run claims the path (or the folder). A
  refusal writes nothing, so the run's work and the person's work cannot
  interleave silently.
- The reverse direction is unchanged and now has a matching failure: a run's
  `write` is refused when a file changed on disk since that run read it, so a save
  during a run fails loudly on both sides instead of quietly on one.
- No frontend change was needed: the editor already shows the server's message in
  a toast, and the approval retry only fires on `needs_approval`, which a lease
  refusal is not.
- Version 2.45.0 across `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- Verified: `cargo test --locked` 540 passed, 1 ignored (1 new); clippy
  `--all-targets -D warnings` clean; frontend 36; version 20; e2e 32; and a live
  headless run where a lease row written straight into the database made a save
  answer `409` naming that run with the file unchanged, the command claim did the
  same for any file in the folder, and removing the row let the save through.
- Not covered: a program outside Studio writing the file is still invisible to the
  ledger — the fingerprint check catches it afterwards, not before.

## 2.44.0

A portable install — the tarball and the zip — had no way off a computer. Studio
can now remove itself: the program folder, the settings and the data, each named
before anything is touched.

- `app/src/uninstall.rs` (`GET /api/uninstall/preview`, `POST /api/uninstall`)
  reports what would go — program folder, settings folder and data folder, each
  with file and byte counts — and removes it on request.
- Guarded three ways, because this is the one operation that destroys data: the
  call has to carry `approve: true`, it has to echo the data folder's own path as
  its confirmation, and it answers to an `uninstall` permission action of its own
  (separate from `update`, so a rule saved to make upgrades painless is never a
  standing grant to delete projects; default `ask`, deny answers 403).
- It refuses to touch anything that does not look like a portable install: the
  binary beside its static tree, and no `Cargo.toml`, `.git` or `target/` — a
  source checkout or a `cargo build` output is never deleted, and a refusal
  changes nothing. The program folder goes first, so a refusal can never leave a
  working install without its data.
- Either tree can be kept: "keep my chats, projects and extensions" and "keep my
  settings and keys" map to `keepData`/`keepConfig`, and the reply says per tree
  whether it was removed, kept or already absent.
- Windows cannot delete a running program, so the binary is renamed aside
  (`<stem>.uninstalling-<pid>`), everything beside it goes, and the folder is
  reported as `pending` with its exact path rather than claimed removed; the
  reply also carries `complete`, so a partial result is never dressed up.
- Settings → About gained an Uninstall card: it prints the paths and counts, the
  two keep boxes, and a confirmation dialog naming the data folder. The button is
  disabled when the program folder is not removable (a checkout, a
  package-managed install).
- Version 2.44.0 across `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- Verified: `cargo test --locked` 539 passed, 1 ignored (4 new); clippy
  `--all-targets -D warnings` clean; `rustfmt --check` clean on the new file;
  frontend 36 (3 new); version 20; e2e 32; and a live run against a staged
  portable copy: the preview named 13 program files, 4 data files and 1 settings
  file, a wrong confirmation answered 400, the right one without approval
  answered 409 with every file still in place, and the approved call removed the
  data and settings folders while reporting the program folder as pending with
  only the renamed binary left in it.
- Not covered: the pending folder's deletion after the window closes (Windows
  holds the file until the process exits, and this does not pretend to schedule
  that), and a run on Linux/macOS, where the program folder is removed directly.

## 2.43.0

Windows gets an installer: one product in Add/Remove Programs with a version, a
Start Menu entry and an uninstall.

- `app/packaging/windows/installer.wxs` is a per-user WiX package — the app writes
  only under the user's own profile, so asking for elevation would buy nothing —
  that harvests the staged release tree (binary, the whole `static` tree, readme,
  license), adds a Start Menu shortcut and registers a major upgrade, so a newer
  version replaces the installed one instead of sitting beside it.
- `scripts/ci/build-windows-msi.sh` validates the stage (a `free-ai-studio.exe`,
  `static/index.html`, a usable version), builds
  `free-ai-suite_<version>_windows-x86_64.msi` against a stable UpgradeCode, and
  proves the payload with an administrative install before anyone runs it.
- WiX 7 refuses to build without accepting a paid maintenance-fee EULA, so the
  toolchain is pinned to WiX 5 (MIT) from NuGet; the packager finds it under
  `.tmp/wix5` or takes `WIX_EXE`. Nothing in the repo accepts a licence on the
  user's behalf.
- `smoke-package.sh` gained a `.msi` branch: it administrative-installs the
  package, finds the binary where the package put it, boots it headless against a
  scratch home, serves `/`, and compares the version it reports with
  `app/Cargo.toml`. The Windows release job builds and smokes the MSI beside the
  portable zip and attaches both.
- Two real failures were found while doing this and fixed at the cause: WiX
  resolves a bind path against the `.wxs` file's directory, not the working
  directory (the stage is now passed as an absolute Windows path), and Git Bash
  rewrites `/a` and `/qn` into paths, which makes msiexec answer 103 and write no
  log at all (the scripts now run it with `MSYS_NO_PATHCONV=1`). `msiexec`'s own
  verbose log is printed on failure instead of "could not unpack".
- `scripts/ci/test-installer.sh` covers the payload (binary, UI beside it, a
  nested skill file), the product name, three refusals and a rebuild; it runs in
  `desktop-ci.yml`'s Windows job with WiX installed from NuGet.
- Checks: `cargo test --locked` 535 passed, 1 ignored; clippy `--all-targets -D
  warnings` clean; `test-installer.sh` 11 passed against real WiX 5.0.2 on this
  machine; `test-bundle.sh` 13; `test-version.sh` 20; frontend 33; e2e 32. The
  2.43.0 MSI built from a release tree was smoked here: administrative install,
  binary at `LocalApp\Programs\Sarthi Studio`, UI beside it, ready on a free
  port, HTML served, version 2.43.0.
- Not covered: a real interactive install (double-click through the wizard, Start
  Menu launch, uninstall from Add/Remove Programs) was not performed; nothing is
  code-signed, so SmartScreen warns on first run; the macOS bundle is unsigned
  too.

## 2.42.0

macOS ships as a real application bundle, and the binary finds its UI inside it.

- `scripts/ci/bundle-macos-app.sh` turns the staged release tree into
  `Sarthi Studio.app`: the binary in `Contents/MacOS`, the whole UI in
  `Contents/Resources/static`, an `Info.plist` carrying the staged version, the
  bundle id `com.saarthi.freeaistudio`, `CFBundleExecutable`, the product name and
  `APPL` as the package type, plus `PkgInfo`. It is archived as
  `free-ai-suite_<version>_macos-universal.app.zip` with forward-slash paths, so
  the archive unpacks the same way everywhere.
- The packager checks what it assembled before zipping — the executable bit, the
  UI, and that the plist carries the version and the executable's name — and
  refuses a stage with no binary, a static tree without `index.html`, or an
  unusable version. A rebuild replaces the previous bundle instead of merging
  into it, so no file from an older release rides along.
- `static_dir_candidates` gained the bundle layout: `<exe>/../Resources/static` is
  tried after the installed system layout and before a `static/` beside the
  binary, so one binary serves the .deb/AppImage tree, the portable tarball and
  the .app. The source tree stays the last resort for `cargo run`.
- `smoke-package.sh` now accepts the bundle: it unpacks the zip, finds the binary
  in `Contents/MacOS`, finds the UI in `Contents/Resources/static`, boots it
  headless against a scratch home, serves `/`, and compares the version it reports
  with `app/Cargo.toml`.
- The macOS release job builds the bundle from the same universal tree as the
  tarball, smokes both, and attaches both to the release; `desktop-ci.yml` runs
  the packager's regression suite on every push.
- Checks: `bash scripts/ci/test-bundle.sh` 13 passed (structure, plist keys and
  values, zip paths, three refusals, rebuild-replaces); `cargo test --locked` 535
  passed, 1 ignored (1 new: the bundle layout resolves before an adjacent
  `static/`); clippy `--all-targets -D warnings` clean; frontend 33; version 20;
  e2e 32. On this machine the bundle was built from the 2.42.0 binary and smoked
  from its own zip: 6 checks, ready on a free port, HTML UI served, version
  2.42.0.
- Not covered: the bundle is unsigned, so Gatekeeper still needs right-click →
  Open on first launch, and there is still no Windows MSI/EXE. The macOS job's
  own run of the new steps has not happened yet — this commit is local.

## 2.41.0

An upgrade can be undone: the build a swap kept can be put back, and a restart
runs it.

- `POST /api/update/rollback` copies the backup recorded in `applied.json` back
  over the installed binary and keeps the build it displaces at
  `<name>.old-<version>`, so a rollback can be rolled forward again without
  downloading anything. The copy is hashed before anything moves, through the same
  `swap_using` path the swap uses, so a restore that lands wrong leaves the
  installed binary exactly as it was.
- It refuses rather than guesses, and every refusal changes nothing: no swap
  recorded, a backup that is gone, a record naming a different binary than the one
  the process runs from (canonical paths compared), and a backup whose name this
  app did not write (`<stem>.old-<version>` in the install folder) all answer 409
  with the reason. `applied.json` is a file on disk, so it is never trusted to
  point a restore at an arbitrary path.
- One case the swap could not have: a rollback before the swap's restart means the
  name the displaced build would take is the very file being restored, so that
  build goes to a timestamped name instead of the restore consuming its own source.
- Gated on `update` like the swap — a saved `deny` is 403, `ask` is 409 with
  `needs_approval`, allow runs it — and the audit records the restore. A later swap
  clears the rollback record, so the panel can never report a restored build and a
  replaced one at the same time.
- `updates/prepared/rollback.json` records the restored and replaced versions, both
  paths and the time; `GET /api/update/prepared` returns it as `rollback`.
  Settings → Privacy & about gained **Restore the previous build** and a line naming
  the build that is back, the one it replaced and where that copy is.
- Checks: `cargo test --locked` 534 passed, 1 ignored (4 new: the previous bytes
  restored with the displaced build kept, a refusal without a record / with a
  missing backup / naming another binary, a foreign name and a backup outside the
  install folder refused with the target untouched, and the record read back through
  the prepared view); clippy `--all-targets -D warnings` clean; `node --check`
  clean; frontend 33 (2 new); version 20; e2e 32.
- Live headless on a scratch home: the prepared view carried `"rollback": null`,
  the restore answered 409 `needs_approval` under the default rule, and with an
  allow rule in force it refused with "No swap is recorded for this build, so there
  is no previous build to restore." and changed nothing.
- Not covered: no MSI/EXE/.app and no uninstaller; the restore itself ran on
  temporary directories, because rolling this repo's own dev binary back would
  replace a dev build.

## 2.40.0

An upgrade no longer leaves the file it replaced lying next to the new one.

- At startup the app looks beside its own binary for `<name>.old-<version>` files
  and removes the ones it wrote, so the install directory does not accumulate a copy
  of every version ever replaced. The version in the name has to be older than the
  build that is running, which means a backup of a *newer* build is kept — someone
  may have rolled forward and kept it deliberately.
- Two things are never touched: the file the process is actually executing (someone
  can roll back by hand and start the backup directly, and deleting it underneath
  them would be the worst possible surprise) and any name that does not parse as a
  version. Keeping a few stray megabytes is cheaper than deleting something this app
  cannot account for.
- A failure is logged as `update-backups-removed` / `update-backups-failed` in
  `studio.log` and never fatal: not being able to tidy a folder is no reason to
  refuse to start, and on Windows a file still held by another process simply stays
  for the next start to try again.
- Checks: `cargo test --locked` 530 passed, 1 ignored (2 new: an older backup removed
  while a newer one, a foreign file and an unparseable name are kept, and the running
  binary never removed even when its own name looks like a backup); clippy
  `--all-targets -D warnings` clean; `node --check` clean; frontend 31; version 20;
  e2e 32.
- Not covered: the cleanup ran against temporary directories, not a real swap-and-
  restart cycle; there is still no MSI/EXE/.app and no uninstaller.

## 2.39.0

A prepared release can now replace the installed binary, with the old one kept
beside it — the first version of this app that can upgrade itself on disk.

- `POST /api/update/swap` copies the prepared binary into the install directory
  under a dot name, hashes the copy against the prepared file, and only then moves
  anything: the installed binary is renamed to `<name>.old-<version>`, the copy
  takes its name, and a failure at that last step restores the backup. A copy that
  lands wrong never reaches the rename, so a mismatch leaves the build exactly as
  it was.
- Renaming is what makes this possible while the app runs: Windows and POSIX both
  allow a running executable to be renamed but not overwritten, so the process
  keeps its image until it restarts. The reply and the panel both say
  `restart_required` and name the backup, because claiming the upgrade is live
  would be false.
- One backup is kept per version: a leftover from an earlier swap is cleared first,
  and if it cannot be removed (still held open) the new backup takes a timestamped
  name instead of the swap failing.
- Gated on `update` through the same helper as the download and the unpack, so a
  saved `deny` is 403, `ask` is 409 with `needs_approval`, and an allow runs it;
  the audit records the swap.
- `updates/prepared/applied.json` records what was replaced, from which version, to
  which, the backup path and the time; `GET /api/update/prepared` returns it as
  `applied`, so a restarted app still says what happened. A record that cannot be
  written is reported as `recorded: false` rather than turning a completed swap into
  an error.
- Settings → Privacy & about gained **Replace this build** and a line that reads
  "…now replaces this build; restart to run it. It replaced … kept at …".
- Checks: `cargo test --locked` 528 passed, 1 ignored (4 new: a swap that keeps the
  previous bytes, a copy that lands wrong and is rolled back with the target
  untouched, a missing prepared binary that changes nothing, and a second swap that
  clears the previous backup); clippy `--all-targets -D warnings` clean; `node
  --check` clean; frontend 31 (2 new); version 20; e2e 32.
- Not covered: the swap is exercised on temporary directories, never on this repo's
  own `target/debug` binary — a live run against the running artifact would replace
  a dev build. There is still no MSI/EXE/.app and no uninstaller, and a failed
  restart leaves the old binary only as the backup file.

## 2.38.0

A verified download can now be unpacked and checked, and the panel says exactly
where the new binary is — while the running build stays untouched.

- The extractor gained gzip. This app's own Linux and macOS artifacts are
  `.tar.gz`, which no extractor here could read, so `Archive::TarGz` now unpacks
  through the same escape-proof `untar` that the zstd tarballs already used
  (`unpack_in` refuses absolute paths and `..`). One new dependency, `flate2`, on
  its pure-Rust backend so no C toolchain joins the build.
- `POST /api/update/prepare` re-checks the staged file's sha256 before touching it
  — the staged view compares sizes only, which notices a deletion and is not
  enough to trust the bytes again — and refuses a non-archive name, a version that
  cannot be a folder name, and a file whose bytes changed. It then unpacks into
  `updates/prepared/<version>/` and records `prepared.json`: version, asset,
  sha256, root, the binary's relative path, the file count and the time.
- An archive that unpacks without a `free-ai-studio`/`sarthi` binary is removed
  and refused, because a tree that cannot start is not a prepared update. The
  search is bounded (depth 6, 5,000 entries) so a hostile archive cannot make the
  app walk forever.
- Gated on `update` exactly like the download — saved `deny` is 403, `ask` is 409
  with `needs_approval`, allow runs it — and audited. Staging and preparing now
  share one gate helper, so the two cannot drift apart.
- `GET /api/update/prepared` reports the record plus `present` and
  `binary_exists`; `POST /api/update/prepared/discard` removes the unpacked tree
  and its record and is never gated, because it only deletes files this app wrote.
- Settings → Privacy & about gained **Prepare update** and **Remove prepared
  update**, with a line naming the folder and the binary and saying that this
  running build is unchanged.
- Checks: `cargo test --locked` 524 passed, 1 ignored (6 new: nothing staged, a
  changed file, a real zip whose binary is found, an archive with no binary
  cleaned up, a version used as a folder name refused, and a real `.tar.gz` built
  inside the test); clippy `--all-targets -D warnings` clean; `node --check`
  clean; frontend 29 (2 new); version 20; e2e 32.
- Not covered: nothing swaps the running binary, there is no MSI/EXE/.app and no
  uninstaller, and an unpacked tree is only removed when the user asks or a new
  preparation replaces it.

## 2.37.0

An available update can now be downloaded and verified — and deliberately not
installed, because replacing a running binary belongs to the installer work.

- `POST /api/update/stage` re-reads the newest release and refuses when this
  build is already newest (409), when the platform has no asset (409), or when
  the download URL is not `https` (409). Nothing is guessed from a redirect.
- The expected hash comes from the release itself: GitHub's own asset `digest`
  when it is a valid sha256, otherwise the entry for that asset in the release's
  `SHA256SUMS.txt`. A release that publishes neither is refused — an
  unverifiable download is never staged.
- The bytes go through the existing `installer::download_verified`, which writes a
  `.part` file, hashes it while streaming, and only renames it once the hash
  matches. A failed attempt therefore leaves the previously staged copy and its
  manifest exactly as they were instead of replacing a good file with a broken
  one; a successful one removes the stale candidate first and writes
  `updates/staged.json` (version, asset, sha256, bytes, time, release URL).
- Gated on the `update` action the way a router install is: a saved `deny` is
  403, `ask` answers 409 with `needs_approval` so the panel asks once and retries
  with `approve: true`, and a saved allow runs it. Allowances and refusals land in
  the permission audit.
- `GET /api/update/staged` reports what is waiting plus whether the file is
  still present and still the size it was verified at (it does not re-hash a
  100 MB archive on every read). `POST /api/update/staged/discard` removes the
  file and its record — a file this app wrote itself, so it stays possible even
  when `update` is denied.
- Settings → Privacy & about gained **Download and stage** (shown only when the
  check found a newer release with an asset for this machine) and **Discard
  staged update**, with a line that says the file is verified but not installed.
- Checks: `cargo test --locked` 518 passed, 1 ignored (5 new, including a local
  HTTP server proving a verified download stages and a hash mismatch leaves the
  previous copy untouched); clippy `--all-targets -D warnings` clean; `node
  --check` clean; frontend 27 (2 new); version 20; e2e 32.
- Not covered: nothing applies a staged update, there is no MSI/EXE/.app and no
  uninstaller, and the staging folder is only cleaned when a new download
  succeeds or the user discards it.

## 2.36.0

Studio can now say whether a newer release exists, without downloading or
installing anything.

- `app/src/update.rs` reads the newest published release
  (`GET https://api.github.com/repos/sairajKhilariArch/Saarthi-Studio/releases/latest`)
  with a 15-second timeout, a `user-agent` and a 512 KiB body cap, and reports it
  against this build: `current`, `latest`, `update_available`, the platform's
  `asset`, `checksum_url`, `other_assets`, and the release name, URL, date and
  notes clipped to 4,000 characters.
- Versions compare numerically, not as strings: 2.10.0 is newer than 2.9.0, a `v`
  prefix is accepted, a pre-release suffix is ignored, and a tag that is not a
  version counts as 0.0.0 so it can never claim to be newer than what is
  installed.
- The asset is matched on the exact token the release workflow writes
  (`_windows-x86_64.zip`, `_linux-x86_64.tar.gz`, `_macos-universal.tar.gz`). A
  target with no published artifact gets `asset: null` plus the release's other
  asset names, instead of offering a binary that would not run.
- `GET /api/update/check` answers 502 with the reason when the API is unreachable,
  non-2xx or not JSON. It writes nothing, downloads nothing and installs nothing
  (`installs_nothing: true` in the payload), so it needs no permission prompt;
  `Action::Update` stays the gate for an install step that does not exist yet.
- `GET /api/health` now reports the running version, and Settings → Privacy &
  about gained an Updates card: the build number, Check for updates, and a status
  line that distinguishes "up to date", "newer release with this asset and its
  size", "newer release but no build for this platform" and "the check failed".
- Checks: `cargo test --locked` 513 passed, 1 ignored (6 new); frontend 25 (4
  new); clippy `--all-targets -D warnings` clean; `node --check` clean; version
  20; e2e 32. Live: the route answered HTTP 200 against the real release API and
  reported `update_available: false` — the newest published release (v1.71.0) is
  older than this 2.35.0 build, which is exactly what numeric comparison should
  say — and in the real page the About card showed `2.35.0` and the button turned
  "Not checked yet." into "You are up to date (2.35.0)."
- Not covered: nothing downloads or installs yet, there is no MSI/EXE/.app and no
  uninstaller, and the check is manual — nothing runs it at boot or on a timer.

## 2.35.0

The database now has a version, a copy taken before it is touched, and a way back.

- `PRAGMA user_version` records the schema version (`SCHEMA_VERSION` = 1 in the
  new `app/src/schema.rs`), so the version travels with the data instead of
  living in whichever binary happens to open it.
- A database written by a **newer** build is refused before anything runs, with
  both versions named: "The database at … was written by a newer Sarthi Studio
  (schema 2); this build understands schema 1. Update the app instead of opening
  that database with an older build." The file is left byte for byte untouched,
  which is the guarantee that makes downgrading safe.
- An existing database is copied first with `VACUUM INTO` — a consistent
  snapshot, not a copy of a file that may be mid-write — and the copy is opened,
  passes `quick_check`, and must still report the version it was taken from
  before a single migration runs. A copy that fails the check is deleted and the
  upgrade stops with nothing changed.
- Migrations no longer discard their errors. Every `let _ = migrate_…` and both
  `println!("… tables: {error}")` arms became propagated failures with the step
  named (`conversation projects`, `permissions`, `agent run tables`, …), because
  a half-migrated database that boots cleanly is the failure mode that loses
  data quietly.
- A migration that fails restores the copy — through a staging name, so an
  interrupted restore cannot leave a truncated file at the real path — drops the
  `-wal`/`-shm`/`-journal` sidecars that belonged to the replaced file, leaves
  the version where it was so the next boot retries, and reports both the error
  and where the copy is. If the restore itself fails, the message says so and
  names the copy rather than claiming success.
- Boot records what happened in `studio.log`: `database-created` on a first run,
  `database-upgraded` with the copy's path on an upgrade. `/api/ready` gained a
  required `schema` check reporting the version.
- This is the first version that writes a version at all, so every existing
  database reports 0, upgrades once, and gets one `history.sqlite3.pre-0.bak`
  beside it. A no-op boot takes no copy and logs nothing.
- Checks: `cargo test --locked` 507 passed, 1 ignored (8 new: a fresh database,
  an upgrade with a verified copy, a newer schema refused byte for byte, a failed
  upgrade put back, plus four over the copy/restore helpers); clippy
  `--all-targets -D warnings` clean; `node --check` clean; frontend 21; version
  20; e2e 32. Live on a scratch home: two boots left exactly one
  `database-created` line, and `/api/ready` answered `READY True` with
  `('schema', True, 'version 1')`.
- Not covered: there is still no auto-update check, installer or uninstaller,
  and the copy is taken on a schema change only, not before every boot.

## 2.34.0

The harness now breaks the two things it can break honestly: the database file and
the data directory.

- Corrupt database: the real database is replaced with garbage. The process must
  exit rather than boot a half-working server, its log must name the failure
  (`file is not a database`), and nothing may answer on the port. Restoring a
  saved copy must bring the studio back ready — the failure is loud, not silent,
  and recovery is a copy away.
- Unwritable data directory (POSIX only): the data directory is made read-only and
  the `data directory` readiness check must report not-ok, then recover when the
  permission is restored. Windows Git Bash ignores `chmod`, so there the case
  prints `skip` instead of pretending to pass.
- `scripts/ci/test-e2e.sh` is 32 checks on Windows and 34 on a POSIX runner.
- Not covered, and named as such in the docs: a genuinely full disk. Producing
  ENOSPC portably needs a privileged loop/tmpfs mount; the read-only directory
  exercises the same write-failure path but is not the same proof.

## 2.33.0

The two views that build their own DOM now have a render smoke, so a canvas or
IDE shell that throws on load, renders nothing, or loses its children fails in
`test-frontend.sh` instead of in front of a user.

- The harness returns the same stub element for the same id or selector, the way
a browser does, and records every element created; without that a render's
children landed on a throwaway stub.- Workflow canvas: a graph of three nodes renders three cards in dependency
  columns, each node's `title` reaches its card (an untitled node falls back to
  its id), the SVG holds one edge per dependency plus the arrow defs, the canvas
  gets a height, and an empty graph says "No nodes yet" rather than rendering a
  blank box.
- IDE shell: `ide.js` is evaluated in the same context after `app.js`, exactly as
the browser loads it. The smoke asserts it builds its chrome, wraps the app's
`api`, and puts the "No file open" workbench inside the main pane — 2,375 lines
of chrome that previously had no harness at all.
- The harness gained `insertBefore`, `insertAdjacentElement`, `insertAdjacentHTML`
and `getComputedStyle` because the real code paths use them; nothing is stubbed
that a test asserts.
- `test-frontend.sh` is 21 cases, up from 18. This is a render smoke, not a layout
test: no rectangles or pixels are checked, because only a browser can answer
those.

## 2.32.0

Windows is now a first-class CI platform, not only a release target.

- `desktop-ci.yml` gained a `windows` job that builds the desktop binary with its
  default `gui` feature and runs the full Rust suite on `windows-latest` — the
  same commands the macOS job uses. The release pipeline already built Windows
  artifacts, but a source break could reach a tag before any test ran there.
- No extra system packages: WebView2 and the MSVC toolchain ship on the runner,
  and `.cargo/config.toml` scopes mold and `-Z threads` to Linux, so the pinned
  nightly toolchain works unchanged.
- Linux keeps clippy and the end-to-end smoke; adding the platform costs one job
  rather than a duplicated matrix.

## 2.31.0

A release artifact is now proven to run before it is published, not merely to
build.

- `scripts/ci/smoke-package.sh` unpacks an artifact the way a user would — a
  portable tarball, a Windows zip, a `.deb` (`dpkg-deb -x`), an AppImage
  (`--appimage-extract`) or a bare binary — boots the binary inside it headless
  against a scratch home on a free port, waits for `/api/ready`, serves `/`, and
  asks `/api/diagnostics/bundle` for its own version. A missing static tree, a
  binary that cannot boot, or a build that reports the wrong version fails.
- The release workflow runs it on every packaged artifact: the `.deb`, the Linux
  tarball and AppImage, the Windows zip, and the macOS universal tarball (which
  previously smoke-tested only the staged binary before packing it).
- On Windows the script passes a native `FREE_AI_STUDIO_HOME` path (`cygpath -m`),
  because a Git Bash `/tmp` path is not an absolute path to the native binary —
  the same trap the end-to-end harness hit with the workspace call.

## 2.30.0

The end-to-end harness now proves that the two failures a user actually hits end
honestly, instead of proving only that the happy path works.

- Provider outage: the mock provider is killed mid-flow, a run is started anyway,
  and the harness asserts the run reaches `failed` with a recorded error while
  `/api/ready` keeps answering — the studio survives a provider that is gone.
- Killed mid-run: the mock answers slowly, so the harness waits until the run has
  a recorded step and its next model call is in flight, then kills the process.
  After the restart the run must come back `interrupted` with its step log
  readable — the boot sweep, not an in-memory flag, is what marks it.
- `scripts/mock-router.py` gained `--chat-delay SECONDS` (default 0) so a run can
  be observed mid-call, and the harness's `field` helper returns an empty value
  for a missing list index instead of raising.
- `scripts/ci/test-e2e.sh` is 28 checks, up from 19.

## 2.29.0

A run's step log now shows the model call while it is happening, instead of
appearing only once the step is decided.

- `agent_steps` gained a `status` column. Before each model call the loop writes
  a placeholder row (`thinking`, "Waiting for the model to choose the next
  step…", `status: "running"`), so a panel poll during a slow reply shows the
  wait rather than a frozen log.
- The step that resolves the wait deletes the placeholder and takes its `seq`, so
  the finished log still reads as one row per action — no duplicate, no gap.
- A run that stops without a step (cancel, failure, approval) clears any
  placeholder, so a spinner never outlives the call it described.
- `GET /api/runs/:id` returns `status` on every step, and the panel renders a
  running row with a pulsing tag (motion suppressed under
  `prefers-reduced-motion`).

## 2.28.0

Extension tools now refuse a call whose arguments do not match what the tool
declared, before any module is loaded.

- A declared `inputSchema` is enforced on every call — the HTTP route and the
  agent step alike — with the bounded JSON Schema subset MCP calls already use
  (`type`, `properties`, `required`, `enum`/`const`, string and number bounds,
  array items, nested objects and the draft-4 tuple form). A boolean schema means
  what it says (`true` accepts anything, `false` refuses everything), arguments
  must be a JSON object, and a schema this build cannot read is refused rather
  than ignored.
- The refusal names the tool and the failing field (`Tool "echo" rejected the
  arguments: \`text\` is required.`) and costs no fuel: nothing is loaded and no
  run row is written, because no run started. On the route a saved rule is
  checked first, so a denied call reads as denied instead of as a bad argument
  list.
- The prompt catalogue now lists each tool's required argument names, so the
  model sees what a valid call needs; descriptions and types stay out to keep the
  block small.

## 2.27.0

The agent can call an installed extension's tools, and the progress checklist
finally describes the tree it lives in.

- Extension tools reach the tool loop: when a run's prompt is built, the declared
  tools of every valid, runnable extension are read from disk and advertised in a
  labelled system message (`extension.tool — description`), capped like the MCP
  catalogue. With nothing installed, no `extension` action is advertised at all,
  so the model is never offered a call that would fail.
- New step `{"action":"extension","extension":"<id>","tool":"<name>","arguments":{}}`
  (`extension-tool` is accepted too, and missing arguments mean `{}`).
- Each call is a real sandboxed run, not a shortcut: the same fuel/memory/wall
  budgets, the same declared grants for the workspace and the extension's own
  data folder, the same one-run-per-extension rule and cancellation boundary, a
  row in `extension_runs` and an audit row for every host call. The module's
  `env.result` becomes the step output, and a tool that reports nothing yields
  `null`.
- Approval: the step is gated by the `extension_tool` action on the target
  `extension/tool` — the same target the HTTP route uses, so one saved rule
  covers both ways of calling the tool. Without `autoApprove` the call waits for
  approval like a write, and a saved `deny` refuses it.
- Docs reconciled: `PROGRESS.md`'s "Next up" was rewritten against the tree (it
  still called the Phase 17 remainder "the next slice" and Phase 13
  unimplemented), with a note recording what was stale, and `FEATURE-STATUS.md`,
  `MASTER-ROADMAP.md` and `KNOWN-ISSUES.md` now agree with it on what Phase 15
  has left.

## 2.26.0

Extensions can declare tools, and calling one is a real sandboxed run rather
than a line in a panel.

- `tools` in the manifest: up to 16, each with a name, a description (required,
  because the model picks tools by it), an optional `.wasm` entry (default: the
  manifest's own) and an optional `inputSchema`. A manifest declaring tools must
  declare `runtime: "wasm"`, and a tool's module is checked for existence and
  containment exactly like the entry file.
- The sandbox gains a tool ABI: `env.arguments(ptr, cap)` hands the module the
  JSON arguments of the call and `env.result(ptr, len)` reports exactly one
  answer (a second report is refused). Neither needs a permission — they are the
  shape of the call, not access to anything — and a plain run reads no
  arguments.
- `POST /api/extensions/:id/tools/:tool` runs the tool with `{arguments}` and
  answers `{result, reported, run}`; the result is parsed as JSON when it is
  JSON and returned as text otherwise. Arguments are capped at 64 KiB and the
  answer at 1 MiB, the same host cap as any other I/O.
- A call is gated by a new `extension_tool` permission action (default `ask`)
  with the target `extension/tool`. This route cannot prompt, so `ask` refuses
  with instructions to save an allow rule and `deny` refuses by rule; a refusal
  reaches the permission audit, and an allowed call lands in `extension_runs`
  with its host calls audited like any other run.
- Tool calls share the run machinery: one run per extension (409 for a second),
  the same fuel/memory/wall budgets, the same workspace and data grants, and
  `POST /api/extensions/:id/cancel` stops one at its next host call.
- Settings → Extensions lists a folder's tools in its row and their descriptions
  in the row's tooltip.
- Checks: `cargo test --locked` **493 passed, 1 ignored** (four new: the tool ABI
  passing arguments and one result, a plain run reading no arguments with a
  second result refused, the tool declaration rules, and a tool module that must
  exist); clippy `--all-targets -D warnings` clean; `node --check` clean;
  frontend 18; version 20; e2e 19.
- Live headless on a scratch home: the tool was listed with its entry; the call
  answered 403 before any rule and 200 `{"ok":true}` after an `extension_tool`
  allow rule, `exitCode 1`, 11 fuel, with a row in `extension-runs`; an unknown
  tool answered 400 naming it.
- Not yet: the agent's own tool loop does not offer extension tools (its catalog
  and a tool step are still to come), `inputSchema` is passed to a model but not
  enforced on the arguments, and the row's tooltip was not opened in a browser.

## 2.25.0

An extension can now declare which builds it works on and which other
extensions it needs, and the app refuses it by name instead of half-reading it.

- `requiresStudio` is the oldest app version the extension works on (`1.2.3`).
  A manifest naming a newer build stays `valid` — it is a real extension — but
  reports `compatible: false` and a `blockedReason` of "Requires Sarthi Studio
  99.0.0 or newer; this build is 2.25.0." It is neither run nor installed.
  Comparison is numeric per part, so `2.9.0` is older than `2.25.0`.
- `apiVersion` names the manifest schema. Missing means 1, the only version this
  build reads; a higher number is refused at parse with the number named, since
  reading part of an unknown schema is worse than saying it cannot be read. The
  field is reported back, so an installed folder's schema is inspectable.
- `dependencies` lists other extension ids that must already be installed (at
  most 16, deduplicated, no self-reference, ids validated). A missing one is
  reported as `missingDependencies` and blocks the run ("Needs base-tools
  installed first.") but not an install, because the dependency can be added
  afterwards.
- Discovery now reads every manifest before judging any of them, so a dependency
  is checked against the folders that really are installed.
- Settings → Extensions shows `needs newer app` for an incompatible folder — a
  distinct badge with the requirement in its tooltip — instead of calling it
  valid.
- Checks: `cargo test --locked` **489 passed, 1 ignored** (six new: a newer
  requirement is valid-but-incompatible with a numeric floor, numeric version
  ordering, a schema from the future refused, dependency validation and dedupe,
  a missing dependency blocking a run until it is installed, an incompatible
  folder refused at install with nothing left behind); clippy `--all-targets -D
  warnings` clean; `node --check` clean; frontend 18; version 20; e2e 19.
- Live headless on a scratch home: `requiresStudio: "99.0.0"` reported
  `compatible: false, runnable: false` with the sentence above, and both the run
  and the install were refused 400 with it; removing the dependency folder made
  the addon report `missingDependencies: ["base-tools"]` and `runnable: false`.
- Not verified: the `needs newer app` badge was not opened in a browser; the
  endpoint behind it was exercised directly.

## 2.24.0

Every extension run now leaves a row in the database, not only a log line.

- `extension_runs` (id, extension, project, started_at, duration_ms, exit_code,
  fuel_used, host_calls, error), created idempotently with the rest of the
  schema and indexed by start time.
- A row is written for each run that reached the sandbox: successes, traps, fuel
  stops, cancellations, and runs whose module would not load. A failed insert
  never fails the run that already happened, matching the permission audit.
- `GET /api/extension-runs` returns the newest 50. Settings lists them under the
  installed extensions with time, duration, exit code, fuel, host calls and the
  error, so "it did nothing" and "it failed" stay distinguishable after the
  toast is gone.
- Checks: `cargo test --locked` **483 passed, 1 ignored** (one new: history
  round-trips newest-first, including a row with no exit code); clippy
  `-D warnings` clean; `node --check` clean; frontend 18; version 20; e2e 19.
  Live: a valid run (exit 8, 7 fuel, 1 host call) and an unloadable module both
  appear in `/api/extension-runs`, newest first, with the project id and the
  loader error.
- The Settings history list was not opened in a browser; the endpoint it reads
  was exercised directly.

## 2.23.0

A run in flight can now be stopped, and only one run per extension exists at a
time.

- `POST /api/extensions/:id/cancel` sets the run's stop flag; the run ends at its
  next host call and answers 400 `"The run was cancelled."` Rather than pretend
  an interpreter can be interrupted mid-instruction, the limit is stated: a
  module that makes no host calls still ends only at the fuel limit.
- A second run of the same extension while one is in flight answers 409, so a
  cancel names exactly one run and a stuck module cannot pile up blocking
  threads. The entry clears when the request returns.
- A 5-second wall-clock budget is checked at every host boundary, which is the
  part fuel cannot bound (time spent in I/O). Computation stays fuel-bounded and
  deterministic.
- Uninstall now cancels a run in flight before moving the folder (ADR gate 7);
  the module is already loaded, so it stops at its next host call.
- Settings shows `Running…` with a `Cancel` action while a run is in flight,
  instead of a button that silently re-submits.
- Checks: `cargo test --locked` **482 passed, 1 ignored** (two new: a
  pre-cancelled run stops at its first host call, and a zero wall budget is
  enforced); clippy `-D warnings` clean; `node --check` clean; frontend 18;
  version 20; e2e 19. Live: cancel while idle answered `cancelled:false`; a slow
  module cancelled 150 ms into its run answered 400 `"The run was cancelled."`
  with `stop requested` in the log; a second concurrent run answered 409; the
  same run without interference ended at the fuel budget in 0.6 s.
- The UI's Running/Cancel states were not clicked in a browser; the endpoints
  behind them were exercised directly.

## 2.22.0

Extensions can persist state now — in their own folder and nowhere else.

- `env.data_read(name, len, out, cap)` and `env.data_write(name, len, data,
  len)` work inside `<extensions>/<id>/data`, which is created on the first
  approved write. The code folder beside it stays unreachable, as does every
  other extension's folder.
- Names are resolved and contained exactly like workspace paths, so `..`, an
  absolute name and a symlink escape are refused before any byte moves. The
  same saved `fs_read`/`fs_write` rules apply, which means a write still needs
  an allow rule; a read defaults to allow.
- Both functions are linked only when the manifest declares
  `workspace.read`/`workspace.write`, the same gates as the workspace pair — the
  permission vocabulary did not grow.
- Both calls are audited as `data.read`/`data.write` with the extension id and
  counted in the run's `hostCalls`.
- Checks: `cargo test --locked` **480 passed, 1 ignored** (two new: write then
  read its own data with the folder created lazily, and a name that tries to
  leave the data folder); clippy `-D warnings` clean; `node --check` clean;
  frontend 18; version 20; e2e 19. Live: an installed module wrote `notes.txt`
  into its data folder and read the 4 bytes back (`hostCalls: 2`), with both
  calls in the log as `data.write` and `data.read`.
- Still missing: a wall-clock backstop behind fuel, a run history beyond the
  log, and UI cancellation.

## 2.21.0

The sandbox can now touch the workspace — through two host functions, scoped to
the open folder and checked against the same saved permission rules the agent
obeys.

- `env.read(path, len, out, cap)` and `env.write(path, len, data, len)` are the
  whole ABI alongside `env.last_error(ptr, cap)`. A host function is linked only
  when the manifest holds the matching permission, so a module importing one it
  did not declare fails to instantiate with the import named.
- `workspace.read` and `workspace.write` are back in `V1_PERMISSIONS`: a manifest
  declaring either is runnable again. `network`, `terminal.run` and `mcp.call`
  are still refused at run.
- Paths go through `workspace::resolve_inside`, so absolute paths, `..`, symlink
  escapes and the missing-parent case are refused exactly as they are for the
  editor and the agent; writes reuse `write_text_file`, which is atomic and
  size-capped.
- Rules apply as they do for a run: a saved deny refuses the call. An extension
  has no UI to answer a prompt, so a rule that would `ask` is also a refusal,
  with a message naming Settings → Permissions — unattended code never gets an
  approval it could not obtain. Reads default to allow, writes to ask, so a
  write needs a saved allow rule.
- Every call is audited with the extension id: `extensions`/`host` in the
  process log for allows and refusals, and the permission audit table (shown in
  Settings) for refusals. The run response reports `hostCalls`.
- I/O is bounded separately from fuel: at most 64 host calls, 1 MiB per read or
  write, 1 KiB per path. Error codes are stable: `-1` refused, `-2` invalid
  path, `-3` missing, `-4` too large, `-5` not UTF-8 text, `-6` I/O failure.
- Checks: `cargo test --locked` **478 passed, 1 ignored** (six new: read with the
  grant, an import not granted, a path escape, a saved deny, a write needing an
  allow rule, and `last_error`); clippy `-D warnings` clean; `node --check`
  clean; frontend 18; version 20; e2e 19. Live: a handcrafted reader module
  returned the 8 bytes of `hello.txt` (`hostCalls: 1`), and after a saved
  `fs_read` deny on that path the same run returned `-1` with a `warn` host line
  and a permission-audit row naming the extension.
- Still missing: the extension-private data directory, a wall-clock backstop
  behind fuel, and a run history beyond the log line.

## 2.20.0

Extension code can now run — but only inside a sandbox that grants it nothing.

- `app/src/extension_run.rs`: a **wasmi 0.31.2** interpreter (pinned with `=`) with
  an empty linker and no WASI. A module must export `run() -> i32` and import
  nothing; its return value is the exit code. It is bounded by 50,000,000 fuel,
  a 64 MiB memory ceiling, a 100,000-element table limit and an 8 MiB module
  size cap.
- Fuel is deterministic cancellation: the same module stops at the same
  instruction on every machine, which a wall-clock kill cannot promise. A trap,
  an import, a missing export and an exhausted budget each report a distinct
  message.
- `POST /api/extensions/:id/run` executes a runnable extension on a blocking
  thread and answers `{exitCode, fuelUsed, elapsedMs}`. Each outcome is written
  to the oplog under the `extensions` area.
- `V1_PERMISSIONS` is now empty: the first slice links no host functions at all,
  so a manifest asking for `workspace.read`, `workspace.write`, `network`,
  `terminal.run` or `mcp.call` is refused at run (400) with the permission named,
  never run with a narrower sandbox than it asked for.
- Settings shows a Run action only for `runnable: true`, and its toast reports
  the exit code, elapsed time and fuel used.
- ADR 0001 gains an implementation note: wasmi instead of Wasmtime (the crate
  pins `rust-version = "1.80"`), compute-only, and fuel in place of a wall-clock
  cap.
- Checks: `cargo test --locked` **472 passed, 1 ignored** (four new: exit code,
  fuel stopping a spin loop, imports and a missing export refused, the memory
  cap); clippy `-D warnings` clean; `node --check` clean; frontend 18; version
  20; e2e 19. Live: a permissionless wasm extension installed, reported
  `runnable: true`, ran and returned exit code 7 with 3 fuel used and one oplog
  line; a manifest requesting `workspace.read` was refused with 400 naming the
  permission.
- No workspace, network or terminal capability exists yet. Host functions are
  the next ADR gate, with their own path-scoping and audit tests.

## 2.19.0

ADR 0001's first gate is implemented: an extension can be gated before any code
would be loaded.

- The manifest accepts `runtime`; only `"wasm"` is valid, and a wasm extension
  needs an `entry` ending in `.wasm`. A manifest without `runtime` still
  installs and keeps its old meaning: not runnable.
- Discovery reports `runtime`, `runnable` and `blockedReason` per folder. The
  reason names the first obstacle: no runtime declared, a permission the v1
  runner does not provide (`network`, `terminal.run`, `mcp.call`), or — today —
  that the WASM runner itself is not implemented.
- `POST /api/extensions/:id/run` refuses with that reason (400), and with 501
  only for a manifest that passes every gate, so a future runner has one place
  to attach. No code is loaded.
- Settings shows the block reason on each row and offers no run action.
- Checks: `cargo test --locked` **468 passed, 1 ignored** (three new: wasm entry
  rules, gating reasons, inspection metadata); clippy `-D warnings` clean;
  `node --check` clean; frontend 18 passed; e2e 19 passed. Live: a wasm manifest
  installed and reported the runner reason, a legacy `main.js` reported no
  runtime, and both run attempts were refused with 400.
- Everything is still install-and-inspect only. Removing the "runner not
  implemented" branch is the first commit allowed to execute extension code.

## 2.18.0

No code: this release records the decision that governs extension execution
before any loader is written.

- `docs/adr/0001-extension-execution-sandbox.md`: execution is out-of-process
  and WebAssembly-first, with WASI disabled and every capability reached through
  a host function that checks the manifest permission and the existing
  permission rules at call time. Deny by default.
- No in-process JS, no Node child process with an ambient environment, no
  Docker-only sandbox. `terminal.run`, `mcp.call` and `network` are not offered
  in v1; a manifest that requires them installs but cannot run, with a stated
  reason.
- Existing `entry: "main.js"` extensions stay installable and are reported as
  not runnable until a sandboxed JS runtime is chosen. Installing grants no
  capability.
- The ADR carries the implementation gates and required tests (path scoping,
  permission refusal, fuel/memory limits, audit trail, UI refusal reason,
  stopping an instance before uninstall).
- Checks: `scripts/ci/test-version.sh` 20 passed; documentation only, no
  behaviour change.

## 2.17.0

Extensions can be installed and removed from the app now.

- Install copies a folder into the extensions directory through a staging
  folder: a bad manifest, a missing entry file or a symlink leaves nothing
  behind. The source folder name must equal the manifest id, the manifest is
  validated before any copy, and the installed copy is re-read so the entry and
  canonical containment checks apply. The copy is capped at 500 files / 16 MiB.
- Uninstall validates the id, confirms the canonical folder is inside the
  extensions directory, and moves it to `.removed/` instead of deleting it.
  Discovery skips hidden folders, so a removed extension stops reporting.
- Settings → Extensions gains a folder-path field with Install, and a Remove
  action on each row.
- Checks: `cargo test --locked` **465 passed, 1 ignored** (three new: install and
  duplicate refusal, rollback on a bad manifest and a missing entry, uninstall
  moving aside and refusing a traversal id); clippy `-D warnings` clean;
  `node --check` clean; frontend 18 passed; e2e 19 passed. Live: install → valid
  → remove → empty, with the folder moved to `.removed`.
- The new panel controls were not clicked in a browser; the endpoints were
  exercised with `curl`.

## 2.16.0

The manifest API had no visible surface; Settings → Extensions is now one.

- The panel shows the directory extensions are read from, then one row per
  folder: name and version (or the folder id when invalid), the description or
  the validation error, and a valid/invalid badge. Refresh re-reads the folder
  without a restart.
- It is a read-only view over the same `GET /api/extensions` the API exposes.
  There is no install, uninstall or run action, because none of those exists yet.
- Checks: `node --check` clean; `bash scripts/ci/test-frontend.sh` 18 passed;
  `cargo test --locked` 462 passed, 1 ignored (backend unchanged); clippy
  `-D warnings` clean; e2e 19 passed.
- The panel was not opened in a browser, so its rendering is unverified; the
  endpoint behind it was exercised live in 2.15.0.

## 2.15.0

Phase 15 starts with an extension manifest — discovered and validated, not
executed.

- `app/src/extensions.rs` scans `<data>/extensions/*/manifest.json` and reports
  every folder with `valid` plus an `error` when it is not usable.
- Manifest fields: `id` (must equal the folder name), `name`, `version` (three
  numeric parts), `description`, `author`, optional `entry`, and `permissions`
  from a closed list (`workspace.read`, `workspace.write`, `terminal.run`,
  `network`, `mcp.call`). An unknown permission is refused, not ignored.
- `entry` must be a relative path inside the folder, free of `..`, and the file
  must exist; canonical paths are compared so a symlink cannot escape either.
- A missing manifest, invalid JSON, a 64 KiB size cap and an id/folder mismatch
  each produce a stated reason while the other folders still report.
- `GET /api/extensions` returns the directory and the entries.
- Checks: `cargo test --locked` **462 passed, 1 ignored** (three new:
  normalisation and permission de-duplication, rule refusals, folder inspection
  that keeps the valid entries); clippy `-D warnings` clean. Live headless: a
  valid folder, an unknown-permission folder and a folder with no manifest were
  reported as valid, invalid and invalid with reasons.
- Deliberately not shipped: execution, a sandbox, custom tools, provider
  adapters and UI extensions. The manifest is where this slice stops, so no
  sandbox claim is implied by it.

## 2.14.0

The workflow canvas now has tests, at the layer a browser-free harness can
honestly check.

- `app/static/tests/harness.js` gains `createElementNS`, so the canvas code can
  build SVG nodes in the inert DOM the tests already use.
- Six new cases in `app.test.js`: layout places dependents one column to the
  right and siblings together, a cyclic graph still terminates, renaming a node
  rewrites the dependencies that point at it, deleting one removes those edges,
  adding a node yields a unique id plus the kind's defaults, and a dependency
  keeps `when: "failed"` through an edit until it is removed.
- Checks: `bash scripts/ci/test-frontend.sh` **18 passed**; `node --check` clean;
  `cargo test --locked` 459 passed, 1 ignored (the backend is unchanged).
- Still not tested: rendering. No assertion inspects positions, edges or the
  inspector as drawn — that needs a browser, and claiming it here would be a lie.

## 2.13.0

Workflows can leave the machine: a graph exports to a portable JSON document and
imports back as a new workflow.

- `GET /api/workflows/:id/export` returns `{format: "sarthi.workflow", version:
  1, name, prompt, skill, everyMinutes, nodes}` as a download named after the
  workflow (lowercase ascii, single dashes).
- `POST /api/workflows/import` accepts that document (or `{workflow: …}`) and
  validates it with the same rules as save — ids, kinds, dependencies, cycles,
  the node cap and the schedule rule — refusing an invalid graph without writing
  anything.
- Imports always get a new id and a unique name (`Review (2)`), so importing
  twice never overwrites the first copy. A scheduled import starts a fresh
  window instead of firing immediately.
- The Automation list gains Export per workflow and an Import from file button;
  both report failures as a toast.
- Checks: `cargo test --locked` **459 passed, 1 ignored** (two new: import
  validation/normalisation and safe unique export names); clippy `-D warnings`
  clean; `node --check` clean. Live headless: a saved graph exported with its
  format, version and nodes intact, then imported as a second workflow with the
  same graph and a distinct name; an unknown node kind was refused with 400.

## 2.12.0

Phase 13 gains a canvas: the workflow graph can be built and edited visually
instead of only as JSON.

- Nodes render as cards in dependency columns; edges are curves in an SVG
  overlay, dashed when the edge asks for a failure. The layout is computed from
  the graph, so a cycle cannot spin it (the server refuses cycles anyway).
- Add a node by kind from the toolbar; select one to edit its id, title, kind,
  prompt, profile, model, retries, auto-approve, condition `expect` or task note,
  and tick its dependencies with a per-edge `succeeded`/`failed` choice.
- Renaming a node rewrites every dependency that pointed at it; deleting one
  removes the edges to it.
- The JSON field stays as the advanced view and is kept in sync in both
  directions. The server remains the only validator: canvas edits still go
  through the same save route.
- Checks: `node --check app/static/app.js` clean; `cargo test --locked` **457
  passed, 1 ignored** (the backend is unchanged); clippy `-D warnings` clean;
  e2e 19 passed, so the app still boots and serves the panel.
- Limit stated plainly: the canvas has no case in the frontend harness and was
  not opened in a browser in this change, so its rendering and interaction are
  unverified.

## 2.11.0

An interrupted workflow run can be resumed instead of written off.

- `POST /api/workflow-runs/:id/resume` accepts a run whose status is
  `interrupted` — the state the boot sweep gives a run whose process closed.
- Resume is a re-entry, not a replay: steps that already ended (succeeded,
  failed, skipped, cancelled, approved, rejected) keep their result; anything
  pending, running, waiting or interrupted returns to `pending`, and the same
  engine loop picks it up. The run goes back to `running` and its error clears.
- Refused with a reason when the run is not interrupted, the workflow was
  deleted, or the graph no longer has the same node ids as the stored steps — an
  edited graph is a new run, not a resume.
- A resumed agent node starts a fresh agent run: the original did not survive
  the process either. Its finished siblings are not repeated.
- The run list offers Resume on interrupted runs.
- Checks: `cargo test --locked` **457 passed, 1 ignored** (one new case for the
  resume boundary: only interrupted runs qualify, only finished steps carry
  over); clippy `--all-targets -D warnings` clean; `node --check` clean. Live
  headless: an approval-only workflow parked at `waiting`, the app was killed and
  restarted (the boot sweep marked the run `interrupted`), resume returned it to
  `waiting`, and approving finished it `succeeded`.

## 2.10.0

Workflows can run on a schedule, with the timing recorded rather than held in
memory.

- A workflow carries `everyMinutes` (0 = manual, otherwise 5–10080) and
  `scheduleSince`. Saving keeps the window when the interval is unchanged and
  starts a fresh one when it changes, so saving a workflow never makes it due
  immediately.
- A scheduler task wakes every 30 seconds, loads the workflows and starts a run
  once `max(last run, window start) + interval` has passed. A workflow with a
  live `running`/`waiting` run is skipped rather than stacked, and the window
  advances when the run starts.
- The anchor is the later of the last run and the window start, so a run longer
  than its interval does not queue missed intervals, and a restart cannot fire
  early.
- `GET /api/workflows` now includes `lastRunAt` and `nextRunAt` (unix seconds,
  `null` when manual); the Automation row shows the interval and the next run.
- Validation refuses an interval outside 0/5–10080 and a schedule with no
  nodes.
- Checks: `cargo test --locked` **456 passed, 1 ignored** (two new: due-time
  anchoring and interval bounds); clippy `--all-targets -D warnings` clean;
  `node --check` clean. Live headless run: a workflow saved with
  `everyMinutes: 5` and a past window was picked up by the scheduler within one
  tick, ran its agent node to `succeeded`, wrote the file, and advanced its
  window.
- Limit: scheduling is per process. Two app instances sharing one config could
  both start a run; the desktop app is single-instance by design, and a run's
  own `created_at` still prevents a tight loop.

## 2.9.0

Phase 13 stops being a saved prompt: a workflow is a validated graph, and a
graph runs as a persisted workflow run.

- `app/src/workflow.rs` holds the model, validation, storage and engine. A
  workflow is still `workflows.json` beside the config, so saved prompts keep
  working (no nodes = the old behaviour).
- **Nodes:** `agent` starts a real agent run — the same `agent_runs` loop the
  Runs panel drives — and waits for it, including at its approval gate;
  `approval` parks the workflow until a person answers it; `condition` asserts
  what its dependencies ended as; `task` records a note as an explicit step.
- **Edges:** `dependsOn` entries are an id ("after it succeeded") or
  `{id, when: "failed"}` for a repair branch. A dependent whose edge condition
  is not met is marked `skipped` instead of blocking the graph.
- **Validation before any run row exists:** duplicate or malformed ids, unknown
  kinds, unknown profiles, missing dependencies, self references, cycles, an
  agent node without a prompt, a condition without dependencies, bad `when` and
  `expect` values, and more than 50 nodes are all refused with the node named.
- **Persistence:** `workflow_runs` and `workflow_steps` in SQLite. Every step
  has a stored status, output, start/end and the agent run it started, so the
  history is the execution record, not a reconstruction. A run that was live
  when the app closed is marked `interrupted` at boot.
- **Routes:** `POST /api/workflows/:id/run`, `GET /api/workflow-runs`,
  `GET /api/workflow-runs/:id`, `POST /api/workflow-runs/:id/approve`,
  `POST /api/workflow-runs/:id/cancel`, plus the existing save/list/delete with
  a `nodes` array.
- **Engine:** one spawned task walks the graph in dependency order, retries a
  failed agent node up to its `retries` (0–3), stops on cancel, and treats a
  failure as handled only when a `when: "failed"` dependent actually succeeded.
- **UI:** Automation gains a nodes editor (JSON, validated by the server with
  the error shown), `Start` for graphs, `Load prompt` for the old behaviour,
  and a live run history with Approve/Reject and Cancel.
- Checks: `cargo test --locked` **454 passed, 1 ignored** (six new workflow
  cases); clippy `--all-targets -D warnings` clean; `node --check` clean. Live
  headless smoke against the mock provider: a one-node agent graph ran to
  `succeeded`, the agent's file was on disk, and `/api/workflow-runs` showed
  `1/1` steps.
- Remaining in Phase 13, stated rather than implied: a visual canvas (the
  editor is JSON today), scheduled and recurring runs, workflow import/export,
  and resume after an app close. Cancelling a workflow stops the graph; a child
  agent run already in flight is not forcibly stopped.

## 2.8.0

The end-to-end run grows the half it was missing: a real agent task that stops
for approval, executes the write, is verified, and survives a restart.

- `scripts/ci/test-e2e.sh` now opens a scratch workspace folder, creates a run
  with `allowWrites`, polls the run's own detail route until it parks at
  `awaiting_approval`, approves the pending write, polls to `succeeded`, asserts
  the file on disk says what the model wrote, asserts the step log is
  `write, verify, done`, then restarts the process and re-reads the run and its
  steps from the database. 19 checks, up from 11.
- `scripts/mock-router.py` scripts just that goal: the first agent call answers
  a `write` action, the next a `done`, and the verifier prompt an `APPROVE`. Any
  other turn keeps the plain reply the UI and console smokes expect.
- The fix the first attempt found: the binary is native, so a Git Bash
  `/tmp/...` path is not absolute to it. The workspace call converts the scratch
  path with `cygpath -m` (forward slashes, so no JSON escaping), and a folder
  that will not open now fails the flow instead of cascading into the checks
  that depend on it.
- `bash scripts/ci/test-e2e.sh`: **19 passed, 0 failed** (run twice, exit 0);
  `cargo test --locked`: 448 passed, 1 ignored; `bash -n` and `python -m
  py_compile` clean.
- Still open in the same flow: a provider-failure case, and install smokes over
  real release artifacts.

## 2.7.0

Closes the last named gap in Phase 7: an MCP tool call is now checked against
the tool's own `inputSchema`, not just its `required` list.

- `validate_value` in `app/src/mcp.rs` walks the schema and the arguments
  together — `type` (a name or a list, with `1.0` counting as an integer),
  `enum`/`const`, nested `properties`, `items` (one schema or the draft-4 tuple
  form), `additionalProperties` (`false` or a schema), `minLength`/`maxLength`,
  `minimum`/`maximum`/`exclusiveMinimum`/`exclusiveMaximum`, `multipleOf`,
  `minItems`/`maxItems`/`uniqueItems`, `minProperties`/`maxProperties`,
  `allOf`/`anyOf`/`oneOf`/`not`, boolean schemas and local `$ref`s
  (`#/$defs/…`, `#/definitions/…`).
- The error names the field by path and says what it expected, in the same plain
  wording as before: `Argument "options.timeout" must be integer.`,
  `Argument "tags[1]" must be string.`, `Missing required argument
  "options.name".` The agent loop already feeds that text back to the model to
  repair the call.
- Deliberately bounded: a recursive schema stops at depth 16; `pattern`,
  `format`, `propertyNames` and remote references are left to the server (it is
  the authority on its own tool); `additionalProperties: false` is not applied
  beside `patternProperties`, because refusing a name a pattern would allow
  without a regex engine would be worse than the server's own answer. A schema
  that is neither an object nor a boolean is refused at call time (`declares an
  input schema … this client cannot read`), which is the phase's "invalid
  schemas are rejected" half.
- `cargo test --locked`: **448 passed, 1 ignored** (5 new cases over the shared
  validator). Clippy `--all-targets -D warnings` clean, rustfmt clean. No new
  dependency and no frontend change; validation errors already surfaced through
  the existing 400 and agent-step text paths.
- Phase 7 is closed against the specification's rows. The route was not driven
  against a live MCP server in this slice: the validator the route and the agent
  loop share is covered by unit tests, and the call-path check is a three-line
  guard.

## 2.6.0

Phase 16 lands a process-level end-to-end run: not another in-process test, but
the actual binary, a real config file, a real SQLite database and a restart.

- `scripts/ci/test-e2e.sh` — builds (or reuses) the debug binary, picks two free
  ports, starts `scripts/mock-router.py` as the provider and launches the studio
  headless with `FREE_AI_STUDIO_HOME` pointed at a scratch directory, so the run
  can never read or write the developer's own settings. 11 checks: the mock
  answers; the studio reports ready on the port it was given; the readiness
  payload is well formed; a conversation is created; a chat turn is answered by
  the provider and its text is the stored reply; the turn is stored as two
  messages; the process log recorded the boot; the process is killed and
  restarted on the same home; the conversation is still there, read from disk;
  the database sits at the documented path.
- Failing honestly where it matters: readiness is polled rather than assumed from
  a listening port, the restart waits for the old process to release the port,
  and on Windows the listener's own PID is killed because a shell `kill` can
  leave the native process behind.
- Wired into `desktop-ci.yml` after clippy, and `docs/TESTING-STRATEGY.md`
  documents the command and the ~3 s runtime.
- Still open and stated as such: the agent-approval half of the flow, opening a
  folder, and a provider-failure case inside the same run.

## 2.5.0

Starts Phase 16 with the layer the repo never had: automated tests for the static
UI, using no new dependency and no build step.

- `app/static/tests/harness.js` evaluates the real `app.js` inside a `node:vm`
  context whose DOM records calls and never draws — stub elements, a style object
  that stores custom properties, inert observers, inert timers (a live one kept
  the test process alive) and a `fetch` that answers 503 instead of rejecting, so
  a load-time loader cannot surface as an unhandled rejection. The file is run,
  not copied: the functions under test are the ones the browser runs.
- `app/static/tests/app.test.js` — 12 cases: HTML escaping, the markdown subset
  (including HTML typed inside bold staying inert and a `javascript:` link
  remaining text), byte and time formatting at their boundaries, `basename`/
  `parentPath`, tab keys, and the icon fallback.
- `scripts/ci/test-frontend.sh` runs them through Node's built-in runner and
  refuses an absent or too-old Node, and `desktop-ci.yml` now calls it next to
  the version-regression suite.
- Scope is stated rather than implied: pure helpers only. There is no render
  smoke, and `app/static/ide.js` still has no harness — `docs/TESTING-STRATEGY.md`
  and `KNOWN-ISSUES.md` say so.
- `bash scripts/ci/test-frontend.sh`: 12 passed, 0 failed. `cargo test --locked`:
  443 passed, 1 ignored. Clippy `--all-targets -D warnings` clean.

## 2.4.0

Closes Phase 17: every privileged call can now be matched to the request that
asked for it, and one endpoint says whether the install can work.

- `app/src/trace.rs` assigns an id to every local API request, keeps it in a
  task-local for the handler's lifetime, and echoes it as `x-request-id`. A
  caller's own id is accepted only when it is 8–64 characters of alphanumerics,
  `-`, `_` or `.`; anything else is replaced, because the id is stored and shown.
- `permission_audit` gained a `request_id` column through an in-place migration.
  Old rows keep `''` — they were written before ids existed and guessing one
  would be a fabrication. A terminal open, a Git call and a settings save are now
  tied to the HTTP call that caused them, in the timeline and in the process log.
- `GET /api/ready` answers from cheap local checks only: the database opens and
  answers, the data directory accepts a write (a probe file, removed again),
  provider base URL plus whether a key is saved and which credential store holds
  it, the latest router install outcomes, and MCP servers by health. Only the
  database and the data directory are `required`; the rest change what works,
  not whether the app runs. It performs no network probe — measured state stays
  in the Router view and the Provider health card.
- Settings → Diagnostics gained a Readiness block above the timeline, with the
  request id shown on audit and log rows.
- `cargo test --locked`: **443 passed, 1 ignored** (8 new: id minting, the
  acceptance and rejection rules, the task-local not leaking out of its scope,
  the migration running twice and leaving the pre-existing row empty, the five
  readiness checks with no probe file left behind, and a permission row carrying
  its id while an older row carries none). Clippy `--all-targets -D warnings`
  clean; `node --check` clean.
- Phase 17 is complete; what remains in the roadmap is 13, 15, 16 and 18.

## 2.3.0

Router installs and MCP servers now appear on the diagnostics timeline instead of
in a panel the reader has to remember to visit.

- `timeline_value` projects `router-installs.json` as one row per router, using
  the record's own `installed_at_unix` and outcome — the file keeps one record
  per router, so the row is the newest attempt. Every attempt, successes and
  failures included, is already in the 2.2.0 process log.
- Every MCP server that has actually answered contributes a row at its
  `checked_at`, carrying its tool count and version. A server that was never
  contacted adds nothing: `unknown` is not an event, and stamping it "now" would
  invent one.
- `router` and `mcp` are valid `area` filters on `GET /api/diagnostics` and in
  the panel's Show menu; an unknown area still answers 400 naming the accepted
  set.
- `cargo test --locked`: **435 passed, 1 ignored** (one new case: a seeded failed
  install becomes an error row with its message and the router id as the
  correlation key, `area=router` returns exactly that row, and `area=mcp` is
  accepted while returning nothing when no server was contacted). Clippy
  `--all-targets -D warnings` clean; `node --check` clean.
- Still open in Phase 17: a request id for operations that are not agent runs,
  and a readiness view.

## 2.2.0

Adds the one record that survives a crash before any database row exists.

- `app/src/oplog.rs` appends a JSON object per line to `studio.log` beside the
  database: redacted with the same scanner the support bundle uses, clipped at
  400 characters, flushed per write, rotated at 1 MiB into one previous file
  (`studio.log.1`), with the newest 200 entries also held in memory.
- `GET /api/diagnostics/log?limit=` (loopback-guarded, 1–500, default 100)
  answers from that ring and names the file. Settings → Diagnostics lists the
  entries under the timeline, with the file's path.
- Writes land where a failure currently leaves nothing behind: app start, a
  router install/update/uninstall/restart, a permission ask or deny, a failed
  provider call, and every run ending — written before the status row, so the
  final status is on disk even if the write that follows fails.
- Editing the rule list is not a decision about an action, so adding or
  revoking a rule stays in `permission_audit` and out of the log.
- `cargo test --locked`: **434 passed, 1 ignored** (six new cases: one object per
  line, ring bound, rotation, redaction and clipping before disk, an unwritable
  location still logging in memory, and a no-op before `init`). Clippy
  `--all-targets -D warnings` clean; `node --check` clean.
- Also in this version: the status documents are reconciled at 2.2.0.
  `FEATURE-STATUS.md` carried a 1.28.0 header and rows that 1.41.0–1.72.0 had
  already closed; `PROGRESS.md` still listed two finished Phase 5 items as open;
  and its release table had twelve blank lines between the header and the rows,
  which broke it into an unheaded block.
- Still open: a request id for operations that are not agent runs, router
  installs and MCP sessions as rows in the SQLite timeline, and a readiness
  view.

## 2.1.0

Completes the isolation stage: runs no longer share a file system view or a lease
ledger by accident.

- The lease ledger now lives in `studio.db` (`run_leases`, `run_reads`) instead of a
  process variable, so a headless server and a desktop build pointed at one database
  see each other's claims. Claims held by terminal runs are dropped on the next claim;
  a paused run keeps its recorded reads but loses its claims.
- A command step claims its whole checkout for as long as it runs: two runs may not
  run commands in one working directory at once, because a command can write anywhere
  inside it. A refusal is a non-fatal `command-conflict` step naming the blocker, the
  same shape as a write conflict. Per-run worktrees mean the common case is two
  separate checkouts, so this only applies where runs really share a folder.
- `max_concurrent_runs` defaults to 2, but only a workspace that is itself the root of
  a git repository may run more than one — each run gets its own checkout there. A
  non-git folder stays at one whatever the setting says. The gate compares
  `git rev-parse --show-toplevel` with the workspace: "inside some work tree" would
  also be true for a temp folder nested in an unrelated repository.
- `cargo test --locked`: **428 passed, 1 ignored**, run twice. A new test claims a
  checkout for one live run, refuses a second run in the same checkout, allows a
  different one, and releases. Three queue tests now drive a 2 s mock instead of
  120–150 ms so their "still queued" assertions cannot race the first run finishing.
- Still open: an editor save or another program still writes behind the lease (caught
  by the fingerprint on the run's next write), each process still admits runs on its
  own, and a worktree that is never applied stays on disk.

## 2.0.0

The first release where a run works in its own checkout *and* can hand that work
back, which is what makes parallel runs a usable loop rather than a dead end.

- `POST /api/runs/:id/apply` commits what a finished run changed on its
  `run/<id>` branch, fast-forwards the shared checkout onto that branch, and removes
  the worktree. Nothing is forced: a shared checkout with uncommitted changes, or one
  with commits the run never saw, refuses the merge and git's own sentence is returned
  as a 409. The branch is kept, so the work is still reachable in history.
- The run detail offers **Apply to workspace** exactly when the run still holds a
  checkout (`worktree` in the detail payload), and a live run is refused — its work is
  not finished yet.
- `cargo test --locked`: **427 passed, 1 ignored**. Two new end-to-end tests: a
  finished writing run applies into the repository (the file is in the shared checkout,
  the checkout is gone, the branch remains, a second apply is a 409), and a run still
  waiting for approval is refused with "still live".
- Packages are at 2.0.0: `app/Cargo.toml`, `app/Cargo.lock`, `app/tauri.conf.json`,
  `vscode-extension/package.json` and its lock. The extension's `engines.vscode` and
  `@types/vscode` stay on the VS Code API line (`^1.92.0`), not the app version.
- Still open, and named as such in `docs/PROGRESS.md`: the lease covers only the
  agent's own writes (a command step may write anywhere), the ledger is in-process, so
  `max_concurrent_runs` still defaults to 1; a worktree is removed only when applied.

## 1.92.0

- A run that may write or run commands now works in its own git worktree, on a
  `run/<id>` branch, in a folder next to the repository (`<repo>-run-<id>`), instead
  of the folder the user is working in: two runs can no longer touch the same path.
  A read-only run still reads the shared folder, and a workspace that is not a git
  repository keeps the old behaviour rather than failing.
- File leases are keyed by the checkout as well as the path, so two runs editing the
  same relative path in their own worktrees no longer look like a conflict.
- `cargo test --locked`: **425 passed, 1 ignored** — one new end-to-end test starts a
  real writing run against a real repository and asserts the shared folder gained no
  file, the run's checkout holds it beside the seeded file, and that checkout is on
  `run/<id>`; clippy `-D warnings` clean; build ok.
- Not built: nothing applies a run's branch back to the user's checkout, a worktree is
  not removed when the run ends, the lease still covers only the agent's own writes (a
  command step may write anywhere), the ledger is in-process, and `max_concurrent_runs`
  therefore still defaults to 1.

## 1.91.0

- Source Control pane: `Worktrees…` lists every worktree of the repository with its
  branch and path, creates a new one on a named branch, and removes one again. A new
  worktree lands next to the repository, never inside it, so the folder does not gain
  an untracked directory from its own tooling; an existing branch keeps its commits
  and a fresh name starts at HEAD.
- Three routes: `GET /api/workspace/git/worktrees`, `POST …/worktrees/add` and
  `POST …/worktrees/remove`. Both writes sit behind the `git` permission action, and
  removal refuses a path that is not a worktree of this repository.
- Verification found two Windows bugs. git prints worktree paths with forward
  slashes while the app built them with the platform separator, so removal rejected
  its own worktree (`same_worktree_path` normalises both). The workspace root also
  arrives as a verbatim `\?\` path, which git refuses when it creates the
  directory (`plain_path` strips it).
- `cargo test --locked`: **424 passed, 1 ignored**; clippy `-D warnings` clean;
  `cargo build` ok. Live on a scratch repo at `:8971`: add returned
  `…/wt-repo-side-work` on branch `side-work` with `a.txt` present, the list showed
  both worktrees, remove returned ok and the folder was gone.
- Not built: agent runs still execute in the shared folder. Pointing a run at a
  worktree and applying its branch back is the next piece, and the cross-process
  ledger is still open.

## 1.90.0

- The sanity check now runs by itself. The first time a workspace renders in the
  Source Control pane, the app looks for conflict markers and mixed line endings
  and writes the result into the hint — no click needed. It stays quiet when the
  tree is healthy, and the `Check` button still reports on demand with a toast.
- The automatic pass runs once per workspace rather than on every refresh, so a
  folder switch re-checks and a refresh does not repeat the walk.
- Two defects found while testing it in a browser are fixed here: the toast counted
  a conflicted file once per marker line ("3 files" for two files), and a later
  refresh wiped the finding from the hint. Files are now counted by distinct path,
  and the finding is kept as pane state so refreshes keep showing it.
- `cargo test --locked`: **423 passed, 1 ignored** (unchanged — the work is in
  `app/static/ide.js`, which has no test harness); `node --check` clean.
- Driven in a browser at `:8970` against a repo holding a conflicted file and a
  mixed-ending file: the hint reported both without a click, and `Check` answered
  "2 files need attention."
- Not built: the check still only reads tracked files and repairs nothing.

## 1.89.0

- `git status` can call a tree clean while a bad merge has left markers in files or
  a conversion has half rewritten their line endings. A `Check` button in the
  Source Control pane now asks the server to look: unresolved markers by
  `file:line` and tracked files whose working-tree endings are mixed.
- The endpoint reads the worktree, not the index, because the damage appears there
  first. It reports up to 20 entries of each kind, and the pane names the first few
  in its hint instead of only saying that something failed.
- `cargo test --locked`: **423 passed, 1 ignored** — one new test builds a clean
  repo, asserts an empty report, then writes a conflicted file and a mixed-ending
  file and asserts both are named.
- Driven against a running server at `:8967` with a deliberately damaged repo; the
  route returned both files and the pane reported them. A clean repo answers with
  empty lists and "Repository looks clean."
- Not built: the check does not repair anything, and it only looks at tracked files.

## 1.88.0

- The staging selection now survives a reload. It is written to `localStorage`
  under a key built from the workspace path, so reloading the tab, closing the app
  or switching folders and coming back keeps the lines you picked.
- Loading happens the first time the pane renders a workspace, not on every
  refresh, and the stored copy is removed as soon as the tree is clean or a batch
  is applied — a stale entry cannot outlive the change it described.
- Storage failures (private windows, storage disabled) stay silent: the selection
  still works for the session.
- `cargo test --locked`: **422 passed, 1 ignored** (unchanged — the work is in
  `app/static/ide.js`, which has no test harness); `node --check` clean.
- Driven in a browser at `:8966` against a 20-line file changed at lines 2 and 18:
  selecting both hunks gave "4 lines selected in 1 file" and wrote
  `[{"path":"a.txt","staged":false,"hunks":[[0,[0,1]],[1,[0,1]]]}]` under
  `freeai-scm-selection:<workspace>`; after a fresh page load the bar and the four
  outlined lines came back, and `Stage all selected` moved them.
- Not built: selections are not shared between machines, and clearing site data
  forgets them.

## 1.87.0

- A commit now also refuses staged files that mix CRLF and LF. Each staged blob is
  read through the index (`git show :path`), NUL bytes skip binaries, and a blob
  counts as mixed when it holds at least one CRLF and more LFs than CRLFs — the
  signature of a conversion that stopped half way, which is how the line-ending
  churn in this repository arrived.
- The guards run in order, markers first: a conflicted file is reported by line, a
  line-ending problem by file name, and either way nothing is recorded.
- `cargo test --locked`: **422 passed, 1 ignored** — one new test stages a file
  with two CRLFs and one lone LF, expects the refusal with history unchanged, then
  normalises the file and expects the same commit to succeed.
- Not built: the guard cannot see commits made outside the app, and a file that is
  wholly CRLF is allowed because only a mix is refused.

## 1.86.0

- A commit can no longer record unresolved conflict markers. Before writing, the
  commit route greps the index for `<<<<<<< ` and `>>>>>>> ` lines and refuses
  with the offending `file:line` entries, so a merge that left markers behind
  fails loudly instead of becoming history.
- The check is `git grep --cached`, so it judges exactly the content the commit
  would record rather than whatever the working tree happens to hold. Only
  `<<<<<<<` and `>>>>>>>` count: `=======` is legitimate under Markdown headings.
- `cargo test --locked`: **421 passed, 1 ignored** — one new test stages a file
  with markers, expects the refusal with history unchanged, then resolves the
  file and expects the same commit to succeed.
- Driven against a running server at `:8965`: the route rejected a staged
  conflicted file by naming its location and left `HEAD` where it was; after the
  file was resolved the same request committed.
- Not built: mixed line endings are not refused, and the check does not look at
  commits made outside the app.

## 1.85.0

- The selection now spans files. Each entry remembers its own direction and path,
  so it survives switching between diffs, and a bar above the file list reports
  "4 lines selected in 2 files" with `Move selected` and `Clear all`.
- `Move selected` applies each file's hunks in turn, later hunks first within a
  file, and names the first hunk that fails by file and index instead of stopping
  silently. It knows which entries are staged and which are not, so a mixed batch
  moves each the right way.
- `cargo test --locked`: **420 passed, 1 ignored** (unchanged — the work is in
  `app/static/ide.js`, which has no test harness); `node --check` clean.
- Driven in the real pane in a browser at `:8964` with `a.txt` changed at lines 2
  and 18 and `b.txt` at line 2: `All` in one hunk of each gave "4 lines selected
  in 2 files", `Move selected` answered "4 lines staged in 2 files", the pane
  listed both files under STAGED with no unstaged entry, and `git diff` was empty
  with the index matching the worktree.
- Not built: selections live in memory, so a reload forgets them, and unstaging a
  replacement pair still moves the old text back as one unit.

## 1.84.0

- A selection no longer stops at one hunk. It is kept per file, so it survives
  closing and reopening the diff, and a bar above the hunks counts it and offers
  `Stage all selected` (or `Unstage all selected`) for the whole file.
- The batch is applied hunk by hunk from the last hunk backwards: staging a later
  hunk removes it from the diff and leaves the indexes of earlier hunks alone, so
  the list captured before the first request stays valid. A failed hunk stops the
  batch and names the hunk it failed on rather than moving the rest silently.
- The browser run caught a real bug before the commit: the stale-index cleanup was
  inserted into the batch function instead of the per-hunk one, so
  `scmApplyFileSelection` threw `ReferenceError: index is not defined` after the
  requests had already gone through — the lines were staged, the pane stayed
  stale. Moved into `scmHunkLines`, where `index` exists.
- `cargo test --locked`: **420 passed, 1 ignored** (unchanged — the work is in
  `app/static/ide.js`, which has no test harness); `node --check` clean.
- Driven in the real pane in a browser at `:8963` on a 20-line file changed at
  line 2 and line 18: two hunks, `All` in each read "4 lines selected", the
  selection survived a close/reopen, `Stage all selected` answered "4 lines
  staged in a.txt", the pane moved the file to STAGED with no unstaged entry, the
  index matched the worktree, and the console stayed clean.
- Not built: a selection that spans files, and unstaging a replacement pair as one
  unit.

## 1.83.0

- The hunk selection was only visible: a keyboard or screen-reader user could
  toggle lines but not learn how much of the hunk was picked. Each hunk head now
  carries a polite `aria-live` count, "2 of 4 selected", updated by every path —
  click, Space/Enter, A, Escape, All and Clear.
- The count is read back from the DOM after each change instead of being tracked
  separately, so it cannot disagree with the outlines or `aria-pressed`.
- `cargo test --locked`: **420 passed, 1 ignored** (unchanged — the work is in
  `app/static/ide.js`, which has no test harness); `node --check` clean.
- Driven in the real pane in a browser at `:8962`: the count started at
  "0 of 4 selected" with `aria-live="polite"`; A made it "4 of 4 selected";
  Escape "0 of 4"; All "4 of 4"; `Stage selected` answered "4 lines staged in
  a.txt" and `git show :a.txt` returned `ONE/two/THREE` with the worktree
  unchanged.
- Not built: a count that spans hunks or files, and a worktree per run.

## 1.82.0

- Selecting a whole hunk still meant clicking every changed line. Pressing `A`
  on a focused line now selects all of them, and Escape clears the hunk again;
  each hunk head also carries `All` and `Clear` buttons for the mouse.
- The selection is set through one helper, so the dashed outline and
  `aria-pressed` stay in step for every line either way. Like the per-line
  toggle, this works inside one hunk only, which is the unit the hunk routes move.
- `cargo test --locked`: **420 passed, 1 ignored** (unchanged — the change is in
  `app/static/ide.js`, which has no test harness); `node --check` clean.
- Driven in the real pane in a browser at `:8961` against `one/two/three` →
  `ONE/two/THREE`: `A` on the focused `-one` selected all four lines with
  `aria-pressed` true, Escape cleared them all, `All` selected them again, and
  `Stage selected` answered "4 lines staged in a.txt"; `git show :a.txt` then
  returned `ONE/two/THREE` with the worktree unchanged.
- Not built: a selection spanning hunks or files, a spoken selection count, and a
  worktree per run.

## 1.81.0

- Line-level staging could only be done with the mouse: a keyboard user could
  reach nothing inside a hunk. Each hunk's changed lines are now focus targets.
- A hunk is a single tab stop — the focused line takes `tabindex=0` and the rest
  `-1` — so Tab lands once per hunk instead of on every line. Arrow keys move
  between the changed lines, Home and End jump to the first and last, and Space
  or Enter toggles the focused line, which is exactly what clicking it does.
  Every line carries `role="button"` and `aria-pressed`, kept in step with its
  selection, so the state is announced and not only drawn.
- Clicking still works and also moves the tab stop to the clicked line, so mouse
  and keyboard never disagree about where focus is. A `:focus-visible` outline is
  drawn separately from the dashed selected outline.
- `cargo test --locked`: **420 passed, 1 ignored** (unchanged — this is entirely
  in `app/static/ide.js`, which has no test harness); `node --check` clean, no
  rustfmt diff.
- Driven in the real pane in a browser against a headless build at `:8960`, with
  `a.txt` at `one/two/three` changed to `ONE/two/THREE`: the hunk rendered four
  focusable lines with tab indexes `[0,-1,-1,-1]` and `aria-pressed` all false; a
  real Space press on the focused `+ONE` selected it (`aria-pressed` true); an
  ArrowUp moved focus to `-one`; a second Space selected that too; `Stage
  selected` answered "2 lines staged in a.txt"; `git show :a.txt` then returned
  `ONE/two/three` with only `-one`/`+ONE` in `git diff --cached`, and the
  worktree still held `ONE/two/THREE`.
- Not built: selecting every line from the hunk or group header with one key, a
  spoken selection count, and a worktree per run.

## 1.80.0

- 1.79.0 could add and remove a remote but not correct one, so a mistyped URL
  meant removing the remote and adding it back.
  `POST /api/workspace/git/remotes/set-url` now changes it in place, with the
  same `name`/`url` body and the same `git` permission gate as the other remote
  routes.
- The remote must already exist: an unknown name is refused by name, so a typo
  cannot quietly create a second remote. The URL is narrowed exactly as when
  adding (capped at 2048 characters, no leading dash, no whitespace or control
  characters), which still allows https, scp-style `git@host:path` and local
  paths. The route answers `{ok, name, url, remotes}` with the refreshed list.
- UI: each remote row has `Edit`, which fills the form with that remote's name
  and URL and turns `Add` into `Save URL`; it returns to `Add` when the form is
  closed or the edited remote is removed.
- `cargo test --locked`: **420 passed, 1 ignored** (1 new: an added remote
  changed in place, with the list still holding exactly one remote and the old
  URL gone, plus an unknown name refused). Clippy `-D warnings` clean, no
  rustfmt diff in the touched files, `node --check` clean.
- Live on a headless build at `:8959`: add `origin` → 200; `set-url` to
  `https://example.com/new.git` → 200 `{name: origin}` with
  `git remote get-url origin` returning the new URL while `git remote` still
  listed one remote; an unknown `upstream` → 400 "`upstream` is not a remote of
  this repository."; a URL with a space → 400; an scp-style
  `git@host:repo.git` → 200 and stored verbatim. The pane itself was not viewed
  in a browser.
- Not built: multiple fetch URLs per remote, a credentials UI, a worktree per
  run.

## 1.79.0

- Fetch, pull and push could only use a remote that already existed, and
  creating one meant leaving the app for a terminal. The Source Control pane now
  lists, adds and removes remotes.
- `GET /api/workspace/git/remotes` reports each remote's name, fetch URL and push
  URL — the push URL only differs when git actually reports one.
  `POST /api/workspace/git/remotes/add` (body `name`, `url`) and
  `/remotes/remove` (body `name`) are gated on the `git` action with the remote
  name as the target, so the audit line names what was touched.
- Both values are narrowed before they reach argv: a name cannot start with a
  dash, is capped at 100 characters and only takes alphanumerics and `._-/`; a
  URL is capped at 2048 characters and cannot start with a dash or contain
  whitespace or control characters, which still leaves `https://`, scp-style
  `git@host:path` and local paths working. A duplicate add surfaces git's own
  sentence, and removing a remote that is not configured is refused by name
  instead of silently succeeding.
- UI: `Remotes…` toggles a name/URL form with `Add` and a row per configured
  remote with `Remove`; a row whose push URL differs says so in its tooltip.
  The "no remote" error from fetch, pull and push now points at the pane
  instead of at a terminal command.
- `cargo test --locked`: **419 passed, 1 ignored** (3 new, against real
  repositories: an add/list/remove round trip that also asserts a duplicate and
  a missing remote are refused; a remote whose push URL differs from its fetch
  URL reported separately; and the name/URL narrowing). Clippy `-D warnings`
  clean, no rustfmt diff in the touched files, `node --check` clean.
- Live on a headless build at `:8958`: empty list `{ok: true, remotes: []}`;
  add `origin` → 200 with the new row and `git remote` showing `origin`; the same
  name again → 400 "error: remote origin already exists."; `-x` → 400 "A remote
  name cannot start with a dash."; `a b` → 400 "That remote name is not valid.";
  a URL with a space → 400 "A remote URL cannot contain spaces or control
  characters."; remove `origin` → 200 with the remote gone from `git remote`;
  removing it again → 400 "`origin` is not a remote of this repository." The
  pane itself was not viewed in a browser.
- Not built: editing an existing remote's URL in place (remove and re-add),
  multiple fetch URLs per remote, a credentials UI, a worktree per run.

## 1.78.0

- The last gap in the inline diff was that a hunk moved as a whole, so two
  unrelated edits sitting close together could not be staged apart.
  `stage-hunk` and `unstage-hunk` now take an optional `lines` array of
  changed-line indexes.
- `hunk_lines_patch` rebuilds the hunk from git's own diff by pairing each
  removal with its addition. An unselected line has to look the same in the
  patch's pre-image and post-image, and which text that is depends on the side
  the index holds: the old text when staging, the new text when unstaging. A
  line the current side does not contain is dropped, and the `@@` counts are
  recomputed because git refuses a patch whose counts are wrong. The patch still
  travels on stdin, never in argv, and `--unidiff-zero` lets a hunk that is all
  change lines — no context for git to anchor on — apply at all.
- An empty `lines` array, an index past the hunk's changed lines, a `lines`
  field that is not an array, and more than 500 picks are 400s that leave the
  index untouched.
- UI: every `+`/`-` line in a hunk is clickable, and `Stage selected` /
  `Unstage selected` moves exactly the clicked lines.
- `cargo test --locked`: **416 passed, 1 ignored** (3 new, against real
  repositories: selecting one replacement pair of a two-block hunk stages that
  pair and leaves the other change in the worktree; the same selection unstages
  back out of the index; and an empty or out-of-range selection refuses and
  stages nothing). Clippy `-D warnings` clean, no rustfmt diff in the touched
  files, `node --check` clean.
- Live on a headless build at `:8957`: `lines: [0, 1]` → 200 `{lines: 2}` with
  the index holding `ONE`/`two`/`three` and only `-one`/`+ONE` in
  `git diff --cached`, the worktree still `ONE`/`two`/`THREE`; the same indexes
  unstage → 200 and the index returns to `one`/`two`/`three`; `lines: []` → 400
  "Select at least one changed line"; `lines: [99]` → 400 "Line 99 is not a
  changed line in this hunk; it has 4", with nothing staged. The pane itself was
  not viewed in a browser.
- Not built: keyboard line selection, adding or removing remotes from the pane,
  `git rebase --skip`, an in-pane reflog undo, a worktree per run.

## 1.77.0

- 1.76.0's documented gap: a conflicted rebase could only be finished in a
  terminal. `POST /api/workspace/git/rebase/continue` now finishes it from the
  pane once the conflicts are resolved **and staged**.
- `continue_rebase` refuses with a 409 that names every unmerged path before git
  is asked, because git's own "you must edit all merge conflicts" never says
  which files. It then runs `git -c core.editor=true rebase --continue`, so a
  process with no terminal can never wait on an editor it cannot answer.
- A second conflict is reported with its own path list, and a rebase that has
  already finished reports that it has nothing to continue instead of
  pretending. Success answers `{continued: true, branch, summary}` with git's own
  output, and the branch is back on its ref with a clean tree.
- UI: a `Continue` control in the Rebase form beside `Abort rebase`.
- `cargo test --locked`: **413 passed, 1 ignored** (2 new, against real
  repositories: a conflict refused before staging with the file named, then
  continued after staging with the replayed log order and a clean tree
  asserted, then refused once finished; and a repository with no rebase in
  progress refusing). Clippy `-D warnings` clean, no rustfmt diff, `node --check`
  clean.
- Live on a headless build at `:8956`: continue with nothing in progress → 409;
  a conflicting rebase → 409 `{conflicted: ["a.txt"]}`; continue before
  resolving → 409 "1 file(s) are still conflicted: a.txt"; after staging → 200
  `{continued: true, branch: "left"}` with `Successfully rebased and updated
  refs/heads/left.`; the log became `left, right, base`, the file held the
  resolution, `git status --porcelain` was empty, and a further continue → 409.
  The pane itself was not viewed in a browser.
- Not built: `git rebase --skip`, an in-pane reflog undo, line-level staging, a
  worktree per run.

## 1.76.0

- The Source Control pane's history work stopped at merge; **rebase** is the
  other half, and git could only be driven from a terminal for it.
  `app/src/git.rs` now runs a real rebase and a real abort.
- `POST /api/workspace/git/rebase` (body `branch`) answers
  `{branch, current, summary}` when git succeeds and **409**
  `{conflicted, output, inProgress}` when it stops on conflicts, leaving the
  tree for the person to finish or abort.
- `POST /api/workspace/git/rebase/abort` puts the branch back where the rebase
  started and refuses with a 409 when nothing is in progress — the one thing
  that makes a history rewrite safe to attempt.
- Refused over uncommitted **tracked** changes and when the named branch is the
  one already checked out, so the action can never be a no-op that looks like
  work. The branch name is narrowed before argv and both routes are gated on the
  `git` action.
- Merge's clean-tree check and ref validation moved into shared helpers
  (`dirty_tracked`, `valid_ref`) rather than being copied for rebase.
- UI: a `Rebase…` form in the Source Control pane, the conflicted files named in
  the toast, and an `Abort rebase` control.
- `cargo test --locked`: **411 passed, 1 ignored** (4 new, all against real
  repositories: a rebase replaying a commit onto an advanced branch with the
  resulting log order asserted; a conflict followed by an abort that restores
  the original branch, file and clean tree with a second abort refused; a dirty
  tracked file refused with the edit untouched; an unknown branch, an
  option-like name and a self-rebase all refused). Clippy `-D warnings` clean,
  no rustfmt diff in the touched files, `node --check` clean.
- Live on a headless build at `:8955`: `rebase main` → 200 `{current: feature}`
  with git's own `Rebasing (1/1)` summary and the log ordering proven
  (`feature work`, `main work`, `base`); `-x` → 400; `nope` → 400; `feature` →
  400 "already checked out"; a conflicting rebase → 409
  `{conflicted: ["a.txt"], inProgress: true}`; abort → 200, then 409, with the
  branch and file back; a dirty tracked file → 409; `git status --porcelain`
  empty afterwards. The pane itself was not viewed in a browser.
- Not built: an in-pane "continue rebase", line-level staging, and a worktree
  per run.

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
  showed `LegacyGeneric:target=mcp_oauth/auth.SarthiStudio-mcp-smoke144`; an
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
  `io.github.sairajkhilariarch.SarthiStudio` (the AppStream id), one entry per
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
  component ID now all use `io.github.sairajkhilariarch.SarthiStudio`, and an
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
  `free-ai-suite_1.26.2_*` instead of carrying an embedded `
` that made
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
  → `Hello, Sarthi!`, exit 0), reviewer APPROVE, run succeeded in team mode.

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

- Every user-facing string says **Sarthi Studio** (window title, page title,
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
