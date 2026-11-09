# ADR 0001 — Extension execution is WASM-first and capability-brokered

- **Status:** accepted; implementation started — compute-only runner in 2.20.0, workspace host functions in 2.21.0, per-extension data in 2.22.0, run control in 2.23.0 (see the implementation notes below)
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

## Implementation note (2.20.0) — compute-only first slice

The first runner slice revises three points above. Each revision narrows the
sandbox rather than widening it, so nothing promised to a user is weaker than
the ADR says.

1. **wasmi, not Wasmtime.** The app declares `rust-version = "1.80"`; Wasmtime
   requires a much newer compiler and pulls a C toolchain on some targets. The
   runner is **wasmi 0.31.2**, a pure-Rust interpreter pinned with `=` so a
   transitive update cannot change sandbox semantics. It is an interpreter, so
   it is slower than a JIT — acceptable for short computations, and it means no
   executable memory is generated. Binary size grows; that was the ADR's
   accepted cost and it still holds.

2. **No host functions at all in the first slice.** Point 5's `workspace.read`
   and `workspace.write` are **not** provided yet: the runner links an empty
   linker and refuses any module that imports something. `V1_PERMISSIONS` is
   therefore empty, and a manifest requesting *any* permission is refused at run
   with the permission named. This is points 2 and 4 honoured literally: an
   extension can only compute and return an `i32` exit code. The workspace host
   functions are the next gate, and they arrive with their own path-scoping and
   audit tests (gates 3 and 5).

3. **Fuel replaces the wall-clock cap for v1.** `consume_fuel(true)` with a
   50,000,000-unit budget, plus a 64 MiB memory ceiling and a 100,000-element
   table limit enforced by a `ResourceLimiter`, and an 8 MiB module size cap.
   Fuel is deterministic: the same module stops at the same instruction on every
   machine, which a wall-clock kill cannot promise. A time-based backstop is
   still worth adding when host calls (and therefore I/O waits) exist.

The ABI in this slice is exactly: export `run() -> i32`; no imports; the return
value is the exit code. A missing export, an import, a trap and an exhausted
fuel budget each produce a distinct message, and the run route answers 400 with
that message. Success/failure and the fuel used are written to the oplog under
the `extensions` area.

Gates from the list above that this slice satisfies: 1 (runtime validated),
2 (refusal with a test per refused permission), 4 (fuel and memory limits
proven by tests), 6 (no run action for a non-runnable extension) and partially 7
(nothing can be running during an uninstall, because no instance outlives one
request). Gate 3 and the audit-per-host-call part of gate 5 stayed open at that point;
both close in the 2.21.0 note below.

## Implementation note (2.21.0) — workspace host functions

Points 3, 4 and 5 of the decision are now implemented for read and write.

**The whole ABI.** A module may import only these, and only when the manifest
does not lack the permission:

```text
env.read(path_ptr, path_len, out_ptr, out_cap) -> i32   bytes written, or < 0
env.write(path_ptr, path_len, data_ptr, data_len) -> i32  0, or < 0
env.last_error(ptr, cap) -> i32                         message bytes written
```

Error codes are part of the contract: `-1` refused, `-2` invalid path, `-3`
missing, `-4` too large, `-5` not UTF-8 text, `-6` I/O failure. `last_error`
exists so a module can surface *why* without the host printing into it. A module
that imports anything else — including `read` when the manifest lacks
`workspace.read` — fails to instantiate with the import named, rather than
being linked to a stub that always refuses.

**Path scoping reuses the app's own resolver.** `workspace::resolve_inside`
rejects absolute paths, `..`, and symlink escapes after canonicalisation, and
requires an existing parent for a new file; writes go through
`workspace::write_text_file`, which is atomic and size-capped. An extension
cannot reach the database, `config.json`, provider keys, the extension's own
code folder, or another extension's folder through these functions.

