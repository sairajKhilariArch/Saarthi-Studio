# Sarthi Studio — Phase 8 Linux Validation

**Date:** 2026-11-25
**Build:** 3.88.12
**Branch:** `platform/phase-8-cross-platform-2026-11-25`
**Host:** Arch Linux, kernel 7.3.5-arch1-1, x86_64

## Status summary

| State | Result |
|---|---|
| Implemented | Yes |
| Built | Headless release binary and `.deb` built locally; AppImage/tarball are built by `desktop-release.yml` |
| Installed | **Not system-installed.** The `.deb` payload was unpacked exactly as a package install lays it out and executed from that tree; `dpkg -i` was not run (no dpkg/root on this Arch host) |
| Executed | Yes, from the packaged layout |
| Tested | Startup, UI assets, data paths, permissions, HTTP security, restart, database |
| Blocked | System package installation; AppImage runtime; RPM and Arch packages; GUI window (headless build used) |

## Defect found and fixed

The `.deb` manifest listed only `index.html`, `app.js` and `styles.css`, plus three skills and one icon. The UI also loads `ide.js`, `ide.css`, `highlight.js` and `replace.js`, and references brand assets. An installed 3.88.9 package would therefore boot to a page with no workbench, no syntax highlighting and missing branding.

- `app/Cargo.toml` now lists all 19 runtime files under `app/static`.
- `scripts/ci/test-deb-assets.sh` compares every non-test static file with the package manifest and fails if a new runtime file is omitted. It is part of Desktop CI.
- The rebuilt `.deb` was unpacked and every requested asset returned HTTP 200.

## Package evidence

```
artifact: app/target/debian/free-ai-studio_3.88.12-1_amd64.deb
size:     5.0 MB
payload:  usr/bin/free-ai-studio
          usr/share/sarthi-studio/static/{index,app,ide,highlight,replace}.{html,js,css}
          usr/share/sarthi-studio/static/brand/*.svg
          usr/share/sarthi-studio/static/skills/*/SKILL.md
          usr/share/applications/io.github.sairajkhilariarch.SarthiStudio.desktop
          usr/share/metainfo/io.github.sairajkhilariarch.SarthiStudio.appdata.xml
          usr/share/icons/hicolor/scalable/apps/sarthi-studio.svg
```

`cargo deb --no-build` ran on Arch without `dpkg-shlibdeps`, so it warned that `$auto` dependencies could not be resolved locally. The release workflow builds the authoritative `.deb` on Ubuntu 22.04 with dpkg present; that path was not executed in this local run.

## Runtime measurements

| Check | Result |
|---|---|
| `free-ai-studio --version` | `Sarthi Studio 3.88.12` |
| Startup to HTTP 200 | **89 ms** |
| Idle RSS after 2 s | **14,476 KB** |
| Idle CPU sample | **0.0 %** |
| Installed UI size | 5.0 MB package |
| Database created | 180,224 bytes, mode **0600** |
| Data directory | mode **0700** |
| Config path | `~/.config/free-ai-studio/config.json` |
| Data path | `~/.local/share/free-ai-studio/history.sqlite3` |
| Restart with same home | Server returned HTTP 200; database reused |
| Foreign `Origin` POST | **403** |
| Foreign `Host` GET | **403** |
| Security headers | CSP with `script-src 'self'`, `frame-ancestors 'none'`, `X-Frame-Options: DENY`, `nosniff`, `no-referrer` |

All of `index.html`, `app.js`, `ide.js`, `ide.css`, `highlight.js`, `replace.js`, `styles.css`, `brand/favicon.svg` and `skills/debug/SKILL.md` returned 200 from the installed tree.

## Distribution coverage

| Target | State |
|---|---|
| Arch Linux (host) | Source build and packaged layout tested; no Arch package produced |
| Ubuntu | Built in CI on `ubuntu-22.04` and `ubuntu-latest`; not installed on a real Ubuntu machine in this phase |
| Debian | No separate test; the `.deb` is Debian-format and built on Ubuntu 22.04 |
| Fedora | **Not tested** |
| AppImage | Built by the release workflow only; not executed locally |
| `.deb` | Built and payload-tested locally; system `dpkg -i` **blocked** |
| `.rpm` | **Not implemented** |
| Arch package | **Not implemented** |

## Desktop integration

- Desktop entry: present in the package, `Exec=free-ai-studio --browser`, `Categories=Development;`, icon installed under hicolor.
- AppStream metadata: version validated against 3.88.12 by `test-linux-metadata.sh` (9/9).
- File associations and deep links: not implemented in the application; nothing is advertised.
- Notifications: no desktop notification feature exists; terminal/router failures are reported in the UI and logs.
- Tray: not implemented.
- Credential storage: Secret Service through `keyring` (`sync-secret-service`); not exercised against a live keyring in this headless run.
- Terminal: real PTY via `portable-pty`; included in the 76-test E2E suite.
- GPU/local models: local model execution delegates to the configured router (Ollama/FreeLLMAPI); GPU use was not measured.

## Remaining Linux gaps

1. A real `dpkg -i` install/upgrade/uninstall must run on Ubuntu/Debian in a disposable VM or CI container with root.
2. AppImage launch and desktop integration need a graphical session.
3. RPM and Arch packages are not produced.
4. The Arch host has no `mold`, so local Cargo commands required the documented `lld` workaround; CI installs mold and uses the repository default.
