# Sarthi Studio — Phase 8 Cross-Platform Audit

**Date:** 2026-11-25
**Branch:** `platform/phase-8-cross-platform-2026-11-25`
**Base:** `e637722` (Phase 7, 3.88.9) → `3d30f6b` (3.88.10) → `f5cb84b` (3.88.11) → `830c4e7` (3.88.12)

## Method

- Inspected the Rust server/desktop crate, the static frontend, Android Java client, VS Code extension, website, packaging manifests, release workflow and CI workflows.
- Traced every `cfg(target_os)`/`cfg(unix)` branch and the platform-facing modules.
- Built and booted the Linux package from its installed layout.
- Ran the full local suites and dispatched Desktop CI (Linux/Windows/macOS) and Android CI on the branch.
- Marked anything not actually executed as **built**, **compiled**, **blocked** or **not supported**; compilation alone is never called platform validation.

## Existing architecture

| Concern | Implementation | Platform handling |
|---|---|---|
| Backend | Rust, Axum, SQLite (bundled), Tokio | cross-platform; Tauri optional |
| Desktop shell | Tauri v2 (`gui` feature) or headless server + browser | macOS `.app` layout, Windows/Linux binaries |
| Frontend | Hand-written ES modules served by the backend | same UI everywhere the browser runs |
| Mobile | Native Java single-activity client with its own SQLite and Keystore | Android only; no desktop filesystem/shell |
| Credentials | `keyring` 3: Secret Service / Credential Manager / Keychain | one API, three native stores |
| Paths | `HOME`/`USERPROFILE` + `.config/free-ai-studio` and `.local/share/free-ai-studio` | deliberately identical dot-directory layout on every OS (see `AGENTS.md`: never move these paths) |
| Terminal | `portable-pty`; PowerShell on Windows, `$SHELL`/bash elsewhere | shared interface |
| Process control | Tokio process + `taskkill`/kill paths | cfg-scoped |
| Browser launch | `xdg-open` / `open` / `rundll32` | `browser.rs` |
| Folder picker | GTK/portal (Linux), PowerShell WinForms dialog, AppleScript | one `PickerResult` interface |
| Updates | GitHub release feed → asset selection → SHA-256 → stage → prepare | archive kinds per OS; no silent binary swap |
| Packaging | `.deb`, tarball, AppImage, Windows zip + MSI, macOS `.app.zip` + universal tarball, Android APK/AAB | release workflow |
| Installer tests | Linux package smoke, WiX packager test, macOS bundle checks, Android instrumentation | CI |

## Findings

| ID | Severity | Finding | Evidence | Status |
|---|---|---|---|---|
| CP-01 | High | Linux `.deb` omitted `ide.js`, `ide.css`, `highlight.js`, `replace.js` and brand assets. The installed UI could not render the workbench. | `app/Cargo.toml` asset list vs `index.html` script/CSS tags and `app/static` tree | **Fixed**; manifest expanded; `test-deb-assets.sh` guard added |
| CP-02 | High | Android network-security-config listed CIDR ranges under `<domain>`, which Android does not match. The documented LAN HTTP setup was blocked when built. | `android/.../network_security_config.xml`; Android domain matching rules | **Fixed**; app-level `CleartextPolicy` + unit tests; config now explicitly permissive with code enforcement |
| CP-03 | High | Android release build was signed with the debug keystore, producing `app-release.apk` while CI/docs expect `app-release-unsigned.apk`; a debug key is not a release identity. | `android/app/build.gradle.kts`, `BUILDING.md`, `android-ci.yml` | **Fixed**; unsigned release artifact verified in CI |
| CP-04 | Medium | Android 15 (`targetSdk 35`) draws edge-to-edge; no system-bar/cutout padding existed. | `MainActivity.buildUi` had a fixed padding only; no insets listener | **Fixed**; `applySystemBarInsets()` |
| CP-05 | Medium | Android SQLite file was created 0644 (directory 0700 masked it); conversation text should be owner-only. | `stat -c %a` on a fresh Linux database returned 644 | **Fixed**; `open_db` chmods 0600; E2E asserts it |
| CP-06 | Medium | Desktop CI's installer-dependency test was not hermetic: the workflow installed `mold` before a test that hides it, so the real binary leaked through `/usr/bin` and the "missing" case passed as present. | CI run `45686434283`, step "Test installer dependency detection"; local rerun passes | **Fixed**; private symlink farm omits mold |
| CP-07 | Medium | Config/data paths are Unix dot-directories on Windows/macOS. | `paths()` in `main.rs` | **Not changed** — the repository's `AGENTS.md` explicitly says never move these paths. Documented as a deliberate portability choice; Windows uses `USERPROFILE`, macOS uses `HOME`. |
| CP-08 | Medium | No `.rpm` or Arch package; AppImage is release-CI only. | packaging tree, release workflow | **Open**; documented as unsupported/not built |
| CP-09 | Medium | Windows/macOS installers are unsigned; macOS has no hardened runtime or notarization; Android release is unsigned. | `installer.wxs`, `bundle-macos-app.sh`, release notes | **Blocked** by certificates/credentials; not faked |
| CP-10 | Low | No tray, desktop notifications, file associations or deep links. | no implementation; not advertised | **Not supported**; no fake controls |
| CP-11 | Low | Android has only emulator validation; no physical device. | CI runner API 35 x86_64 | **Blocked** by hardware |
| CP-12 | Low | Linux ARM64, Windows ARM64 and macOS Intel runtime are not validated. macOS builds a universal binary in release CI; only Intel compile is in Desktop CI. | rustup targets, workflow matrix | **Not supported / blocked** |

## Intentional constraints preserved

- `terminal.rs` refuses to run outside a workspace root; this is a security boundary, not a platform gap.
- The Android client deliberately has **no** filesystem root access, shell execution or desktop project UI. A fake terminal was not added.
- The local web UI has no accounts; it is loopback-only. Remote access is the separate HTTPS host API with bearer tokens.
- Workflow/agent features remain backend-executed; no platform-specific simulations were introduced.

## Files changed in Phase 8

```
app/Cargo.toml                         complete .deb runtime payload
app/src/main.rs                        SQLite 0600
scripts/ci/test-deb-assets.sh          new package-asset guard
scripts/ci/test-install-deps.sh        hermetic mold test
scripts/ci/test-e2e.sh                 database-mode assertion
android/app/build.gradle.kts           unsigned release, version 1.51.1/61
android/.../CleartextPolicy.java       new LAN-only cleartext policy
android/.../CleartextPolicyTest.java   new unit coverage
android/.../MainActivity.java          policy enforcement + system-bar insets
android/.../network_security_config.xml corrected platform config
.github/workflows/*.yml                asset guard + Android layout checks
```

## Verification limits

This host is Linux only. Windows and macOS evidence comes from GitHub Actions on their native runners; install/uninstall there remains **blocked**. Android was built and executed only on the CI emulator. No signing/notarization credentials exist. See `PLATFORM_COMPATIBILITY_MATRIX.md` for the per-state result.
