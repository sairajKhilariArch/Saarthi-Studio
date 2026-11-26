# Build Matrix Report — Phase 9

**Version:** 3.88.15
**Commit:** `0af8acb`
**Release workflow run:** `45694555979` (workflow_dispatch, no tag, `publish=false`)
**Android CI run:** `45694560967`
**Desktop CI run:** `45694547969`

## 1. Supported and built targets

| Platform | Target | Runner | Artifact | Built in Phase 9 | Runtime validation |
|---|---|---|---|---|---|
| Linux x86_64 | Headless `.deb` | `ubuntu-22.04` | `free-ai-studio_3.88.15-1_amd64.deb` | **Yes**, locally + CI | Payload booted locally; `dpkg -i` **blocked** |
| Linux x86_64 | Portable | `ubuntu-latest` | `sarthi-studio_3.88.15_linux-x86_64.tar.gz` | CI | `smoke-package.sh` unpack/boot |
| Linux x86_64 | AppImage | `ubuntu-latest` | `sarthi-studio_3.88.15_x86_64.AppImage` | CI | AppImage extract/boot |
| Windows x86_64 | Portable | `windows-latest` | `sarthi-studio_3.88.15_windows-x86_64.zip` | CI | Packaged zip smoke; install **blocked** |
| Windows x86_64 | MSI | `windows-latest` | `sarthi-studio_3.88.15_windows-x86_64.msi` | CI (WiX 5) | Administrative install smoke on CI; no physical install |
| macOS universal | Portable | `macos-latest` | `sarthi-studio_3.88.15_macos-universal.tar.gz` | CI | `lipo` both arches; headless HTTP smoke |
| macOS universal | `.app` | `macos-latest` | `sarthi-studio_3.88.15_macos-universal.app.zip` | CI | Bundle smoke; unsigned |
| Android | APK/AAB | `ubuntu-latest` + Android SDK 35 | `free-ai-studio-android.apk`, `.aab` | **Yes** (CI run `45694555979` job success; Android CI `45694560967` success) | Structural smoke; emulator tests in Android CI |
| VS Code | VSIX | `ubuntu-latest` | `free-ai-studio-vscode.vsix` | **Yes** (release CI) | 45-test extension suite in Desktop CI |
| Web | Static production build | `ubuntu-latest` | `website/dist` (37 HTML pages) | **Yes** (Desktop CI `45694547969` website job success) | `astro check`, `check-build` |
| Web | Rust backend | Linux | `.deb`/tarball binary | **Yes** | Loopback HTTP + `/api/ready` verified from package payload |

## 2. Targets deliberately not produced

| Target | Why not |
|---|---|
| `.rpm` | No RPM spec, packaging tool or validation exists. Producing an untested RPM and calling it supported would violate the brief. |
| Arch package | Same: no PKGBUILD or makepkg validation. The project documents a source install instead. |
| Linux aarch64 | No Linux ARM runner or artifact; the updater explicitly refuses an unsupported arch. |
| Windows ARM64 | No target or hardware validation. |
| macOS x86_64-only | The universal binary already contains both slices; a second artifact would be redundant. |
| Android physical-device package | Emulator only; no production signing. |
| Server/cloud web package | The web UI is served by the local Rust backend, not a remote Node server. No remote deployment package exists. |

## 3. Matrix mechanics

- All platform jobs depend on `preflight`, which validates version/tag.
- Each job uploads a private `actions/upload-artifact` bundle with an exact
  `verify-artifacts.sh` name check.
- `finalize` downloads all bundles with `merge-multiple: true`, then verifies
  the complete 14-file contract. A missing or renamed file fails there.
- `publish` is separate and blocked while signing credentials are absent.
- Android and VSIX assets already succeeded in `45694555979`; the finalize job
  is the remaining gate for the seven platform bundles.

## 4. Local reproduction

The `.deb` was rebuilt on this host at 3.88.15:

```
cargo build --release --locked --no-default-features
cargo deb --locked --no-build
# free-ai-studio_3.88.15-1_amd64.deb, 5,233,568 bytes
# SHA-256 686691cf762c43d4107a38b46071db1d014d0edf8f83c34313925c6529eed537
```

The package contains 25 files, including `usr/bin/free-ai-studio`, the desktop
entry, AppStream metadata, icon, all 19 runtime static UI files and the three
bundled skills. It reports `Sarthi Studio 3.88.15`, serves `/`,
`/static/app.js`, `/static/styles.css`, `/static/index.html` with HTTP 200 and
returns healthy `/api/ready`.

Windows/macOS ARM builds were not attempted locally; they are native CI jobs
only. No cross-compiled artifact is presented as validated.

## 5. Build evidence status

| Evidence | State |
|---|---|
| Local Linux build | **PASS** |
| Local package payload boot | **PASS** |
| CI Linux/Windows/macOS/Android/VSIX builds | **PASS** — run `45694555979`, every build job success |
| Full 14-asset finalize | **PASS** — run `45694555979`, 14 files, checksums and metadata verified |
| Windows/macOS install & launch | **BLOCKED** (no hardware) |
| Signing | **BLOCKED** (no credentials) |
| Public release | **NOT PERFORMED** (not authorized) |

A successful build is not an installation test. The runtime states above are
kept separate from the build states on purpose.
