# Sarthi Studio — Platform Compatibility Matrix

**Date:** 2026-11-25
**Code version:** 3.88.12 (reports committed as 3.88.13, docs-only)
**Branch:** `platform/phase-8-cross-platform-2026-11-25`

## State definitions

| State | Meaning in this matrix |
|---|---|
| **Verified** | Built and executed on the target platform, with the listed behaviour exercised |
| **CI-tested** | Built, unit-tested and linted on a native runner of that OS, but not installed/launched |
| **Built** | An artifact was produced but not installed |
| **Payload-tested** | The package payload was unpacked and executed from its installed layout, without a system package manager install |
| **Blocked** | Hardware, credentials or an OS runner was unavailable; no claim is made |
| **Not supported** | The feature/artifact does not exist |
| **N/A** | Does not apply to that platform |

## Matrix

| Capability | Linux x86_64 | Windows x86_64 | macOS (universal) | Android | Web (loopback) |
|---|---|---|---|---|---|
| Build | **Verified** (`cargo build --release`) | **CI-tested** | **CI-tested** | **CI-tested** (CI) | N/A (no build) |
| Unit tests | **Verified** 719 + 3 CLI | **CI-tested** | **CI-tested** | **CI-tested** JVM + 6 instrumented | **Verified** 125 JS |
| Clippy/format | **Verified** | **CI-tested** clippy | **CI-tested** clippy | Android lint: 0 errors, 51 warnings | N/A |
| Package | `.deb` **built + payload-tested**; tarball/AppImage release-CI only | zip/MSI workflow; packager **CI-tested** | `.app.zip`/universal tarball workflow; bundle script **CI-tested** | debug APK + unsigned release APK/AAB **built** | N/A |
| Install | **Blocked** (`dpkg -i`) | **Blocked** | **Blocked** | **Verified on emulator** (debug APK) | N/A |
| First launch | **Verified** headless from package layout | **Blocked** | **Blocked** | **Verified** emulator | **Verified** HTTP UI |
| GUI window | **Blocked** (headless build; needs WebKitGTK session) | **Blocked** | **Blocked** | **Verified** activity launch | Browser UI **Verified** |
| Data directory + SQLite | **Verified** 0700 dir, 0600 DB | **CI-tested** code; runtime **blocked** | code shared; runtime **blocked** | **Verified** instrumentation migration/backup | server-side **Verified** |
| Credential storage | Secret Service compiled; runtime **blocked** | Credential Manager compiled; runtime **blocked** | Keychain compiled; runtime **blocked** | **Keystore** AES-GCM implemented; emulator run | server-side keychain |
| Terminal | **Verified** by E2E (PTY routes) | ConPTY code **CI-tested**; runtime **blocked** | PTY code **CI-tested**; runtime **blocked** | **N/A / not exposed** | requires desktop host |
| Filesystem/project | **Verified** E2E | **CI-tested** code | **CI-tested** code | **N/A / not exposed** | requires desktop host |
| Notifications | **Not supported** | **Not supported** | **Not supported** | Toast/UI only | browser notifications **not implemented** |
| Tray | **Not supported** | **Not supported** | **Not supported** | N/A | N/A |
| Deep links | **Not supported** | **Not supported** | **Not supported** | no intent filter | N/A |
| Update check + SHA-256 | **Verified** unit tests | **CI-tested** | **CI-tested** | no self-update | N/A |
| Update install/rollback | **Not verified live**; rollback logic unit-tested | **Blocked** | **Blocked** | distributor signing | N/A |
| Upgrade over older version | **Blocked** (`dpkg`) | **Blocked** | **Blocked** | **Blocked** | data migration **Verified** on desktop |
| Uninstall | package metadata only; **blocked** | MSI component defined; **blocked** | **blocked** | standard app uninstall; not tested | N/A |
| User data preserved on upgrade | **Blocked** (not installed) | **Blocked** | **Blocked** | not applicable | restart-persistence **Verified** |
| Signing | package unsigned | **unsigned**; Authenticode blocked | **unsigned**, no notarization | release unsigned by design | N/A |
| ARM64 | **Not supported** | **Not supported** | build targets arm64 in release; runtime **blocked** | ARM device **blocked** | N/A |
| OS versions covered | Arch Linux 7.3.5 (host); Ubuntu CI | CI server image, **not Win10/11** | CI macOS image | API 35 emulator | Chromium |

## CI evidence

| Run | Branch commit | Jobs | Result |
|---|---|---|---|
| Desktop CI `45687899519` | `830c4e7` (3.88.12) | Linux, Windows, macOS | **success** (3/3) |
| Android CI `45686438710` | `3d30f6b` (3.88.10) | test, lint, APK, AAB, emulator | **success** |

Desktop CI steps: Linux package metadata 9/9, version 26/26, artifacts 12/12, release contract 27/27, frontend 125/125, `.deb` asset guard, installer-dependency test, macOS bundle packager, formatting, build, 719 + 3 tests, clippy, 76 E2E — all successful on each applicable runner.

Android CI: JVM tests, lint (0 errors, 51 warnings), debug APK, instrumentation APK, unsigned release APK/AAB, API 35 x86_64 emulator, 6 instrumentation tests — all successful.

## Resource comparison

| Platform | Hardware | Startup | Idle memory | Idle CPU | Disk |
|---|---|---|---|---|---|
| Linux x86_64 | Arch, kernel 7.3.5, local desktop | **89 ms** to HTTP 200 | **14,476 KB RSS** | **0.0 %** sample | 5.0 MB `.deb` |
| Windows | GitHub `windows-latest` | **Not measured** | Not measured | Not measured | Not measured |
| macOS | GitHub `macos-latest` | **Not measured** | Not measured | Not measured | Not measured |
| Android | CI emulator API 35 x86_64 | **Not measured** | Not measured | Not measured | 69–123 KB artifacts |
| Web | Same Linux process | same process | same process | same process | N/A |

Only the Linux figures are comparable measurements. The other platforms were not instrumented, and different hardware would make a direct comparison invalid.

## Coverage statement

- **Linux:** source, package payload, runtime, permissions, HTTP boundary and persistence are verified; system install/upgrade/uninstall remain blocked.
- **Windows:** source compiles, tests/clippy pass and the MSI packager accepts the tree; no install or launch is claimed.
- **macOS:** source compiles and tests/clippy pass; no install, launch, signature, notarization or Gatekeeper claim.
- **Android:** built, installed and executed on the CI emulator; no physical device or production signing.
- **Web:** served UI and security headers verified on loopback; no accounts, other browsers or public deployment.

No platform is marked fully supported beyond this evidence.
