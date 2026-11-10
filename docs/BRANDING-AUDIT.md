# Branding audit — Sarthi Studio

Product-wide identity audit for 2.51.0. The official display name is **Sarthi
Studio**; this file records what was renamed, what was deliberately kept, and
why keeping it is the safe choice. Regenerate the numbers with:

```bash
git grep -I -o -F "Sarthi Studio" | wc -l
git grep -I -o -F "free-ai-studio" | wc -l
git grep -I -n -E "saarthi|Saarthi|free-ai-suite"
```

## 1. What the first pass found (at 2.50.0)

| Pattern | Occurrences | Files | Kind |
|---|---|---|---|
| `Saarthi Studio` | 148 | 72 | user-facing brand |
| `saarthi` (lowercase) | 132 | 38 | temp names, ids, urls, docs |
| `Saarthi-Studio` | 32 | 12 | repository slug in links |
| `saarthi-studio` | 26 | 18 | artifact names, paths |
| `SaarthiStudio` | 21 | 11 | desktop-id / metainfo component id |
| `SAARTHI` | 17 | 4 | `[[SAARTHI_IMAGE:…]]`, the Android speaker label |
| `Saarthi` (bare) | 15 | 9 | prose, sentence starts |
| **`saarthi*` total** | **391** | **113** | |
| `Free AI Studio` | 31 | 12 | old product name (title, installer, docs) |
| `freeAiStudio` | 31 | 3 | VS Code command and setting ids |
| `freeaistudio` | 5 | 2 | Tauri identifier, package ids |
| `free-ai-studio` | 187 | 45 | crate, binary, env vars, config/data paths |
| `FREE_AI_STUDIO` | 75 | 20 | environment variables |
| `free-ai-suite` | 119 | 24 | legacy Linux install tree, icons, artifact names |

Nothing was in a binary asset except the icons, which were renamed rather than
edited.

## 2. Renamed (346 replacements across 98 files)

| Old | New | Where |
|---|---|---|
| `Saarthi Studio` | `Sarthi Studio` | window/tab titles, About, settings, docs, installer text |
| `Free AI Studio` | `Sarthi Studio` | `productName`, README, installer display names, Android label |
| `Saarthi-Studio` | `Sarthi-Studio` | artifact names, paths — **except** the repo slug |
| `SaarthiStudio` | `SarthiStudio` | `.desktop`/metainfo ids, file names |
| `Saarthi` | `Sarthi` | prose, log and error text, test names |
| `saarthi-studio` | `sarthi-studio` | CI artifact names, release staging, docs |
| `saarthi` (temp prefixes) | `sarthi` | worktree/checkpoint temp file names |
| `saarthi.workflow` | `sarthi.workflow` | workflow export format field |
| `SAARTHI` | `SARTHI` | Android speaker label — **except** the image marker |
| `free-ai-suite` | `sarthi-studio` | Linux install tree, icon basenames, MSI/AppImage names |

### Files renamed

| Old path | New path |
|---|---|
| `app/packaging/io.github.sairajkhilariarch.SaarthiStudio.desktop` | `…SarthiStudio.desktop` |
| `app/packaging/io.github.sairajkhilariarch.SaarthiStudio.appdata.xml` | `…SarthiStudio.appdata.xml` |
| `app/packaging/icons/free-ai-suite.svg` | `app/packaging/icons/sarthi-studio.svg` |
| `app/packaging/icons/free-ai-suite-256.png` | `app/packaging/icons/sarthi-studio-256.png` |

Renaming the `.desktop` file was also a correctness fix: `Cargo.toml` and both
desktop workflows already referenced `…SarthiStudio.desktop` after the text
pass, so the old filename would have failed the packaging check.

## 3. Deliberately retained identifiers

Changing any of these breaks installed applications, stored conversations or
in-flight releases. Each is machine-facing; none is displayed to a person.

| Identifier | Where | Why it stays | Migration path if it must change |
|---|---|---|---|
| crate/binary `free-ai-studio` | `app/Cargo.toml`, `Cargo.lock`, all packaging scripts, `.desktop` `Exec=` | installed binaries, updater manifests, CI stages and user scripts invoke this name | rename crate + emit both binaries for one release, keep the old binary as a shim that execs the new one |
| `FREE_AI_STUDIO_*` | env vars (`HOME`, `PORT`, `STATIC_DIR`, e2e harness, docs) | CI, scripts and the e2e suite set them; renaming silently changes behaviour of existing wrappers | read both names with the new one winning, then drop the alias after two minor releases |
| `~/.config/free-ai-studio/` | config path | existing settings; moving it is the data-loss scenario the brief forbids | copy-on-first-boot into the new path, never move, keep reading the old one |
| `~/.local/share/free-ai-studio/` | `history.sqlite3`, projects, skills, workflows | all user data | same: copy + verify, never move, old path still read |
| Tauri `identifier` `com.saarthi.freeaistudio` | `app/tauri.conf.json`, macOS `CFBundleIdentifier` | bundle identity ties upgrades and app-data association to it | new identifier + explicit migration of the app-data folder, shipped as its own release |
| Android `com.saarthi.studio` | `android/app/build.gradle`, manifest, Java packages | package id is the install/upgrade identity; changing it installs a second app and orphans data | new package id with an export/import migration, not a rename |
| `[[SAARTHI_IMAGE:…]]` | stored message bodies, Android `BackupPolicy`/`MainActivity`, backup validation | the marker is inside conversation rows and exported backups; renaming it corrupts existing history | accept both markers on read, write the new one only after a backup round-trip test |
| `freeAiStudio.*` | VS Code command and setting ids | user keybindings and `settings.json` entries reference them | register the new ids as aliases of the old ones for one release |
| `sairajKhilariArch/Saarthi-Studio` | every GitHub link, `update.rs`, `router_registry.rs` | this is the repository name, not the product name; rewriting it breaks links and the update check | rename the repository on GitHub, which redirects, then update both in one commit |
| `app/Cargo.lock` package name | lockfile | generated; follows `Cargo.toml` | follows the crate rename |

