# Sarthi Studio — Installation and Upgrade Guide

**Version:** 3.88.18 (acceptance evidence gathered on 3.88.17; 3.88.18 is a docs-only bump)  
**Verified host:** Linux x86_64 (Arch, kernel 7.3.5)  
**Warning:** Windows and macOS packages are **unsigned**. Linux packages are unsigned unless a signed release is explicitly published. Verify SHA-256 before installing. Never disable OS security controls just to install this app.

## 1. Choose an artifact

| Platform | Artifact | Notes |
|---|---|---|
| Linux `.deb` | `free-ai-studio_<version>-1_amd64.deb` | Built in CI with dependency metadata; system install was **not** executed in Phase 10 |
| Linux AppImage | `sarthi-studio_<version>_x86_64.AppImage` | `chmod +x` and run; no system install |
| Linux portable | `sarthi-studio_<version>_linux-x86_64.tar.gz` | Extract and run `./free-ai-studio` |
| Windows | `sarthi-studio_<version>_windows-x86_64.zip` or `.msi` | **Unsigned**; install/launch not verified on Windows |
| macOS | `sarthi-studio_<version>_macos-universal.app.zip` or `.tar.gz` | **Unsigned, not notarized**; right-click → Open may be required; not verified on macOS |
| Android | `free-ai-studio-android.apk` / `.aab` | **Unsigned diagnostic build**; sign before install/distribution |
| VS Code | `free-ai-studio-vscode.vsix` | Extensions → `…` → Install from VSIX, or `code --install-extension` |

Every release also carries `SHA256SUMS.txt`, `sbom.cdx.json`, `release-manifest.json` and `RELEASE_NOTES.md`.

## 2. Verify before installing

```bash
sha256sum -c SHA256SUMS.txt
```

A checksum verifies integrity, not publisher identity. Because the current artifacts are unsigned, a compromise of the release origin could replace both an artifact and the checksum file. This is a known, release-blocking limitation.

## 3. Linux

### 3.1 Debian/Ubuntu package

```bash
sudo dpkg -i free-ai-studio_<version>-1_amd64.deb
sudo apt-get -f install    # resolve any missing dependencies
free-ai-studio             # or launch Sarthi Studio from the menu
```

**Phase 10 status:** the `.deb` payload was verified locally, but `dpkg -i`, dependency resolution, upgrade and purge were **not executed** because this host has no dpkg/root. Treat this command as the intended procedure, not a tested result.

### 3.2 AppImage

```bash
chmod +x sarthi-studio_<version>_x86_64.AppImage
./sarthi-studio_<version>_x86_64.AppImage
```

### 3.3 Portable tarball

```bash
tar -xzf sarthi-studio_<version>_linux-x86_64.tar.gz
cd sarthi-studio_<version>_linux-x86_64
./free-ai-studio
```

The binary finds the UI in `../share/sarthi-studio/static`, a macOS `Resources/static`, a `static/` directory beside the executable, or the source tree fallback. Set `FREE_AI_STUDIO_STATIC_DIR` only if you deliberately relocate the UI.

### 3.4 Install from source

Prerequisites: Linux, Rust stable/nightly per `rust-toolchain.toml`, `mold`, `pkg-config`, GTK/WebKit/AppIndicator development packages, Docker for the router.

```bash
./install.sh --check     # inspect dependencies without changing the machine
./install.sh --deps-only # install dependencies only
./install-and-run.sh     # build, install, start
```

The source installer builds into `~/.local/share/sarthi-studio/app`, installs `~/.local/bin/sarthi-studio`, and adds an applications-menu entry. Its stdout log is `~/.local/state/sarthi-studio/studio.log`.

## 4. Windows

1. Extract the zip and run `free-ai-studio.exe`, or run the MSI.
2. WebView2 is included with Windows 10/11; install it if the window cannot start.
3. The MSI and executable are unsigned, so SmartScreen may warn.

**Not verified in Phase 10:** MSI install, first launch, WebView2 window, ConPTY terminal, Credential Manager, upgrade and uninstall. Do not treat the CI build as an installed-product validation.

