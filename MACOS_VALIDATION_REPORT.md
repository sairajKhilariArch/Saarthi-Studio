# Sarthi Studio — Phase 8 macOS Validation

**Date:** 2026-11-25
**Build:** 3.88.12
**Runner:** GitHub Actions `macos-latest` (Desktop CI run `45687899519`)

## Status summary

| State | Result |
|---|---|
| Implemented | Yes |
| Built | Debug binary built on the macOS runner |
| Packaged | `.app`/tarball packaging exists in the release workflow; **not rebuilt in this run** |
| Installed | **No** — no Mac available |
| Executed | **No** — CI compiles and tests, it does not launch the GUI |
| Tested | `cargo build`, **719 + 3 tests**, clippy `-D warnings` on macOS |
| Signed / notarized | **No** |
| Blocked | Launch, Keychain runtime, `.dmg`, signing, hardened runtime, notarization, Gatekeeper, Apple Silicon/Intel runtime matrix |

## CI evidence

Job **Test and build desktop (macOS)** — **success**.

| Step | Result |
|---|---|
| Check out source | success |
| Set up nightly Rust | success |
| Cache cargo | success |
| Build debug binary | success |
| Run desktop tests | success |
| Run clippy (macOS) | success |

This proves the macOS-specific `cfg(target_os = "macos")` code paths compile and the test suite passes on Apple's OS. It does **not** prove an application window opens, the Keychain works, or a downloaded bundle passes Gatekeeper.

The Linux job separately ran **Test the macOS bundle packager** successfully, validating the bundle script's layout, `Info.plist` and archive rules on a non-Mac host.

## Packaging

`scripts/ci/bundle-macos-app.sh`:

- Produces `Sarthi Studio.app` with the binary in `Contents/MacOS` and the full UI in `Contents/Resources/static`.
- `Info.plist` carries the display name, bundle identifier `com.saarthi.freeaistudio`, version and minimum macOS 10.15.
- Refuses a missing binary, missing `index.html`, unusable version, non-executable payload or mismatched plist.
- Zips the bundle.

`desktop-release.yml` builds both `aarch64-apple-darwin` and `x86_64-apple-darwin` and merges them into a universal binary, then stages the universal tarball and `.app.zip`.

Not present:

- **`.dmg`** — not produced anywhere.
- Code signing and entitlements.
- Hardened runtime.
- Notarization and stapling.

The release workflow states explicitly that Gatekeeper will reject the unsigned download; users must approve it manually. No signing certificate or notary credential exists in this repository or environment, and none was fabricated.

## macOS-specific implementation

| Concern | Implementation | State |
|---|---|---|
| Data/config paths | `$HOME/.config/free-ai-studio`, `$HOME/.local/share/free-ai-studio` (repository policy keeps these paths stable) | Implemented; runtime not checked |
| Credential storage | `keyring` `apple-native` (Keychain) | Compiled; runtime **blocked** |
| SQLite | bundled rusqlite, 0600 file, 0700 directory | Unix code shared with the tested Linux build |
| Folder picker | AppleScript `choose folder` | Compiled; GUI **blocked** |
| Terminal | `portable-pty` with `$SHELL` or `/bin/bash` | Shared code; runtime **blocked** |
| Process management | Tokio child processes + kill | Shared code; runtime **blocked** |
| Browser launch | `open <url>` | Not executed |
| Deep links | No URL scheme registered | **Not supported** |
| Notifications | No desktop notification implementation | **Not supported** |
| Updates | Universal macOS archive selected by `choose_asset`; SHA-256 verified | Logic tested; no live update |

## Architecture coverage

- The release workflow builds and lipo-merges **arm64 + x86_64**, so the shipped artifact is universal.
- This Desktop CI job runs on one `macos-latest` image; its architecture is not evidence that both slices execute.
- Intel and Apple Silicon runtime validation are **BLOCKED** by hardware.

## Remaining work before calling macOS supported for distribution

1. Sign with a Developer ID Application certificate, enable hardened runtime and required entitlements.
2. Notarize with Apple and staple the ticket.
3. Produce and test a `.dmg` (or document `.app.zip` as the supported artifact).
4. Install, launch, upgrade and uninstall on at least one Apple Silicon and one Intel Mac.
5. Exercise Keychain storage, the folder picker, terminal and Gatekeeper acceptance.
