# Plugin permissions and security

An extension never gets unrestricted access. It declares what it needs, a person
saves rules for it in **Settings → Permissions**, and every host call is checked
against those rules at the moment it is made and written to the audit trail.

## The closed vocabulary

| Permission | Grants | Gate |
| --- | --- | --- |
| `workspace.read` | `env.read` inside the open folder and the extension's own `data/` | `fs_read` rule for the path |
| `workspace.write` | `env.write` inside the open folder | `fs_write` rule for the path |
| `terminal.run` | `env.command` | `terminal` rule for the command |
| `network` | `env.fetch` to one host and path | `network` rule for the URL |
| `mcp.call` | `env.mcp_call` for one configured server tool | `mcp` rule for `server.tool` |
| — (no permission) | `env.arguments`, `env.config`, `env.result`, `env.last_error` | always available |

An unknown permission name fails validation. Silently dropping a capability
request is how a person ends up believing an extension cannot do something that
it can.

Declaring a permission is necessary but not sufficient: the rule must allow the
specific target, and an extension cannot answer an approval prompt, so an `ask`
rule refuses the call.

## What is not possible

- **No ambient access.** The module runs in wasmi with no imports other than the
  host functions above. There is no filesystem, clock, environment or socket in
  its address space.
- **No escape from the folder.** A path is resolved against the granted root and
  rejected if it leaves it; symlinks are resolved before the check.
- **No network by default.** `env.fetch` follows no redirects, refuses non-http(s)
  schemes, and is refused unless the rule covers the host.
- **No secrets.** The app never hands an API key, token or config value to a
  module. `env.config` returns only the plugin's own saved settings.
- **No app API.** A module cannot call `http://127.0.0.1:<port>/api/...`; the
  sandbox has no HTTP client, and `env.fetch` is a brokered request with its own
  rules.
- **No cross-plugin access.** The data root handed to a run is that extension's
  own `data/` folder.
- **Panels cannot act.** A panel is HTML with no bridge and a `default-src
  'none'` CSP: no network, no app access, no forms, no top-level navigation.

## Enablement is part of the security model

Disabling an extension removes its tools from the model's catalog, refuses runs
and tool calls, stops its provider presets and stops serving its panels. It is
the off switch for a plugin whose permissions you no longer trust, short of
removing it.

## Audit

Every host call that reached the sandbox is recorded with capability, path or
host, decision and detail; every run is recorded in `extension_runs` with exit
code, fuel used, host calls and any error. Enable/disable changes, settings
changes and marketplace installs are written to the app's operational log.

## Skills

A skill is prompt text, not code. Its `permissions` declaration is shown in the
UI and repeated in the prompt so the model knows what the instructions assume,
but it grants nothing: a skill cannot execute anything by itself. Anything a
skill asks the agent to do still goes through the agent's own approvals.

## Marketplace hardening

- A registry must be `https`, or `http` on loopback during development.
- Every listing must carry a 64-hex SHA-256; a download that does not match is
  refused before anything is unpacked.
- The archive is extracted with traversal checks (`..`, absolute paths, drive
  letters, backslashes are refused), a 500-file limit and a 16 MiB expanded
  limit.
- The archive's manifest id must equal the id that was chosen, so a registry
  cannot substitute a different plugin.
- Valid but incompatible manifests are refused by the installer, which removes
  the staged copy.
- Installing is gated by the app's `fs_write` permission rule.

## Known limits

- wasmi is an interpreter, not a process sandbox: a bug in the interpreter or in
  this host layer is the boundary. Memory is capped at 64 MiB, fuel at
  50,000,000, wall clock at 5 s and host calls at 64.
- Wall-clock limits are checked at host-call boundaries, so a module that
  computes without any host call ends at its fuel limit instead.
- `inputSchema` validates tool arguments before the call; a module that mishandles
  a valid argument is the module's own bug, not a sandbox escape.
- Signed plugins are not implemented. Verify a plugin's source before installing
  it from a registry you do not control.
