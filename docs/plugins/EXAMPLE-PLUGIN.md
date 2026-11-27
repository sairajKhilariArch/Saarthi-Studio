# Example plugin: Hello Studio

`examples/plugins/hello-studio/` is a complete, working plugin. It is not a mock:
the test `extensions::tests::the_example_plugin_installs_and_its_tools_run`
installs it through the real installer, runs both of its tools in the sandbox,
and asserts what they answer.

## What it contains

| File | Purpose |
| --- | --- |
| `manifest.json` | two tools, one panel, one provider preset, no permissions |
| `echo.wasm` / `echo.wat` | a tool that returns the JSON arguments it was given |
| `settings.wasm` / `settings.wat` | a tool that returns the plugin's saved settings |
| `json.html` | a JSON formatter panel that runs entirely inside the sandbox |

## Install it

```sh
# from the repository root
cp -r examples/plugins/hello-studio /tmp/hello-studio
curl -s localhost:8765/api/plugins/validate \
  -H 'content-type: application/json' -d '{"path":"/tmp/hello-studio"}'
curl -s localhost:8765/api/extensions/install \
  -H 'content-type: application/json' -d '{"path":"/tmp/hello-studio"}'
```

Or paste `/tmp/hello-studio` into **Settings → Extensions → Install folder**.

## What you get

1. **Plugins** lists it as an extension with `2 tools · 1 panel`, enabled.
2. **Settings → Extensions** shows it, and its panel button opens the JSON
   formatter — paste broken JSON and it says which character failed.
3. Its provider preset appears with the other presets: `Ollama (local)` pointing
   at `http://127.0.0.1:11434/v1`. Nothing is contacted until you use it.
4. Saving settings on the Plugins page and calling `settings` returns them:

```sh
curl -s localhost:8765/api/plugins/extension/hello-studio/settings \
  -H 'content-type: application/json' -d '{"settings":{"mode":"fast"}}'
curl -s localhost:8765/api/extensions/hello-studio/tools/settings \
  -H 'content-type: application/json' -d '{"arguments":{}}'
# {"result":"{\"mode\":\"fast\"}"}
```

Tool calls need an `extension_tool` allow rule for `hello-studio/settings` in
Settings → Permissions; the refusal names the rule.

## Why it is safe to run

- `permissions: []`: no filesystem, no network, no shell, no MCP. The modules can
  only read their arguments and settings and report an answer.
- The panel has no bridge and no network.
- The provider preset is a URL and a model name; the app makes the request.

## Changing it

Edit a `.wat` file, then rebuild the module. From the repository:

```sh
REGENERATE=1 cargo test --manifest-path app/Cargo.toml \
  regenerate_example_tools -- --ignored
```

or with wabt: `wat2wasm echo.wat -o echo.wasm`. The test
`example_tool_modules_match_their_wat` fails if the `.wasm` and the `.wat` drift
apart, so a compiled module cannot quietly stop matching the source in the tree.

## Reuse it

Copy the folder, rename it, change `id` to the new folder name and bump
`version`. That is the whole "new plugin" flow.
