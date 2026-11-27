# Manifest specification (`apiVersion: 1`)

One `manifest.json` per extension folder. The folder name **is** the id: a
mismatch is refused so a copy-and-rename mistake is visible instead of loading
the wrong plugin.

```json
{
  "apiVersion": 1,
  "id": "hello-studio",
  "name": "Hello Studio",
  "version": "0.1.0",
  "description": "What it does, in one line.",
  "author": "Someone",
  "runtime": "wasm",
  "entry": "echo.wasm",
  "permissions": ["workspace.read"],
  "requiresStudio": "4.0.0",
  "dependencies": [],
  "tools": [],
  "panels": [],
  "providers": []
}
```

## Fields

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `apiVersion` | number | no | missing means `1`; any other value is refused |
| `id` | string | yes | 1–40 chars of `a-z`, `0-9`, `-`, `_`; must equal the folder name |
| `name` | string | yes | 1–80 chars, trimmed |
| `version` | string | yes | exactly three numeric parts, e.g. `1.2.3` |
| `description` | string | no | ≤ 400 chars, single line |
| `author` | string | no | ≤ 80 chars |
| `runtime` | string | no | only `wasm`; missing means installable but not executable |
| `entry` | string | when `runtime` is `wasm` | relative path inside the folder, no `..`, no `\`, no `:`, ≤ 160 chars |
| `permissions` | string[] | no | ≤ 16, each from the closed list in [PERMISSIONS.md](PERMISSIONS.md) |
| `requiresStudio` | string | no | three numeric parts; newer than the running build means incompatible |
| `dependencies` | string[] | no | ≤ 16 extension ids |
| `tools` | object[] | no | ≤ 16, see below; a manifest with tools must declare `runtime: "wasm"` |
| `panels` | object[] | no | ≤ 8 |
| `providers` | object[] | no | ≤ 8 |

`manifest.json` itself is capped at 64 KiB.

## Tools

```json
{
  "name": "echo",
  "description": "Returns the JSON arguments it was given.",
  "entry": "echo.wasm",
  "inputSchema": { "type": "object" }
}
```

| Field | Required | Rules |
| --- | --- | --- |
| `name` | yes | 1–40 chars of `a-z`, `0-9`, `-`, `_`; the model calls this name |
| `description` | yes | ≤ 400 chars; the model chooses by it |
| `entry` | no | module path; defaults to the manifest's `entry` |
| `inputSchema` | no | JSON Schema object ≤ 8 KiB, used to validate arguments before the call |

Each tool runs as its own sandboxed module invocation. Calling one requires an
`extension_tool` allow rule in Permissions, because a call has no prompt in which
to ask.

## Panels

```json
{ "id": "json", "title": "JSON formatter", "entry": "json.html" }
```

`entry` must be a `.html` file inside the folder and must exist. A panel is
served under `default-src 'none'; style-src 'unsafe-inline'; script-src
'unsafe-inline'; img-src data:; font-src data:; media-src data:; form-action
'none'; base-uri 'none'; frame-ancestors 'self'` inside a sandboxed iframe with
`allow-scripts` only. It can draw and compute; it cannot reach the app, its API,
the network, the filesystem or your projects.

## Provider presets

```json
{
  "id": "ollama-local",
  "name": "Ollama (local)",
  "baseUrl": "http://127.0.0.1:11434/v1",
  "model": "llama3.2",
  "keyUrl": "https://ollama.com/download",
  "note": "Runs on this machine; no API key is needed."
}
```

`baseUrl` must be `https`, or `http` on a loopback host. It may not contain
credentials, a query or a fragment. A preset is declarative: the app still makes
the request and holds the key, so an extension can point Studio at a gateway
without ever seeing a secret. Presets from a disabled or incompatible extension
are not offered.

## Install limits

| Limit | Value |
| --- | --- |
| files per extension | 500 |
| total bytes per extension | 16 MiB |
| `panels[].entry` | must be `.html` and ≤ 512 KiB |

## Validate before shipping

```sh
curl -s localhost:8765/api/plugins/validate \
  -H 'content-type: application/json' \
  -d "{\"path\":\"/path/to/hello-studio\"}" | jq
```

`POST /api/plugins/validate` accepts either a folder `path` (checks the manifest,
the entry file and every panel file) or an inline `manifest` object with an
optional `folder` name. It is the installer's own parser, so a manifest that
validates here installs there.