## 4. Files modified

98 text files carry brand replacements (`git show --stat 2.51.0` lists them in
full). By area:

| Area | Files | What changed |
|---|---|---|
| `app/static/` | 6 | titles, welcome/empty states, About, toasts, tooltips |
| `app/src/` | 18 | log/error text, temp file names, test names, static-dir comment |
| `app/` metadata | 3 | `Cargo.toml` (version + description), `Cargo.lock`, `tauri.conf.json` |
| `app/packaging/` | 4 (2 renamed) | MSI display names, metainfo, desktop entry, icons |
| `.github/workflows/` | 4 | artifact names, staging paths, smoke expectations |
| `scripts/` | 3 | bundle/MSI builders, artifact checks |
| `vscode-extension/` | 5 | display name, command titles, docs |
| `android/` | 12 | launcher label, speaker label, legacy docs and scripts |
| docs | 12 | README, architecture, glossary, changelog, status docs |
| root scripts | 2 | `install.sh`, `.gitignore` comment |

## 5. Data migration and backward compatibility

**No user-data migration is required, because no user-data path moved.** The
config directory, the data directory, the SQLite file name and the environment
variables are byte-identical before and after this release.

Two changes touch on-disk layout, both handled:

1. **Linux install tree.** New packages install the UI under
   `usr/share/sarthi-studio/static`. `static_dir_candidates` tries that first
   and then `usr/share/free-ai-suite/static`, so a machine still carrying the
   pre-2.51 tree keeps starting; the unit test asserts the order. Nothing is
   deleted on upgrade.
2. **Workflow export `format`.** Export now writes `sarthi.workflow`. Import
   never validated the field (it checks only name/prompt/nodes), so documents
   exported before 2.51 import unchanged — verified by reading `parse_import`.

Stored conversations, backups and the Android backup validator keep the
`[[SAARTHI_IMAGE:…]]` marker, so old messages render exactly as before.

## 6. Remaining old-name references (second full audit)

`git grep` after the pass:

| Pattern | Left | Explanation |
|---|---|---|
| `saarthi` | 40 in 38 files | `com.saarthi.studio` (Android package), `com.saarthi.freeaistudio` (bundle id), repo slug |
| `Saarthi` | 12 in 6 files | `sairajKhilariArch/Saarthi-Studio` links only |
| `SAARTHI` | 13 in 4 files | `[[SAARTHI_IMAGE:…]]` marker + its tests |
| `freeAiStudio` | 31 in 3 files | VS Code command/setting ids |
| `free-ai-studio` | 187 in 45 files | crate, binary, `FREE_AI_STUDIO_*`, config/data paths, docs describing them |
| `FREE_AI_STUDIO` | 75 in 20 files | environment variables |
| `free-ai-suite` | 7 in 2 files | 5 historical CHANGELOG lines, 2 the intentional runtime fallback |
| `Free AI Studio`, `Saarthi Studio`, `SaarthiStudio` | 0 | fully replaced |

Every survivor is one of the identifiers in §3, the historical changelog lines
that describe releases shipped under the old artifact names, or the documented
fallback path.

## 7. Cross-platform status

| Platform | Changed | Verified here |
|---|---|---|
| Windows | MSI product name, shortcut, uninstaller, install folder, artifact `sarthi-studio_<v>_windows-x86_64.msi` | `test-installer.sh` built a real MSI and admin-installed it (11/11). No interactive install, nothing signed. |
| Linux | deb/AppImage staged under `sarthi-studio`, metainfo id + file, `.desktop` name and icon, artifact names | packaging scripts and `Cargo.toml` asset list inspected; AppStream validation (`appstreamcli`) **not** run |
| macOS | `Sarthi Studio.app`, `CFBundleName`, bundle id kept | `test-bundle.sh` built a bundle and zipped it (13/13); not run on macOS |
| Android | launcher label `Sarthi Studio`, speaker label `SARTHI`, package id kept | grep only; Gradle/instrumentation tests **not** run |
| VS Code | `displayName`, command titles | grep only; extension tests **not** run |
| Web UI | tab/window title, About, dock labels | frontend suite 43/43; headless render not re-driven |

## 8. Outstanding

- `appstreamcli validate-tree` has not been run against the renamed metainfo;
  the filename/id/desktop-id triple now matches, but validation is the proof.
- macOS bundle and Android build were exercised only through their local
  scripts / not at all; GitHub Actions has not been observed since the change.
- The Tauri desktop window was not launched, so its title bar is verified by
  config (`productName`) rather than by sight.
- `free-ai-studio` still appears in `ps`, install paths and the updater; this is
  intentional and reversible only through the migrations in §3.
