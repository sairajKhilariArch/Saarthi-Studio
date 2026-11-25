# Sarthi Studio — Phase 8 Windows Validation

**Date:** 2026-11-25
**Build:** 3.88.12
**Runner:** GitHub Actions `windows-latest` (Desktop CI run `45687899519`)

## Status summary

| State | Result |
|---|---|
| Implemented | Yes |
| Built | Debug binary built on the Windows runner |
| Packaged | MSI packager tested on the runner; release zip/MSI are built by the release workflow |
| Installed | **No** — no Windows 10/11 desktop available |
| Executed | **No** — CI builds/tests; it does not launch the GUI |
| Tested | `cargo build`, **719 + 3 tests**, clippy `-D warnings`, WiX MSI packager test |
| Blocked | Install, first launch, upgrade, uninstall, data preservation, Credential Manager runtime, notifications, tray, Windows 10/11 behaviour |

## CI evidence

Job **Test and build desktop (Windows)** — **success**.

| Step | Result |
|---|---|
| Check out source | success |
| Set up nightly Rust | success |
| Cache cargo | success |
| Build debug binary | success |
| Run desktop tests | success |
| Run clippy `-D warnings` | success |
| Install WiX v5 | success |
| Test the Windows MSI packager | success |

This validates the Windows-specific code paths (PowerShell folder picker, ConPTY terminal, `USERPROFILE` paths, process handling, Credential Manager backend compilation, installer rules) on Windows. The runner is a server image, **not** a Windows 10 or 11 desktop, and no GUI was launched.

## Windows-specific implementation

| Concern | Implementation |
|---|---|
| Home/data | `USERPROFILE` + `.config/free-ai-studio` and `.local/share/free-ai-studio` (repository policy keeps these paths stable cross-platform) |
| Terminal | `portable-pty` with `powershell.exe`, ConPTY |
| Process control | Tokio child processes; Windows-specific kill paths in `runner.rs` |
| Folder picker | Fixed PowerShell WinForms `FolderBrowserDialog` script; no request text interpolated |
| Browser launch | `rundll32 url.dll,FileProtocolHandler <url>`, avoiding `cmd`'s `&` splitting |
| Credential storage | `keyring` `windows-native` → Windows Credential Manager |
| Installer | Per-user MSI, `%LOCALAPPDATA%\Programs\Sarthi Studio`, fixed UpgradeCode, MajorUpgrade, Start-menu shortcut |
| Uninstall | Removes install files/shortcut; running-binary cleanup is scheduled by a detached PowerShell that waits for process exit |
| Update | Windows zip asset selected; SHA-256 verified; no silent replacement of a running `.exe` |
| Tray / notifications | Not implemented |
| Deep links | No URL scheme registered |

## Packaging

- Portable zip and MSI are created by `desktop-release.yml` on `windows-latest`.
- The MSI packages the entire staged tree, so new UI files are included without editing WiX.
- `scripts/ci/test-installer.sh` exercises the packager's refusals; it passed on the Windows runner. Locally on Linux it is skipped without WiX.
- The MSI is **unsigned**; there is no Authenticode certificate.

## Not tested / blocked

- Install via MSI, first launch, restart, upgrade from 3.88.9, uninstall, and preservation of user data.
- Windows Credential Manager read/write.
- Windows 10 and Windows 11 (CI runs a server image).
- System tray, toast notifications, file associations and deep links (features do not exist).
- Windows ARM64 build.
- PowerShell path handling beyond the tested folder-picker/cleanup scripts.
- SmartScreen and installer reputation.

## Evidence boundary

A passing Windows CI job means the source compiles, the test suite passes, clippy is clean and the MSI packager accepts the staged tree. It is **not** an installation or launch test, and this report does not claim one.
