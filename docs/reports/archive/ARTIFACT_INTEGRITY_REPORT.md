# Artifact Integrity and Supply-Chain Report — Phase 9

**Version:** 3.88.15
**Commit:** `0af8acb`
**Release workflow:** `.github/workflows/desktop-release.yml`

## 1. What a release now contains

For 3.88.15 and later, the release contract is 14 files:

| Group | Files |
|---|---|
| Platform (10) | Android APK/AAB, VSIX, `.deb`, Linux tarball, AppImage, Windows zip/MSI, macOS app zip/tarball |
| Integrity (4) | `SHA256SUMS.txt`, `RELEASE_NOTES.md`, `release-manifest.json`, `sbom.cdx.json` |

The finalize job is the only place that creates the integrity files. It runs
after every platform job succeeds, downloads all bundles into one directory and
then:

1. verifies the exact artifact names and non-empty files;
2. generates a CycloneDX 1.5 SBOM from `app/Cargo.lock`;
3. generates curated notes from the exact `CHANGELOG.md` section;
4. generates `release-manifest.json` with version, tag, source commit, workflow
   run/repository and a SHA-256 + size for every file except the manifest and
   checksum file;
5. hashes every remaining file into one global `SHA256SUMS.txt`;
6. verifies the checksum file lists exactly those files and matches their bytes;
7. validates SBOM version/format/component count, manifest version/commit/hash
   agreement, and notes version/limitations;
8. uploads the complete bundle and attests build provenance.

## 2. SBOM

```
SBOM: 523 Rust components for 3.88.15
```

- Format: CycloneDX **1.5**, `bomFormat: CycloneDX`.
- Components: 523 `pkg:cargo/...` entries with versions and registry SHA-256
  hashes from `Cargo.lock`.
- Identity: deterministic `urn:uuid` derived from version + commit, so repeated
  builds of the same source are comparable.
- Source commit: `metadata.properties.source.commit`.
- Scope: the Rust dependency graph. It does not claim to inventory npm packages
  for the VS Code extension or website; those are built from their own lockfiles
  and are not part of the desktop binary.
- Secret check: generated JSON is scanned for `sk-live-`, `ghp_` and private-key
  markers before it is written.

## 3. Provenance

`actions/attest-build-provenance@v2` runs in `finalize` with `id-token: write`
and `attestations: write`, over `dist/*`. This produces a GitHub artifact
attestation bound to the workflow, repository, commit and file digests. It is
separate from a human-readable claim: the manifest records the same public
identifiers, and the attestation is verifiable with `gh attestation verify`.

No signing key, token or private environment value is placed in the manifest or
SBOM. Only public CI identifiers are recorded.

## 4. Independent verification procedure

```sh
# Download every asset plus SHA256SUMS.txt from the release page.
sha256sum -c SHA256SUMS.txt

# Inspect one entry:
grep 'free-ai-studio_3.88.15-1_amd64.deb' SHA256SUMS.txt
sha256sum free-ai-studio_3.88.15-1_amd64.deb

# Check the manifest's hashes against the files:
python3 - <<'PY'
import hashlib, json, pathlib
m = json.load(open("release-manifest.json"))
for a in m["artifacts"]:
    p = pathlib.Path(a["name"])
    assert hashlib.sha256(p.read_bytes()).hexdigest() == a["sha256"], a["name"]
print("manifest hashes match", len(m["artifacts"]), "artifacts")
PY
```

The local 3.88.15 `.deb` hash is
`686691cf762c43d4107a38b46071db1d014d0edf8f83c34313925c6529eed537`.
CI run `45694555979` produced the finalized bundle. Downloaded and verified
independently:

```
free-ai-studio_3.88.15-1_amd64.deb   cb64db73226508c8bb7ea3f2120b122f4be48e4196313e94f3b98c797c301599
free-ai-studio-android.apk           3d686c2836bf82d1b5821eebcbdff05af426573d422b436608445ac126017e98
free-ai-studio-vscode.vsix           478070c35e5d48917f65c9aa830bba2c85b1a5dee6816ce08b3230f05335d9f1

SHA256SUMS.txt covers and matches 13 files          OK
metadata valid: 523 SBOM components, 12 artifacts   OK
Attest artifact provenance                          success
```

CI artifacts are rebuilt, so their hashes differ from the local `.deb` above;
the release manifest is authoritative for each run.

## 5. Tests and tamper rejection

`scripts/ci/test-release-metadata.sh` (8/8 **PASS**) proves:

- notes refuse a version with no CHANGELOG section;
- SBOM is CycloneDX 1.5 with >100 components and no private-key material;
- manifest binds 4 fixture artifacts to version and commit;
- checksums cover and verify every file;
- **a modified artifact is rejected** by `verify-checksums.sh`;
- 3.88.14 does not require the new metadata, 3.88.15 does.

A full local finalize simulation with 14 fixtures produced:

```
manifest: 12 artifacts for 3.88.15 (...)
metadata valid: 523 SBOM components, 12 manifest artifacts, checksums verified
ok   SHA256SUMS.txt covers and matches 13 files
```

(12 = every artifact except the manifest and checksum file; 13 = those 12 plus
the manifest.)

## 6. Signing status (integrity is not authenticity)

Checksums detect corruption, not a compromised publisher. The manifest records
signing explicitly as false:

```json
"signing": {
  "windows_authenticode": false,
  "macos_developer_id": false,
  "macos_notarized": false,
  "android_release_keystore": false,
  "linux_manifest_signature": false
}
```

`check-publish-gates.sh` exits **3 BLOCKED** while any of those is false. No
unsigned artifact is described as signed. See `CODE_SIGNING_REPORT.md`.

## 7. Known gaps

- The SBOM covers Rust crates only; it is not a combined SBOM for npm/Android.
- GitHub's release upload does not itself sign; provenance attests the build,
  while platform signatures are still absent.
- Actions are referenced by version tags, not commit SHAs. This is the existing
  repository convention and remains a supply-chain hardening item.
- `SHA256SUMS.txt` cannot hash itself; its integrity comes from the release
  transport and the manifest/attestation, which are standard limitations.
