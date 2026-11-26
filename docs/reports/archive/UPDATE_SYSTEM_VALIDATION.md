# Update System Validation — Phase 9

**Version:** 3.88.15
**Engine:** `app/src/update.rs` + `app/src/installer.rs`
**State:** logic **PASS**; live network upgrade **BLOCKED** (no published release)

## 1. Pipeline

```
GET  /api/update/check      read latest release, compare versions, choose asset
POST /api/update/stage      download, verify SHA-256, write staged.json
GET  /api/update/staged     report what is staged and whether size still matches
POST /api/update/prepare    re-hash, unpack into updates/prepared/<version>
POST /api/update/apply      swap binary, keep backup, write applied.json
POST /api/update/rollback   restore the displaced build
POST /api/update/staged/discard
```

The check endpoint is read-only. Staging, preparing and applying are gated by
the `update` permission (`ask`/`deny`/saved allow). The running install is never
silently replaced: preparation records `replaces_running_binary: false`, and the
swap is an explicit step.

## 2. Security properties and where they are enforced

| Property | Implementation |
|---|---|
| Version comparison | Numeric `MAJOR.MINOR.PATCH`; `v` prefix allowed; non-numeric parts become 0, so an unparseable tag cannot look newer |
| Downgrade protection | `update_available` requires latest > current; `stage_from` refuses otherwise |
| Asset selection | Exact `_windows-x86_64.zip`, `_macos-universal.tar.gz`, `_linux-x86_64.tar.gz`; other arch/OS returns no asset |
| HTTPS only | `stage_from` refuses a non-`https://` download URL |
| Checksum source | GitHub asset `digest` when valid, else the release's `SHA256SUMS.txt`; neither → refuse |
| Corrupt download | Streamed into `<asset>.part`, hashed while writing; mismatch deletes `.part` and returns an error; previous staged file untouched |
| Interrupted download | Stream error deletes `.part`; destination is only created by rename after a matching hash |
| Archive traversal | `extract` rejects absolute paths, `..` and entries outside the destination |
| Path traversal | Staged asset names reject `/`, `\`, `..`; versions used as directory names reject `..` and unsafe characters |
| Wrong payload | Prepared tree must contain a recognisable `free-ai-studio`/`sarthi-studio` binary or it is removed and refused |
| Failed swap | Copy is byte-verified before replacement; a mismatch rolls back and keeps the old binary |
| Rollback | Restores a recorded backup; refuses an absent record, absent backup, changed binary or a backup name the app did not write |
| User data | Staging/prepared trees live under the database's data directory; config/conversations/projects are never modified |

## 3. Tests executed

`cargo test --locked`: **719 passed, 0 failed, 1 ignored** (the ignored test
deliberately downloads a real external release asset). Update-specific tests:

| Area | Tests |
|---|---|
| Version/asset/report | `versions_compare_numerically_not_alphabetically`, `the_asset_choice_follows_the_platform_and_refuses_an_unsupported_arch`, `a_newer_release_reports_the_asset_digest_and_checksum_file`, `the_same_or_an_older_release_says_there_is_no_update`, `a_release_without_a_tag_is_an_error_not_a_zero_version`, `an_unsupported_platform_still_reports_the_release_without_an_asset` |
| Refusals | `staging_is_refused_when_there_is_no_newer_release`, `staging_is_refused_when_the_platform_has_no_asset`, `a_malformed_digest_is_never_used_as_the_expected_hash`, `a_manifest_only_names_plain_assets_inside_the_staging_folder`, `preparing_is_refused_when_nothing_is_staged` |
| Download integrity | `a_verified_asset_is_staged_and_a_mismatch_leaves_the_previous_one_alone`, `a_matching_download_is_kept_and_a_mismatch_is_discarded` (installer) |
| Prepare/unpack | `preparing_refuses_a_staged_file_whose_bytes_changed`, `a_verified_zip_is_unpacked_and_its_binary_named`, `an_archive_without_a_binary_is_refused_and_cleaned_up`, `a_gzipped_tarball_prepares_the_same_way`, `a_version_used_as_a_folder_name_cannot_escape` |
| Swap/rollback | `a_swap_replaces_the_binary_and_keeps_the_previous_one`, `a_copy_that_does_not_match_is_rolled_back`, `a_missing_prepared_binary_changes_nothing`, `a_second_swap_replaces_the_previous_backup`, `leftover_backups_are_removed_but_newer_and_unknown_files_are_kept`, `the_running_binary_is_never_removed_even_if_it_looks_like_a_backup`, `a_rollback_restores_the_previous_build_and_keeps_the_new_one`, `a_rollback_is_refused_without_a_record_a_backup_or_the_same_binary`, `a_rollback_refuses_a_backup_name_this_app_did_not_write`, `a_rollback_is_recorded_beside_the_prepared_update` |

That is 25 update/installer tests inside the 719-test suite.

## 4. Executed end-to-end preservation check

The installer report's 3.88.3 → 3.88.15 run proves the data directory, database
and a created conversation survive a binary replacement. The update engine's
own staging path was not driven over the network because no 3.88.15 release
exists; its download/verify path is covered by local HTTP tests (valid bytes and
hash mismatch).

## 5. Required cases not executed

| Case | State | Note |
|---|---|---|
| Successful live update from the public release feed | **BLOCKED** | No public release was authorized/created. Any claim would be fabricated. |
| Corrupted download | **PASS** | Hash mismatch test; previous staged artifact preserved |
| Network interruption mid-download | **NOT EXERCISED** | Code deletes `.part` on stream error and renames only after hash match; no fault-injection test was run |
| Invalid checksum / missing checksum | **PASS** | Mismatch and “publishes no checksum” tests |
| Invalid signature | **NOT APPLICABLE** | Updates are checksum-verified; no artifact signature exists |
| Insufficient disk space | **NOT EXERCISED** | Write errors are returned and `.part` removed, but no ENOSPC test was run |
| Failed installation/swap | **PASS** | Copy mismatch rollback and missing-binary tests |
| Recovery after failure | **PASS** | Rollback restores previous build and keeps new build/record |
| Interrupted metadata | **PASS** | Manifest is written after verified download; malformed/missing record refuses |

## 6. Limitations

- The app prepares and can apply an update, but distribution is still a manual
  package action for Windows/macOS/Linux; there is no background auto-updater.
- Android has no in-app self-update.
- No update is accepted without a checksum, but checksums are not signatures;
  a compromised release feed could publish a matching malicious hash. Platform
  signing remains the missing trust root.
- `update` permission defaults to ask; a denied rule blocks staging/preparing.
