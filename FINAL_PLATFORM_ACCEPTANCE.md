# Sarthi Studio — Final Platform Acceptance

**Phase:** 10 final production acceptance  
**Date:** 2026-11-26  
**Code verified:** 3.88.17 (`1d6e458`)  
**Policy:** a successful compile is not a platform PASS. Each platform is reported at the highest level actually executed.

## Acceptance levels

1. Source compiles  
2. Release binary/package builds  
3. Installer/package is produced  
4. Installer succeeds  
5. Application launches  
6. Core features work  
7. Upgrade works  
8. Uninstallation works  
9. User data survives an upgrade

## Matrix

| Level | Linux x86_64 | Windows x86_64 | macOS universal | Android | Web (loopback) |
|---|---|---|---|---|---|
| 1. Compiles | **PASS** local | **PASS** CI | **PASS** CI | **PASS** CI | N/A |
| 2. Release build | **PASS** local + CI | **PASS** CI | **PASS** CI | **PASS** unsigned release APK/AAB CI | N/A |
| 3. Package produced | **PASS** `.deb`, tarball, AppImage in CI; local `.deb` payload | **PASS** zip/MSI CI | **PASS** `.app.zip` / tarball CI | **PASS** APK/AAB | N/A |
| 4. Installer succeeds | **BLOCKED** — no `dpkg`/root on Arch host | **BLOCKED** — no Windows host | **BLOCKED** — no macOS host | **PASS** debug APK on API 35 emulator | N/A |
| 5. Launches | **PASS** headless release; native GUI **BLOCKED** | **BLOCKED** | **BLOCKED** | **PASS** emulator activity | **PASS** browser at 127.0.0.1 |
| 6. Core features | **PASS** 76/76 release E2E | **CI tests only**, runtime **BLOCKED** | **CI tests only**, runtime **BLOCKED** | Partial: native client shell + host-mode limitations; no physical device | **PASS** desktop API/UI over loopback; no accounts |
| 7. Upgrade | **BLOCKED** system install; payload replacement tested | **BLOCKED** | **BLOCKED** | **BLOCKED** | N/A |
| 8. Uninstall | **BLOCKED** | **BLOCKED** | **BLOCKED** | Not tested | N/A |
| 9. Data survives upgrade | **PASS** payload replacement preserved DB/config; no package-manager upgrade | **BLOCKED** | **BLOCKED** | Not tested | N/A |

## Platform detail

### Linux x86_64 — the only runtime-verified desktop platform

- Host: Arch Linux, kernel 7.3.5, AMD Ryzen 9 8940HX, 14 GiB RAM, no desktop session used for the native window.
- Release binary: cold start 83 ms, warm 67–84 ms; shutdown 12 ms.
- E2E: 76/76 on debug and 76/76 on release binary.
- `.deb` produced locally in Phase 9 (3.88.15, 5,233,568 bytes, SHA-256 `686691cf…9eed537`); payload 14/14 checks; upgrade probe 3.88.3 → 3.88.15 preserved conversation/config and grew the DB 176,128 → 180,224 bytes.
- **Not** installed through `dpkg`; `dpkg-shlibdeps` and root are unavailable, so dependencies were not resolved and system install/upgrade/uninstall are not claimed.
- Native Tauri window was not launched; UI verification used the same local server in a browser.

### Windows x86_64 — CI-tested, runtime blocked

- CI on `windows-latest`: fmt, build, 721 + 3 tests, clippy, 76 E2E, frontend, package metadata and MSI/zip packaging.
- No Windows machine was available; MSI install, first launch, GUI, ConPTY terminal, Credential Manager, upgrade and uninstall were not executed.
- Artifacts are **unsigned**; Authenticode was not configured.

### macOS universal — CI-tested, runtime blocked

- CI on `macos-latest`: fmt, build, universal bundle/zip, tests, clippy, E2E.
- The Phase 9 CI failure in `filewatch::tests::a_started_watcher_indexes_a_file_written_after_open` was a test race: a fixed 150 ms sleep did not guarantee FSEvents registration. Phase 10 replaced it with an index-sentinel handshake; the test passed 3/3 locally and is re-run in Phase 10 CI.
- No macOS machine: `.app` install, Gatekeeper, hardened runtime, notarization, GUI, upgrade and uninstall are not verified. Artifacts are unsigned.

### Android — emulator only

- Android CI `45694560967` succeeded: JVM tests, lint (0 errors, 51 warnings), debug APK, instrumentation APK, unsigned release APK/AAB, API 35 x86_64 emulator and 6 instrumentation tests.
- Release artifacts are **unsigned by design**; distribution requires the project keystore.
- No physical device, no production signing, no Play upload.

### Web (loopback)

- Served by the same Rust process at `127.0.0.1`; live UI loaded with no console errors, 10 primary navigation buttons and 281 controls.
- No account system, no public deployment, no cross-browser matrix; Firefox/Safari were not tested.
- Binds loopback only; remote use requires an SSH tunnel or the separately gated Desktop Host API.

## Platform verdict

**Linux is the only desktop platform accepted at runtime, and even there system package installation is blocked.** Windows and macOS are CI-tested builds, not installed products. Android is emulator-verified with unsigned artifacts. Web is loopback-only. No platform is advertised as fully supported beyond this evidence.
