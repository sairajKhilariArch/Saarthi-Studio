# Sarthi Studio — Phase 8 Installer and Update Validation

**Date:** 2026-11-25
**Build:** 3.88.12

## Artifact matrix

| Platform | Artifact | Built | Installed | Launched | Verified |
|---|---|---|---|---|---|
| Linux x86_64 | `.deb` | **Yes, locally** (5.0 MB) | Payload unpacked, not `dpkg -i` | Yes, from package layout | Assets, version, DB, permissions |
| Linux x86_64 | portable tarball | Release workflow | No | No | Script exists |
| Linux x86_64 | AppImage | Release workflow | No | No | Script exists |
| Windows x86_64 | portable zip | CI workflow | No | No | Packaged via PowerShell staging |
| Windows x86_64 | MSI (WiX v5) | CI workflow; packager unit test in Desktop CI | No | No | Per-user, MajorUpgrade, uninstall component |
| macOS universal | tarball + `.app.zip` | CI workflow (Intel runner, lipo) | No | No | Bundle layout/Info.plist checks |
| Android | debug APK | **Yes, CI** | **Emulator** | **Yes** | Instrumentation passed |
| Android | unsigned release APK/AAB | **Yes, CI** | No | No | R8 + shrink; signing deferred |

A package file existing is not treated as an install test. Only the Android debug APK was actually installed and launched (on the CI emulator), and the Linux `.deb` payload was booted from an unpacked install layout.

## Linux `.deb`

- Payload now includes **all 19 runtime UI files**, not the three that caused a broken installed UI.
- Installed-layout boot: `Sarthi Studio 3.88.12`, all tested assets HTTP 200, database 0600, data dir 0700.
- `cargo deb` on this Arch host lacked `dpkg-shlibdeps`, so `$auto` dependency resolution was skipped locally. The release workflow builds on Ubuntu 22.04 with dpkg available; that is the authoritative package build.
- Not run: `dpkg -i`, upgrade over an older installed version, `dpkg -r`, and preservation of `~/.config`/`~/.local/share` across package upgrade. **Blocked** by the absence of dpkg/root here.

## Windows MSI

`app/packaging/windows/installer.wxs`:

- Per-user install under `%LOCALAPPDATA%\Programs\Sarthi Studio`, no elevation.
- Fixed `UpgradeCode` + `MajorUpgrade`, so a newer MSI replaces the previous install.
- Start-menu shortcut component is the key path and is removed on uninstall.
- `Files Include="!(bindpath.stage)\**"` installs the whole staged tree, so new UI files do not require an MSI edit.
- Uninstall removes the install directory and shortcut, not the user's home data.
- `scripts/ci/test-installer.sh` validates refusal/acceptance rules; on this Linux host it **skipped** because WiX is unavailable. Desktop CI's Windows job installs WiX and runs the packager test.

**Not verified:** actual MSI install, first launch, restart, upgrade from 3.88.9, uninstall, data preservation. No Windows machine is available; **BLOCKED**.

## macOS bundle

- `bundle-macos-app.sh` produces `Sarthi Studio.app` with binary in `Contents/MacOS`, UI in `Contents/Resources/static`, `Info.plist` version and identifier, and a zip.
- **No code signing, hardened runtime, entitlements or notarization.** The release workflow documents this explicitly; Gatekeeper will not treat the bundle as a trusted download.
- **BLOCKED:** signing identity, notarization credentials, Gatekeeper install/launch.

## Android signing

- Debug APK is signed by the Android debug key and is installable (used by the emulator run).
- Release APK/AAB are **unsigned** by design; the previous debug-key signing was removed because it contradicted the artifact name and is not a release identity.
- **BLOCKED:** production keystore, `apksigner` verification, Play upload.

## Update engine

Implemented (`app/src/update.rs`):

1. **Version detection:** reads the GitHub release feed and compares semantic versions.
2. **Asset selection:** exact platform/arch token; macOS uses the universal archive, Windows `_windows-x86_64.zip`, Linux `_linux-x86_64.tar.gz`. Unsupported platform/arch refuses instead of guessing.
3. **Integrity:** prefers GitHub's `sha256:` digest, falls back to `SHA256SUMS.txt`; a staged file is re-hashed before unpacking.
4. **Safe staging:** downloads into the app's own data directory; archive extraction rejects absolute paths and `..`; only plain asset names are addressable.
5. **Prepare:** unpacks into `updates/prepared/<version>`, refuses an archive without a recognisable app binary, and records `replaces_running_binary: false`.
6. **Rollback:** a swap/rollback record exists and `POST /api/update/rollback` restores a displaced build; covered by Rust tests in the 719-test suite.
7. **User data:** updates never write to the config/database paths; preparation lives under the data directory and the running install is untouched.

### Update limitations

- This build **prepares** an update; it does not silently replace a running binary. Installation remains an explicit package/manual action. That is deliberate, and is why no "apply downloaded update" claim is made.
- No live GitHub release was downloaded during Phase 8 (no production release was created). Update parsing, checksum rejection, platform refusal and rollback logic are covered by unit tests, not by an end-to-end network upgrade.
- Windows MSI upgrade and Linux package upgrade were not executed.
- Android has no in-app self-update; distribution is via APK/AAB.

## What a user can rely on today

- A Linux user can install the rebuilt `.deb` and get the full UI; their home data is outside the package and survives upgrades.
- A Windows user gets a per-user MSI that replaces prior versions and leaves home data alone, but it is unsigned.
- A macOS user gets an unsigned `.app` that requires manual Gatekeeper approval.
- An Android user gets a debug-installable APK; release signing is a distributor step.
- Updates verify SHA-256 before unpacking and never overwrite the running install silently.
