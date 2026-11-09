# ADR 0001 — Extension execution is WASM-first and capability-brokered

- **Status:** accepted (design); implementation not started
- **Date:** 2026-11-09
- **Deciders:** project maintainers
- **Supersedes:** none

## Context

Phase 15 shipped a manifest, discovery, a Settings panel and an install/remove
path (2.15.0–2.17.0). None of that executes extension code, and the docs say so
plainly. Permission names such as `workspace.write` are declarations today, not
enforced capabilities.

The app is a single Rust binary on Linux, Windows and macOS, with SQLite, an
open-workspace concept, a terminal hub, MCP servers, provider keys in
`config.json` and an existing permission/audit system. It cannot assume Node,
Docker, a container runtime or administrator rights. A user installs an
extension by pointing at a folder; the folder is untrusted from that moment,
including when its author is the user.

Assets an extension could reach if execution were added carelessly:

- the open workspace and every file the user can read;
- the SQLite database (conversations, agent runs, usage);
- `config.json`, including provider and search API keys;
- the terminal, and therefore arbitrary code execution;
- the network, with the host's identity and any ambient credentials;
- MCP servers and their tools;
- other extensions' folders and data.

A loader that runs extension code in-process would hand over all of that at
once. This ADR exists so that decision is made before a loader, not after.

## Decision

1. **Never execute extension code in the app's process.** No `eval`, no
   in-process JS engine, no dynamic library loading.

2. **The v1 runner is WebAssembly** (Wasmtime, embedded). WASI is **disabled**:
   no ambient filesystem, network, environment, clock or randomness. The module
   receives only what a host function explicitly returns.

3. **Every capability is a host function.** There is no capability a module can
   obtain on its own. Each host call:

   - checks the manifest grants the required permission;
   - applies the same permission rules the agent already obeys (deny/ask/allow,
     prefix rules, project/session scope);
   - validates its arguments (paths resolved and canonicalised, sizes bounded);
   - records an audit entry with the extension id, the capability, the target
     and the decision.

4. **Deny by default, at two points.** An unknown permission still fails
   installation (2.15.0 behaviour). A permission that exists but has no v1 host
   function fails the **run** before instantiation, naming the permission — it
   is never silently ignored.

5. **v1 capability surface** (v1 = first runner release):

   | Manifest permission | v1 behaviour |
   |---|---|
   | `workspace.read` | host function, scoped to the open workspace; `..` and symlink escapes refused after canonicalisation |
   | `workspace.write` | as read, plus the permission rules; writes are atomic and size-capped |
   | `network` | **no host function in v1**; a manifest requiring it installs but refuses to run |
   | `terminal.run` | **no host function in v1**; refused at run |
   | `mcp.call` | **no host function in v1**; refused at run |

   Nothing in the v1 surface can reach config, the database, provider keys, the
   terminal or the network.

6. **Resource limits are part of the boundary.** Each run has a fuel budget and
   a wall-clock cap, a fixed memory ceiling, a bounded output and a bounded
   number of host calls. Exceeding any of them stops the instance and records a
   failure; there is no persistent process between runs.

7. **Per-extension data directory.** An extension may read/write only
   `<data>/extensions/<id>/data` through the same host functions, and only when
   it holds the matching permission. Its code folder is read-only to it.

8. **Manifest shape.** Execution adds `runtime: "wasm"` and requires `entry` to
   point at a `.wasm` file. A manifest without `runtime` keeps today's meaning:
   installable, not runnable. Existing `entry: "main.js"` extensions stay
   installable and are reported as **not runnable yet**, not as broken.

9. **The UI never implies a capability it does not have.** A run control appears
   only for a runnable extension, and the refusal reason (unsupported
   permission, missing runtime, invalid module) is shown where the action would
   be.

## Consequences

- A new dependency (Wasmtime) and a larger binary. That cost is accepted because
  it buys memory isolation and the removal of ambient authority on all three
  platforms with one implementation.
- JavaScript extensions are deferred. A JS runtime must itself run inside the
  sandbox (for example a JS engine compiled to WASM) before `main.js` can be
  executed; until then those manifests remain installable but not runnable.
- Extensions cannot use the filesystem or network directly. Everything goes
  through host functions, which is slower but auditable and testable.
- The permission vocabulary in `app/src/extensions.rs` becomes a real contract,
  not a label.

## Alternatives rejected

- **In-process JavaScript or native code.** No isolation; one bug is full
  access to keys and the database.
- **A Node child process with an inherited environment.** Node is not guaranteed
  on the machine, is not a sandbox, and inherits the environment and filesystem.
- **OS containers or Docker.** Not available on a stock desktop install, and
  adds a second runtime the user must maintain.
- **OS-specific sandboxes alone** (AppContainer, `sandbox-exec`, seccomp/landlock).
  Useful as a second layer later, but three implementations with different
  semantics cannot be the only boundary.
- **Static analysis of extension code.** Not a security boundary.

## Implementation gates

A loader is not merged until all of these exist and are tested:

1. `runtime` in the manifest, validated; non-WASM entries never reach a loader.
2. A run route that refuses unsupported permissions before instantiation, with a
   test for each refused permission.
3. Path-scoping tests for `workspace.read`/`write`: `..`, absolute paths,
   symlinks and the extension's own code folder.
4. Fuel/timeout and memory-limit tests that prove a runaway module stops.
5. Audit tests: every host call and every refusal is recorded with the extension
   id.
6. A UI state that never shows a run action for a non-runnable extension.
7. An uninstall that stops a running instance before moving the folder.

## Open questions

- Trust and signing: should an extension need a signature, or is a local folder
  the user chose sufficient for v1?
- Does `network` ever ship, and if so as a host-mediated allowlist or not at all?
- Can `terminal.run` be mediated safely enough to exist, or should extensions
  never have it?
- Is there a second OS-level layer worth adding under Wasmtime?
- When a sandboxed JS runtime becomes available, does `main.js` get a
  compatibility window or a hard cutover?
