# Versioning Strategy — Phase 9

**Authoritative desktop version:** 3.88.15
**Source of truth:** `app/Cargo.toml` (`[package].version`)
**Phase 9 commit:** `0af8acb`

## 1. Release lines

Saarthi Studio does not force unrelated products onto one version. It has four
independent release lines, each internally consistent:

| Line | Source of truth | Current | Tag convention |
|---|---|---|---|
| Desktop (Rust app, installers, website backend) | `app/Cargo.toml` | **3.88.15** | `v3.88.15` |
| Android | `android/app/build.gradle.kts` (`versionName`) | **1.51.1** (code 61) | distributed with a desktop release; not separately tagged |
| VS Code extension | `vscode-extension/package.json` | **2.6.1** | marketplace/package version |
| Website | `website/package.json` | **0.5.2** | independent static-site line |

The website deliberately has its own version line; app paths must not move.
Desktop tags are the only tags the release workflow accepts.

## 2. Semantic Versioning policy

Desktop follows SemVer:

- **MAJOR** — incompatible configuration, database or API changes.
- **MINOR** — new backward-compatible features.
- **PATCH** — fixes, security patches and release-engineering changes.

Phase 9 is a PATCH release (3.88.14 → 3.88.15): it changes release mechanics,
CI, metadata and documentation, not the user-facing protocol or data schema.

Android follows `MAJOR.MINOR.PATCH` for its own app history and increments
`versionCode` monotonically. The code is the install-order authority; the name
is the user-visible version.

The database schema has its own monotonic integer. A downgrade is refused when
the database schema is newer than the running build; this is already enforced
at startup and tested.

## 3. Single-source flow

```
app/Cargo.toml
   ├── app/Cargo.lock (cargo update -p free-ai-studio --precise)
   ├── AppStream <release version="...">
   ├── CHANGELOG.md section "## <version>"
   ├── installer/manifest names
   ├── artifact names (sarthi-studio_<version>_*, free-ai-studio_<version>-1_amd64.deb)
   ├── SHA256SUMS / manifest / SBOM / notes
   └── git tag v<version>
```

`scripts/ci/version.sh` is the only parser used by CI, tests and release jobs.
It rejects a non-SemVer value, strips a leading `v` where allowed, and can
compare versions. `scripts/ci/version-matrix.sh --check` additionally verifies:

- newest AppStream `<release>` equals the Cargo version;
- `CHANGELOG.md` has a section for that version;
- Android `versionName` is SemVer, its `versionCode` is numeric, and README
  documents the pair;
- `package.json` and `package-lock.json` agree for VS Code and website.

## 4. Tag validation

The release workflow's `preflight` job:

1. reads the version from `app/Cargo.toml`;
2. if a tag is supplied, requires it to start with `v`;
3. requires `tag[1:] == version` exactly;
4. refuses a tag that names another version;
5. exports `version`, `tag`, `release` outputs used by every later job.

A tag push or dispatch with no tag cannot publish. A dispatch with a tag and
`publish=true` is the only publication path, and it still has to pass
`check-publish-gates.sh`.

Historical tags are not rewritten. The 3.88.15 metadata contract applies only
from 3.88.15, so audits of 3.4.0+ releases do not suddenly require files those
releases never had.

## 5. Version bump rules for this branch

- Every commit subject starts with the version it bumps to.
- Phase 9 implementation commit: `3.88.15: ...`.
- Reports-only follow-up (if needed) must bump to 3.88.16, update
  `Cargo.toml`, `Cargo.lock`, AppStream and `CHANGELOG.md` in the same commit.
- No commit may leave `app/Cargo.toml` and `Cargo.lock` disagreeing.

## 6. Verification evidence

```
$ bash scripts/ci/version-matrix.sh --check
LINE                     VERSION    STATE
desktop                  3.88.15    authoritative (tag must match)
android                  1.51.1     versionCode 61 (independent)
vscode-extension         2.6.1      independent
website                  0.5.2      independent
version matrix consistent
```

```
$ bash scripts/ci/test-version.sh
26 passed, 0 failed
```

`test-version.sh` also covers empty/CRLF/space names, tag/version mismatch and
shell-continuation regressions. `verify-release-contract.sh` proves the
workflow and `release-assets.sh` promise the same 14 files for 3.88.15.

## 7. Known limitation

There is no single file that stores all four product versions, because the
Android/VS Code/website projects are independent build systems. The matrix
script is the consistency authority; it fails CI when any line drifts from its
own metadata, without forcing unrelated products to share a number.
