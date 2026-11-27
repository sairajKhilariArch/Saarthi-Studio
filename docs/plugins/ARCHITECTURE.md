# Plugin architecture

Sarthi Studio has two plugin kinds, and one catalog over both.

| Kind | What it is | What it contributes | Executes code? |
| --- | --- | --- | --- |
| **Extension** | a folder with `manifest.json` | sandboxed wasm tools, read-only HTML panels, declarative provider presets | yes, inside a wasm sandbox |
| **Skill** | a folder with `SKILL.md` | instructions the model follows, plus declared capabilities it expects | no — prompt text only |

Both are listed together on **Plugins**, are enabled and disabled from the same
switch, and are installed and removed from the same page. They are stored and
executed by different modules because they are different things: pretending a
Markdown file has a sandbox, or that a wasm module is prompt text, would hide
real limits.

## Module map

| File | Responsibility |
| --- | --- |
| `app/src/extensions.rs` | manifest parsing and validation, install/remove, listing, dependency and compatibility rules, provider presets, panel serving, tool calls |
| `app/src/extension_run.rs` | the wasm sandbox (wasmi), its host functions, limits and audit trail |
| `app/src/skills.rs` | skill discovery, install/import/create/export, enablement, usage history |
| `app/src/plugins.rs` | the unified catalog, per-plugin state and settings, the marketplace and manifest validation routes |
| `app/src/agent.rs` | advertises extension tools to the model and dispatches a chosen call |
| `app/static/app.js`, `app/static/index.html` | the Plugins page and the Extensions settings panel |

## Where things live

```text
<data dir>/                     # ~/.local/share/SAARTHI-STUDIO on Linux
  studio.db                     # extension_runs, skill_usage, plugin_state
  extensions/
    hello-studio/               # folder name == manifest id
      manifest.json
      echo.wasm
      json.html
<config dir>/                   # ~/.config/SAARTHI-STUDIO on Linux
  skills.json                   # origin/version record for installed skills
  plugin-registries.json        # marketplace registry URLs
```

Skills come from four roots in precedence order: `FREE_AI_STUDIO_SKILLS_DIR`
when set (the only root in that case), the built-in `static/skills` folder, the
open project's `skills/` folder, and the user's own skills folder. Only the user
root is written to.

## Lifecycle

1. **Discovery** — every listing recomputes the verdict from the manifests on
   disk (`extensions::inspect`). Nothing is cached, so an app upgrade re-judges a
   folder against the build that is running now.
2. **Validation** — `parse_manifest` accepts a closed schema: unknown permissions
   fail, a folder name that differs from the id fails, a future `apiVersion`
   fails, an entry path that could escape the folder fails.
3. **Install** — the folder is copied to a staging directory inside the
   extensions root and renamed into place, so an interrupted copy cannot leave a
   half extension. A folder whose manifest is valid but incompatible is removed
   again with the reason.
4. **Enable/disable** — one row in `plugin_state` (extensions) or one entry in
   the config's `disabled_skills` (skills). Missing means enabled.
5. **Run** — an extension run or tool call re-checks validity, compatibility,
   dependencies and enablement before a single byte of the module is loaded.
6. **Removal** — the folder moves to `.removed/`; nothing is deleted.

## Enablement is enforced, not cosmetic

A disabled extension:

- is not advertised to the model (`extensions::catalog` skips it), so the agent
  cannot choose a tool that would be refused;
- refuses a direct run (`POST /api/extensions/:id/run`);
- refuses a tool call (`POST /api/extensions/:id/tools/:tool` and the agent's
  dispatch path);
- stops contributing provider presets;
- stops serving its panels.

A disabled skill is not mentioned to the router in the skills index and is not
auto-selected, which is the same list the Skills page edits.

## Isolation and limits

Extension code runs in wasmi with no ambient access: no filesystem, no clock, no
environment, no network unless the manifest declared the capability *and* a saved
permission rule allows the specific path or host. See
[PERMISSIONS.md](PERMISSIONS.md). Budgets per run:

| Limit | Value |
| --- | --- |
| fuel (deterministic instruction budget) | 50,000,000 |
| wall clock | 5 s |
| linear memory | 64 MiB |
| tables | 100,000 elements |
| module file | 8 MiB |
| host calls | 64 |
| bytes per host call | 1 MiB |
| tool arguments | 64 KiB |

Panels are HTML served under `default-src 'none'` with no bridge to the app.

## Compatibility and dependencies

- `apiVersion` names the manifest schema. Only `1` exists; any other number is
  refused at parse rather than read partially.
- `requiresStudio` states the oldest app version that works. A well-formed
  manifest naming a newer build stays *valid* but is *incompatible*: it is
  neither installed nor run, and the row says why.
- `dependencies` lists extension ids that must be installed first. A missing one
  blocks a run but not an install, because the dependency can be added after.
  Cycles are detected across the installed set and reported as a chain.

## Error isolation

A manifest that fails to parse becomes an `invalid` row with its error text; it
never stops the other extensions from loading. A failing run is recorded in
`extension_runs` with exit code, fuel used, host calls and the error, and leaves
the app running. Marketplace registries are fetched independently: one broken
URL produces one error entry beside the listings that did load.

## Deliberately not implemented

- No plugin can install another plugin, or add a registry.
- No extension can call the app's HTTP API or read another plugin's data folder.
- Skills have no per-skill permissions because they are not executable; their
  `permissions` field is a declaration the UI shows and the prompt repeats.
- There is no bundled marketplace and no unsigned auto-update of plugins: an
  install is always a deliberate action against a registry the person added.
