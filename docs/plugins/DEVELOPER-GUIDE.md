# Plugin developer guide

## 1. Lay out the folder

```text
hello-studio/
  manifest.json
  echo.wasm          # a tool module
  echo.wat           # its readable source (optional, but keep it)
  json.html          # a panel
```

The folder name must equal the manifest `id`.

## 2. Write the manifest

Start from [MANIFEST.md](MANIFEST.md) and declare the least you need:

```json
{
  "apiVersion": 1,
  "id": "hello-studio",
  "name": "Hello Studio",
  "version": "0.1.0",
  "description": "Two sandboxed tools and a JSON formatter panel.",
  "runtime": "wasm",
  "entry": "echo.wasm",
  "permissions": [],
  "tools": [{ "name": "echo", "description": "Returns its arguments.", "entry": "echo.wasm" }],
  "panels": [{ "id": "json", "title": "JSON formatter", "entry": "json.html" }]
}
```

`"permissions": []` is a feature, not a shortcut: a tool that only transforms its
arguments needs no access to anything.

## 3. Build and validate

```sh
cargo build --target wasm32-unknown-unknown --release   # or wat2wasm
curl -s localhost:8765/api/plugins/validate \
  -H 'content-type: application/json' -d '{"path":"'$PWD'/hello-studio"}'
```

Fix every entry in `errors`. Warnings (for example an incompatible
`requiresStudio`) do not block install but will block a run.

## 4. Install and try it

- **Plugins** page → the extension must appear once installed.
- Install from **Settings → Extensions** by absolute folder path, or publish a zip
  in a registry ([MARKETPLACE.md](MARKETPLACE.md)).
- Call the tool with:

```sh
curl -s localhost:8765/api/extensions/hello-studio/tools/echo \
  -H 'content-type: application/json' -d '{"arguments":{"hello":"studio"}}' | jq
```

A tool call needs an `extension_tool` allow rule in **Settings → Permissions**.
The refusal names the rule to add; it is not a bug.

## 5. Debugging

| Symptom | Cause |
| --- | --- |
| `This module imports "env"."read", which the sandbox does not provide` | the manifest does not declare `workspace.read`, or no rule allows the path |
| `Tool "x" rejected the arguments` | `inputSchema` did not match the call |
| `The run exceeded its 5-second wall-clock budget` | the module stayed inside one computation; it stops at the next host call, or at the fuel limit |
| `The 64-call budget for host access is spent` | batch work instead of calling per item |
| `This extension is disabled` | enable it on the Plugins page |
| A tool call with no result | `env.result` was never called; a tool must report once |

Every run is recorded with fuel used and host calls, so a module that burns fuel
without doing work is visible in the same list.

## 6. Versioning

- Bump `version` on every user-visible change; three numeric parts.
- Set `requiresStudio` only when the plugin truly needs a newer host ABI.
- Keep `apiVersion` at `1`; a future schema will arrive as `2` and older builds
  will refuse it by design.
- Declare `dependencies` for another extension you call into; a missing one blocks
  the run and is reported by id.

## 7. Publish

1. Zip the folder (folder at the root, or exactly one top-level folder).
2. Compute `sha256sum hello-studio-0.1.0.zip`.
3. Add a listing to your registry index with that hash.
4. Host the index and the zip over HTTPS.

Nothing is published automatically, and the app never installs a plugin without a
person choosing it.

## Layout conventions

| Kind | Folder | Written by |
| --- | --- | --- |
| extension | `<data>/extensions/<id>/` | the installer, or you |
| extension `data/` | `<data>/extensions/<id>/data/` | the plugin, through `data_write` |
| skill | user skills folder (`skills.json` records origin/version) | the Skills page, or you |

## Example

[EXAMPLE-PLUGIN.md](EXAMPLE-PLUGIN.md) walks through the shipped, working
`examples/plugins/hello-studio` — two tools, a panel and a provider preset.
