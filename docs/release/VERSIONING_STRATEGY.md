# Sarthi Studio versioning strategy

**Current desktop version:** 4.4.0  
**Desktop source of truth:** `app/Cargo.toml`  
**Desktop tag:** `v4.4.0`

## Independent release lines

| Product | Source of truth | Current version | Release convention |
|---|---|---:|---|
| Desktop (Rust host, installers) | `app/Cargo.toml` | **4.4.0** | Git tag `v4.4.0` |
| Android companion | `android/app/build.gradle.kts` | **1.52.0** (version code 62) | Independent Android version |
| VS Code extension | `vscode-extension/package.json` | **2.6.1** | Marketplace/package version |
| Website | `website/package.json` | **0.5.2** | Independent static-site version |

Do not force unrelated applications to share a version number. The desktop release may bundle Android and VS Code artifacts, but their internal version numbers remain independent.

## Semantic versioning

The desktop application follows Semantic Versioning:

- **MAJOR:** incompatible configuration, database, or API changes.
- **MINOR:** backward-compatible product features.
- **PATCH:** compatible fixes and maintenance.

The move from 3.x to 4.0.0 establishes the next major desktop release line. It does not, by itself, imply that Android, VS Code, or the website require a major-version change.

Android increments `versionCode` monotonically. The database schema has its own monotonic integer and refuses to open a database created by a newer schema.

## Single-source version flow

`app/Cargo.toml` is authoritative for the desktop release. Keep these in sync:

1. The local `free-ai-studio` package entry in `app/Cargo.lock`.
2. The newest AppStream `<release>` entry.
3. A non-empty `CHANGELOG.md` section.
4. The release tag `v<version>`.

Run:

```bash
bash scripts/ci/version-matrix.sh --check
bash scripts/ci/test-version.sh
bash scripts/ci/verify-release-contract.sh .github/workflows/desktop-release.yml
```

The matrix also checks Android README/versionCode documentation and the VS Code and website package/lockfile pairs.

## Release safety

A tag push builds and verifies release artifacts; it does not automatically publish a public GitHub release. Public publication is a separate, explicitly authorized workflow dispatch and must pass the repository's signing and release gates. Do not describe unsigned artifacts as signed.

Historical desktop release notes are preserved in `docs/release-history/CHANGELOG-3.x.md`.
