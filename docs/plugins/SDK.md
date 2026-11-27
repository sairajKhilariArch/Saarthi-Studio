# Plugin SDK reference

An extension is a WebAssembly module plus a manifest. There is no SDK package to
install and no JavaScript runtime in the sandbox: the host surface is ten
functions, and the module is plain wasm you can produce from any language that
targets it.

## Module contract

- Export `memory` (the host reads and writes through it).
- Export `run` as `() -> i32`. The value is reported as the run's exit code. A
  tool that called `env.result` is successful even when that number is positive —
  the shipped example returns the number of bytes it echoed. A negative value, or
  a tool call that never reports a result, is a failure the run records.
- Import only the `env` functions you need. An import this run was not granted
  fails before instantiation with a sentence naming the missing capability.

## Host functions

Every pointer/length pair refers to the module's own linear memory. Functions
that return an `i32` return the number of bytes written into an output buffer,
`0` when there is nothing to write, or a negative error code.

| Import | Signature | Behaviour |
| --- | --- | --- |
| `env.read` | `(path_ptr, path_len, out_ptr, out_cap) -> i32` | reads a file inside the granted root; needs `workspace.read` and an `fs_read` rule |
| `env.write` | `(path_ptr, path_len, data_ptr, data_len) -> i32` | writes a file inside the granted root; needs `workspace.write` and an `fs_write` rule |
| `env.data_read` | `(path_ptr, path_len, out_ptr, out_cap) -> i32` | reads from this extension's own `data/` folder |
| `env.data_write` | `(path_ptr, path_len, data_ptr, data_len) -> i32` | writes to this extension's own `data/` folder |
| `env.arguments` | `(out_ptr, out_cap) -> i32` | the tool call's JSON arguments; empty for a plain run |
| `env.config` | `(out_ptr, out_cap) -> i32` | this plugin's saved settings as JSON (`{}` when none) |
| `env.result` | `(ptr, len) -> i32` | the tool's one answer; a second call is refused |
| `env.last_error` | `(out_ptr, out_cap) -> i32` | why the last host call failed |
| `env.fetch` | `(req_ptr, req_len, out_ptr, out_cap) -> i32` | brokered HTTP; needs `network` and a `network` rule |
| `env.command` | `(cmd_ptr, cmd_len, out_ptr, out_cap) -> i32` | one shell command in the open folder; needs `terminal.run` and a `terminal` rule |
| `env.mcp_call` | `(req_ptr, req_len, out_ptr, out_cap) -> i32` | one configured MCP tool; needs `mcp.call` and an `mcp` rule |

`env.arguments`, `env.config`, `env.result` and `env.last_error` need no
permission: they are the shape of a call, not access to anything.

### `env.fetch` request and answer

```json
{ "url": "https://api.example.com/v1/thing", "method": "GET", "headers": { "accept": "application/json" }, "body": "" }
```

`url` must be absolute `http(s)` with no credentials. `method` is one of `GET`,
`POST`, `PUT`, `PATCH`, `DELETE`, `HEAD` (default `GET`). At most 32 headers and
a bounded body. Redirects are not followed. The answer is JSON:

```json
{ "status": 200, "content_type": "application/json", "body": "…" }
```

### `env.mcp_call` request

```json
{ "server": "docs", "tool": "search", "arguments": { "query": "wsgi" } }
```

The answer is the MCP tool's JSON result. The broker owns the server's command,
URL, headers and token; the module only names the tool.

### Error codes

| Code | Meaning |
| --- | --- |
| `0` | nothing to return |
| `> 0` | bytes written into the output buffer |
| `-1` | the run was cancelled, is out of wall-clock time, or the call budget is spent |
| `-2` | the request was malformed (bad JSON, bad URL, wrong type) |
| `-4` | a buffer or size limit was exceeded |
| `-6` | the operation failed at the boundary (network error, spawn error) |

After a negative return, call `env.last_error` for the sentence to log or show.

## A complete tool, in WAT

```wat
(module
  (import "env" "arguments" (func $arguments (param i32 i32) (result i32)))
  (import "env" "result" (func $result (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (local $len i32)
    (local.set $len (call $arguments (i32.const 0) (i32.const 4096)))
    (drop (call $result (i32.const 0) (local.get $len)))
    (local.get $len)))
```

```
wat2wasm echo.wat -o echo.wasm
```

The same module in Rust, with no external crate:

```rust
#[link(wasm_import_module = "env")]
extern "C" {
    fn arguments(out: *mut u8, cap: i32) -> i32;
    fn config(out: *mut u8, cap: i32) -> i32;
    fn result(ptr: *const u8, len: i32) -> i32;
}

#[no_mangle]
pub extern "C" fn run() -> i32 {
    let mut buffer = [0u8; 4096];
    let len = unsafe { arguments(buffer.as_mut_ptr(), buffer.len() as i32) };
    if len < 0 {
        return len;
    }
    unsafe { result(buffer.as_ptr(), len) };
    len
}
```

Build with `cargo build --target wasm32-unknown-unknown --release` and rename the
artifact to the name your manifest's `entry` or `tools[].entry` uses.

## UI panels

A panel is one HTML file, rendered in an iframe with `sandbox="allow-scripts"`
under a `default-src 'none'` policy. Inline CSS and inline `<script>` are allowed;
external scripts, fonts, images, network requests, forms and top-level navigation
are not. A panel can compute and draw — a formatter, a chart from pasted data, a
reference card — but it has no bridge to the app, its API, your projects or your
settings.

## Settings

`env.config` returns the JSON a person saved for that plugin on the Plugins page.
It is a flat object of strings, numbers and booleans, at most 32 keys and 16 KiB.
Use it for an endpoint, a region, a project name — data, not capabilities.
Settings are handed over as a snapshot at the start of each run.

## Validation

```sh
curl -s localhost:8765/api/plugins/validate -H 'content-type: application/json' \
  -d '{"path":"/path/to/hello-studio"}'
```

Answers `{ "ok": true, "valid": bool, "errors": [...], "warnings": [...],
"compatible": bool, "manifest": { ... } }`. Inline manifests work too:

```sh
curl -s localhost:8765/api/plugins/validate -H 'content-type: application/json' \
  -d '{"folder":"hello-studio","manifest":{"id":"hello-studio","name":"Hello","version":"0.1.0"}}'
```
