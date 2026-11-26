# Installer Validation Report — Phase 9

**Version:** 3.88.15
**Commit:** `0af8acb`
**Host:** Arch Linux x86_64, kernel 7.3.5 (no `dpkg`, no root, no Windows/macOS hardware)

## 1. Result matrix

| Platform | Package | Built | Installed | First launch | Upgrade | Uninstall | Data preserved |
|---|---|---|---|---|---|---|---|
| Linux | `.deb` 3.88.15 | **PASS** | **BLOCKED** (`dpkg` absent) | **PASS** from unpacked payload | **PASS** 3.88.3 → 3.88.15 from payloads | **BLOCKED** | **PASS** (simulated upgrade) |
| Linux | portable tarball | CI | Not installed | CI smoke | Not tested | N/A | N/A |
| Linux | AppImage | CI | Not installed | CI extract/boot | Not tested | N/A | N/A |
| Windows | zip | CI | **BLOCKED** | **BLOCKED** | **BLOCKED** | N/A | **BLOCKED** |
| Windows | MSI | CI (WiX 5) | **BLOCKED** | **BLOCKED** | **BLOCKED** | **BLOCKED** | **BLOCKED** |
| macOS | `.app.zip` / tar | CI | **BLOCKED** | **BLOCKED** | **BLOCKED** | **BLOCKED** | **BLOCKED** |
| Android | debug APK | CI | **PASS** (emulator) | **PASS** | Not tested | Not tested | N/A |
| Android | unsigned release APK/AAB | CI | **BLOCKED** (unsigned) | **BLOCKED** | N/A | N/A | N/A |

A package file is not treated as proof of installation. Only the rows marked
**PASS** were executed.

## 2. Linux `.deb`

### Build

```
cargo build --release --locked --no-default-features
cargo deb --locked --no-build
free-ai-studio_3.88.15-1_amd64.deb   5,233,568 bytes
SHA-256 686691cf762c43d4107a38b46071db1d014d0edf8f83c34313925c6529eed537
```

`dpkg-shlibdeps` is absent on Arch, so `$auto` dependency resolution was skipped
locally. The release workflow builds the authoritative `.deb` on Ubuntu 22.04,
where dpkg is present and the static dependency check runs.

### Payload

25 files, including:

- `usr/bin/free-ai-studio`
- `usr/share/applications/io.github.sairajkhilariarch.SarthiStudio.desktop`
- `usr/share/metainfo/io.github.sairajkhilariarch.SarthiStudio.appdata.xml`
- `usr/share/icons/hicolor/scalable/apps/sarthi-studio.svg`
- 19 runtime static files (`index.html`, `app.js`, `styles.css`, brand assets)
- three bundled skills under `usr/share/sarthi-studio/static/skills/`

The control file records `Version: 3.88.15-1`, `Architecture: amd64` and a
description stating that upgrades replace binaries/UI under `/usr` while home
data is preserved. There are **no maintainer scripts** (`preinst`, `postinst`,
`prerm`, `postrm`), so package removal does not touch user data.

### Fresh payload boot

The package was unpacked with `ar` + `tar` and run from its installed layout
with a scratch HOME:

```
Sarthi Studio 3.88.15
/                      200
/static/app.js         200
/static/styles.css     200
/static/index.html     200
/api/ready             200
```

`/api/ready` reported database, schema, data directory, provider, routers and
MCP checks healthy. Startup was not re-timed in this phase; Phase 8 measured
89 ms to HTTP 200 and 14,476 KB RSS for the same headless shape.

`smoke-package.sh` itself requires `dpkg-deb`, so it correctly failed on this
host (`FAIL dpkg-deb is needed to smoke a .deb`); the equivalent payload
inspection and boot were performed manually. CI has dpkg and runs the script.

### Upgrade 3.88.3 → 3.88.15

The 3.88.3 `.deb` payload and the 3.88.15 payload were extracted side by side.
The older binary created a conversation in a scratch HOME; the newer binary was
then started over the same HOME:

```
old_version=Sarthi Studio 3.88.3
create=201 {"id":"...","title":"phase9 upgrade probe"}
db_before=ab2989420fe6080473b27d4ff64da89cf15ad9b9ef41f63643a2e0ddd2b9d2da size=176128
new_version=Sarthi Studio 3.88.15
before_items 1 after_items 1
UPGRADE_DATA_PRESERVED
config_marker={"marker":"preserve-me"}
db_after_size=180224
ready_checks [('database', True), ('schema', True), ('data directory', True), ('provider', True), ('routers', True), ('mcp', True)]
```

The conversation survived, an unrelated config file survived, and the schema
migrated successfully. This is a **binary replacement over the same home**, not
a `dpkg` upgrade; the package-manager path is still blocked.

### Uninstall

There are no maintainer scripts and the database/config live under
`$XDG_DATA_HOME`/`$XDG_CONFIG_HOME`, outside `/usr`. Therefore removing the
package cannot remove conversations by design. Actual `dpkg -r` and
reinstallation were **not run** because dpkg/root are unavailable.

## 3. Windows

The CI job stages the same tree, builds a portable zip and a WiX 5 MSI, then
runs `smoke-package.sh` on each and an administrative MSI extraction on Windows.
`app/packaging/windows/installer.wxs` defines a per-user install under
`%LOCALAPPDATA%\Programs\Sarthi Studio`, a fixed UpgradeCode with MajorUpgrade,
a Start-menu shortcut and removal of the install directory on uninstall.

**BLOCKED:** install, first launch, restart, upgrade from an older MSI,
failed-install recovery, uninstall, reinstall and data preservation on real
Windows. No Windows hardware is available. CI's MSI smoke is not a substitute.

## 4. macOS

CI builds a universal binary with both `arm64` and `x86_64` slices, packages the
headless tarball and a `.app.zip`, and boots the packaged payload. The bundle is
**unsigned and not notarized**; Gatekeeper will require an explicit Open.

**BLOCKED:** install, first launch, restart, upgrade, uninstall and data
preservation. No Mac is available.

## 5. Android

Phase 8/Android CI installed and launched the **debug** APK on an API 35 x86_64
emulator and passed 6 instrumentation tests. The release APK/AAB are unsigned
diagnostics; `smoke-apk.sh` validates their manifest/dex/resources but does not
claim installability.

**BLOCKED:** release install (needs a release keystore), physical-device
validation, upgrade/uninstall/data-preservation matrix.

## 6. User-data guarantees

- Linux: user data is outside the package; no maintainer script deletes it.
- Windows: per-user install directory; uninstall component does not target
  `%APPDATA%`/`%LOCALAPPDATA%` data paths.
- macOS: app bundle does not own the data directory; unsigned bundle can be
  replaced without touching home data.
- Android: `allowBackup="false"`; app data is removed only by Android's own
  uninstall behavior, not by the update engine.

The Linux upgrade probe is the only executed proof of preservation. The other
platforms are design review plus CI packaging, and are labelled **BLOCKED**.