## 5. macOS

1. Unzip the app and move it to Applications.
2. Because it is unsigned/not notarized, use right-click → Open on first launch if Gatekeeper blocks it.
3. Terminal/PTY, Keychain and GUI behaviour were not executed here.

## 6. Android companion

- Use the APK only after signing it with your own release keystore; the shipped artifact is an unsigned diagnostic build.
- The emulator CI run covered install and six instrumentation tests on API 35; no physical device was tested.
- Desktop Host mode requires TLS and a host token:

```bash
FREE_AI_STUDIO_HOST_API_ENABLED=true \
FREE_AI_STUDIO_HOST_API_BIND=<LAN-IP> \
FREE_AI_STUDIO_HOST_API_TLS_CERT=/path/cert.pem \
FREE_AI_STUDIO_HOST_API_TLS_KEY=/path/key.pem \
free-ai-studio
free-ai-studio host-token create
```

Do not expose the Host API without TLS and a token.

## 7. First run

```bash
free-ai-studio              # application window
free-ai-studio --browser    # default browser
free-ai-studio --no-gui     # headless server
FREE_AI_STUDIO_PORT=9000 free-ai-studio
```

Then open Settings, configure the router/provider, test the connection, save, restart and confirm persistence.

## 8. Upgrade

### Portable / AppImage / zip / app

1. Stop the running app.
2. **Back up** `~/.config/free-ai-studio/` and `~/.local/share/free-ai-studio/history.sqlite3` (Linux; use the platform equivalents elsewhere).
3. Replace the old binary/bundle with the new one.
4. Start the new version and confirm settings, conversations and the project folder.
5. If startup fails, restore the previous binary and the backups.

The app migrates the database forward automatically, backs it up before migration and refuses a database from a newer schema instead of overwriting it.

### Package manager

```bash
sudo dpkg -i free-ai-studio_<new-version>-1_amd64.deb
```

Package-manager upgrade and rollback were **not executed** in Phase 10. A payload-level probe (3.88.3 → 3.88.15) preserved config and conversation data; that is not a substitute for a real `dpkg` upgrade.

### Updates from inside the app

The update client checks HTTPS downloads, a published SHA-256 and a staged re-hash, and rejects archive traversal. **No live update from a published release was tested**, because no release was published. Do not rely on in-app update/rollback for this build.

## 9. Uninstall and data retention

### Package / portable

Remove the installed binary/bundle. The application's **user data is outside the install directory** and is not removed automatically:

- `~/.config/free-ai-studio/`
- `~/.local/share/free-ai-studio/`
- `~/.local/state/sarthi-studio/` (source installer log)
- `~/.config/sarthi-studio/` (source installer/router helper files)

Delete those directories only if you intend to erase settings, history and router ownership records. **Back up first.** Uninstall behaviour was not executed through a package manager in Phase 10.

### Android / VS Code

- Android: use the normal app uninstall; no desktop data is removed.
- VS Code: uninstall the extension from the Extensions view. The extension's built-in server may keep its own data; review its documentation before deleting.

## 10. Post-install checklist

- [ ] `free-ai-studio --version` prints `Sarthi Studio 3.88.18`.
- [ ] `/api/ready` answers and reports database, schema and data directory checks.
- [ ] Settings save and survive a restart.
- [ ] Provider **Test connection** succeeds.
- [ ] A conversation sends, streams, stops and reopens after restart.
- [ ] The intended folder opens and a file can be edited.
- [ ] Agent write/command approval behaves as configured.
- [ ] Data locations are backed up.
- [ ] The artifact is described as **unsigned** unless a signed release was actually published.

## 11. Known installation limitations

| Limitation | Status |
|---|---|
| Linux system install/upgrade/uninstall | Blocked (no dpkg/root); `.deb` payload only |
| Windows/macOS install and launch | Blocked (no hosts); CI build only |
| Signatures/notarization | Absent; publication gate blocks |
| Live update/rollback | Not validated |
| Dependency metadata on Arch | Not resolved locally; CI Ubuntu job provides it |
| Android production signing | External; unsigned diagnostic artifact |
