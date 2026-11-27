# Hello Studio

A working reference plugin for Sarthi Studio.

- `echo` — returns the JSON arguments it was given, through `env.arguments` and
  `env.result`.
- `settings` — returns this plugin's saved settings, through `env.config`.
- `json.html` — a JSON formatter panel that runs inside the sandboxed frame.
- A provider preset for a local Ollama at `http://127.0.0.1:11434/v1`.

It declares `"permissions": []`, so it has no filesystem, network, shell or MCP
access at all.

```sh
cp -r . /tmp/hello-studio
curl -s localhost:8765/api/extensions/install \
  -H 'content-type: application/json' -d '{"path":"/tmp/hello-studio"}'
```

Rebuild a module after editing its `.wat`:

```sh
REGENERATE=1 cargo test --manifest-path app/Cargo.toml regenerate_example_tools -- --ignored
```

Full walkthrough: [docs/plugins/EXAMPLE-PLUGIN.md](../../../docs/plugins/EXAMPLE-PLUGIN.md).