**Rules are enforced per call, but `ask` means no.** Each call resolves
`fs_read`/`fs_write` for the target path against the active project, exactly as
the agent does. `allow` runs the call; `deny` refuses it. `ask` also refuses —
synchronously, with no UI to answer — and says so in `last_error` and in the
audit trail. This follows point 3 ("applies the same permission rules the agent
already obeys") and the deny-by-default stance: a manifest permission is what
makes a capability *available*, not what makes a destructive action *approved*.
Reads default to allow and writes to ask, so a write needs a saved allow rule.

**Limits and audit.** Fuel still bounds computation; I/O is bounded separately
at 64 host calls and 1 MiB per read or write (1 KiB per path), because host
calls are not metered by gas. Every call appends `{capability, path, decision,
scope, detail}`; the caller writes allows and refusals to the process log under
`extensions`/`host`, and refusals to the permission audit table so they appear
in Settings. The runner itself never touches the database, so it stays testable
without an AppState.

Gate 3 (path-scoping tests) and the audit half of gate 5 are now covered by
unit tests for escape, deny, ungranted import and write-requires-allow.

## Implementation note (2.22.0) — the per-extension data folder

Point 7 is implemented as a second pair of host functions rather than a wider
workspace root:

```text
env.data_read(name_ptr, name_len, out_ptr, out_cap) -> i32
env.data_write(name_ptr, name_len, data_ptr, data_len) -> i32
```

They are rooted at `<data>/extensions/<id>/data`, so the module's code folder
(the parent) and every other extension are outside the resolvable tree. The
folder is created on the first **approved** write, not at install: a manifest
cannot make the host create directories by merely asking to write. Reads of a
missing folder return `-3` like a missing file.

Names are resolved with the same `resolve_inside` containment, so `..`, an
absolute name and a symlink escape are refused exactly as for workspace paths;
writes reuse `write_text_file`. There is no directory listing and no delete:
the folder holds small state, not a filesystem.

**Gating.** No new permission name was invented — the closed vocabulary stays
five entries. `data_read` needs `workspace.read` and `data_write` needs
`workspace.write`, the same declaration that makes the workspace pair available,
and the same saved `fs_read`/`fs_write` rules apply to the name. A write still
defaults to `ask`, so it needs a saved allow rule; that is deliberate
unattended-write policy, not a gap. Calls are audited as `data.read` and
`data.write`, and count against the same 64-call, 1 MiB-per-call budget.

## Implementation note (2.23.0) — stopping a run

Point 6 asked for "a fuel budget and a wall-clock cap"; fuel arrived in 2.20.0,
the wall clock here.

**The wall clock is checked at host boundaries, not inside the interpreter.**
wasmi offers no way to interrupt a running instance from another thread, and
pretending otherwise would be a false promise. A 5-second budget is therefore
compared against `Instant::now()` at the start of every host function; once it
has passed, the call is refused, the reason is recorded, and the run's result is
discarded in favour of `Err("The run exceeded its 5-second wall-clock
budget.")`. Computation remains bounded by fuel, which is deterministic.

**Cancellation uses the same boundary.** `POST /api/extensions/:id/cancel` sets
an `AtomicBool` shared with the run; the same host-boundary check notices it and
stops the run with `"The run was cancelled."`. One run per extension is allowed
(a second request answers 409), so the cancel names exactly one run and a stuck
module cannot accumulate blocking threads; the registry entry is removed when
the request returns, including on error.

**Uninstall stops a run first.** Gate 7 is now satisfied: removing an extension
cancels a run in flight before moving the folder. The module bytes were loaded
before execution began, so the run ends at its next host call instead of reading
from a folder that just moved.

**The honest limit:** a module that performs no host calls cannot be cancelled
or time-capped mid-computation; it ends when fuel runs out, and the route
reports that fuel stop. This is stated in the UI copy and in `KNOWN-ISSUES.md`
rather than implied away.

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
