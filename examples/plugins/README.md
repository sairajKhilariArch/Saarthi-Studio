# Example plugins

Working reference plugins, not mockups. Each one installs through the normal
installer and runs in the normal sandbox.

| Folder | What it is |
| --- | --- |
| [`hello-studio/`](hello-studio/) | two wasm tools (`echo`, `settings`), a JSON formatter panel and an Ollama provider preset |

Install one from **Settings → Extensions** by folder path, or with:

```sh
curl -s localhost:8765/api/extensions/install \
  -H 'content-type: application/json' -d '{"path":"'$PWD'/examples/plugins/hello-studio"}'
```

Read [docs/plugins/EXAMPLE-PLUGIN.md](../../docs/plugins/EXAMPLE-PLUGIN.md) for a
walkthrough, and [docs/plugins/SDK.md](../../docs/plugins/SDK.md) for the host
interface these use.
