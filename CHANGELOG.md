# Changelog

## 4.1.0

### Plugins
- Add a **Plugins** page: one catalog over extensions (wasm tools, HTML panels,
  provider presets) and skills, with enable/disable, per-extension settings,
  declared permissions, panels and removal in one place.
- Enforce enablement instead of hiding it: a disabled extension is not advertised
  to the model, refuses direct runs, tool calls and panel requests, and stops
  contributing provider presets.
- Add per-plugin settings, validated as a flat bounded object and delivered to
  the sandbox through the new `env.config` host call.
- Add a marketplace over registry indexes a person adds. There is no bundled
  catalog: a registry must be HTTPS (or loopback), every download is checked
  against its declared SHA-256, archives are extracted with traversal and size
  limits, and the manifest id must match the chosen listing.
- Add `POST /api/plugins/validate`, which runs the installer's own manifest
  parser against a folder or an inline manifest.
- Add `GET /api/skills/:id/export`, so a skill can be copied or committed as the
  exact `SKILL.md` it is.
- Add a working example plugin (`examples/plugins/hello-studio`) with two
  sandboxed tools, a JSON formatter panel and an Ollama preset, plus a test that
  installs it and runs both tools.

### Documentation
- Add `docs/plugins/`: architecture, manifest specification, permissions and
  security, marketplace, SDK reference, developer guide and the example
  walkthrough.

## 4.0.0

### Reliability
- Reject malformed JSON from successful router responses instead of converting it into a fake assistant answer.
- Reject successful JSON responses that do not contain supported chat message content.
- Return a clear gateway error for malformed one-shot streaming responses.
- Add regression tests for invalid JSON, missing content, plain text and structured text parts.

### Release
- Start the Sarthi Studio desktop 4.x major release line.
- Keep Android, VS Code and website versioning independent.
- Move historical 3.x release notes to [the archived changelog](docs/release-history/CHANGELOG-3.x.md).

## Previous releases

Historical 3.x release notes are preserved in [CHANGELOG-3.x.md](docs/release-history/CHANGELOG-3.x.md).
