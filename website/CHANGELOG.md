# Website changelog

## 0.5.2

### Fixed

- Start commands and data paths name the binaries and files that actually ship:
  `free-ai-studio --browser` instead of `SAARTHI-STUDIO --browser`, and
  `~/.config/free-ai-studio/config.json` plus
  `~/.local/share/free-ai-studio/history.sqlite3` instead of uppercase paths
  and `studio.db`. The old documentation told people to run a command that
  does not exist and look in files the app never writes.

## 0.5.1

### Fixed

- Download data is pinned to the real **v3.83.0** release and names the assets
  that release actually published: `free-ai-studio_3.83.0-1_amd64.deb`,
  `sarthi-studio_3.83.0_x86_64.AppImage`, `…linux-x86_64.tar.gz`,
  `free-ai-studio-android.apk`, `…android.aab`, `free-ai-studio-vscode.vsix`
  and `SHA256SUMS.txt`. The old snapshot linked uppercase `SAARTHI-STUDIO_*`
  names, so every binary download returned 404.
- v3.83.0 has no Windows or macOS builds. Those entries are empty and their
  pages explain that the platform is not attached to this release instead of
  linking a missing file; the banner count and release summary are computed
  rather than hardcoded.
- Install commands use the binary inside the packages
  (`/usr/bin/free-ai-studio` in the deb, `free-ai-studio` in the tarball)
  through a single `EXECUTABLE_NAME` constant. Docs pages (FAQ, installation,
  configuration, getting started, troubleshooting, privacy, developer guide)
  use it too, and the VS Code install line names the real VSIX file.
- `website/README.md` documents the hand-checked release snapshot rule.

Verified: `npm run check` 0 errors, `npm run build` 37 pages,
`npm run check:build` resolves every local reference, and every generated
`releases/download` URL answers 200.
