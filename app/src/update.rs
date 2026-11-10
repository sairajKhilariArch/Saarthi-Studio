//! Checking whether a newer release exists — and nothing more.
//!
//! The upgrade itself is deliberately out of scope here: this reads the public
//! release page over HTTPS, compares versions and names the asset a person could
//! download for this machine. It writes nothing, starts no download and installs
//! nothing, which is why it can run on demand without a permission prompt.
//! `Action::Update` guards the install step when one exists.
//!
//! Everything except `fetch` is a pure function of the release JSON, so the
//! parsing, the version order and the asset choice are all testable without a
//! network.

use crate::{installer, json_error, now_seconds, permissions, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path as FsPath, PathBuf};
use std::{cmp::Ordering, time::Duration};

/// The public release endpoint. `latest` excludes pre-releases and drafts, which
/// is what an update check should offer.
pub(crate) const RELEASE_URL: &str =
    "https://api.github.com/repos/sairajKhilariArch/Saarthi-Studio/releases/latest";

/// A release body is prose plus asset names; anything near this size is not a
/// release this app should be reading.
const MAX_BODY_BYTES: usize = 512 * 1024;

/// Long enough for a slow connection, short enough that a dead network does not
/// leave the button spinning.
const TIMEOUT: Duration = Duration::from_secs(15);

/// How much of the release notes the reply carries. The panel links out for the
/// rest rather than shipping an unbounded document to the browser.
const NOTES_CHARS: usize = 4_000;

/// The numeric core of a version, with a leading `v` allowed because tags carry
/// one. A part that is not a number counts as 0, so a tag this app cannot parse
/// never claims to be newer than what is installed.
fn numbers(version: &str) -> [u64; 3] {
    let trimmed = version.trim().trim_start_matches(['v', 'V']);
    let core = trimmed
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .to_string();
    let mut parts = core
        .split('.')
        .map(|part| part.trim().parse::<u64>().unwrap_or(0));
    [
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ]
}

/// `MAJOR.MINOR.PATCH` order, numerically: 2.9.0 is older than 2.10.0.
pub(crate) fn compare_versions(left: &str, right: &str) -> Ordering {
    numbers(left).cmp(&numbers(right))
}

/// The asset this machine would install, matched on the exact token the release
/// workflow writes. An arch with no artifact answers `None` instead of offering a
/// binary that would not run.
pub(crate) fn choose_asset(names: &[String], os: &str, arch: &str) -> Option<String> {
    let wanted = match (os, arch) {
        ("windows", "x86_64") => "_windows-x86_64.zip",
        ("macos", _) => "_macos-universal.tar.gz",
        ("linux", "x86_64") => "_linux-x86_64.tar.gz",
        // No aarch64 Linux artifact is published today; saying so is the honest
        // answer, and `alternatives` still lists what the release does carry.
        _ => return None,
    };
    names.iter().find(|name| name.ends_with(wanted)).cloned()
}

fn clipped(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= limit {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// The reply the panel renders. `current` is this build, `latest` is the newest
/// published release, and `asset` is what this platform would download.
pub(crate) fn report(
    release: &Value,
    os: &str,
    arch: &str,
    current: &str,
) -> Result<Value, String> {
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| "The release has no tag, so its version is unknown.".to_string())?;
    let latest = tag.trim_start_matches(['v', 'V']).to_string();

    let assets = release
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let names: Vec<String> = assets
        .iter()
        .filter_map(|asset| asset.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let chosen = choose_asset(&names, os, arch);

    let asset = chosen.as_ref().and_then(|name| {
        assets
            .iter()
            .find(|asset| asset.get("name").and_then(Value::as_str) == Some(name.as_str()))
            .map(|asset| {
                json!({
                    "name": name,
                    // `digest` is GitHub's own hash ("sha256:…") when it has one;
                    // null is reported as null rather than guessed.
                    "digest": asset.get("digest").cloned().unwrap_or(Value::Null),
                    "size": asset.get("size").cloned().unwrap_or(Value::Null),
                    "url": asset.get("browser_download_url").cloned().unwrap_or(Value::Null),
                })
            })
    });

    let checksum_url = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some("SHA256SUMS.txt"))
        .and_then(|asset| asset.get("browser_download_url").cloned())
        .unwrap_or(Value::Null);

    let others: Vec<String> = names
        .iter()
        .filter(|name| Some(*name) != chosen.as_ref())
        .take(12)
        .cloned()
        .collect();

    Ok(json!({
        "ok": true,
        "current": current,
        "latest": latest,
        "update_available": compare_versions(&latest, current) == Ordering::Greater,
        "platform": format!("{os}/{arch}"),
        "asset": asset,
        "checksum_url": checksum_url,
        "other_assets": others,
        "release": {
            "name": release.get("name").cloned().unwrap_or(Value::Null),
            "tag": tag,
            "url": release.get("html_url").cloned().unwrap_or(Value::Null),
            "published_at": release.get("published_at").cloned().unwrap_or(Value::Null),
            "notes": release
                .get("body")
                .and_then(Value::as_str)
                .map(|body| clipped(body, NOTES_CHARS))
                .unwrap_or_default(),
        },
        "checked_at": now_seconds(),
        // Said in the payload so the panel does not have to imply otherwise.
        "installs_nothing": true,
    }))
}

fn platform() -> (&'static str, &'static str) {
    (std::env::consts::OS, std::env::consts::ARCH)
}

async fn fetch(state: &AppState) -> Result<Value, String> {
    let response = state
        .client
        .get(RELEASE_URL)
        .timeout(TIMEOUT)
        .header("accept", "application/vnd.github+json")
        .header(
            "user-agent",
            format!("saarthi-studio/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|error| format!("Could not reach the release API: {error}"))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("The release API stopped mid-answer: {error}"))?;
    if !status.is_success() {
        return Err(format!("The release API answered {}.", status.as_u16()));
    }
    if bytes.len() > MAX_BODY_BYTES {
        return Err(format!(
            "The release API answered {} bytes, which is too large to be a release.",
            bytes.len()
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("The release API answered something that is not JSON: {error}"))
}

/// `GET /api/update/check` — reads the newest published release and reports it
/// against this build. No download, no install, no writes.
pub(crate) async fn check_route(State(state): State<AppState>) -> axum::response::Response {
    match fetch(&state).await {
        Ok(release) => {
            let (os, arch) = platform();
            match report(&release, os, arch, env!("CARGO_PKG_VERSION")) {
                Ok(payload) => Json(payload).into_response(),
                Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
            }
        }
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
    }
}

/* ---- staging a verified download ---------------------------------- */

/// Where a verified download waits until somebody applies it. Beside the
/// database, so a staged update travels with the same data directory a backup
/// does and never lands in the install tree it would replace.
pub(crate) fn staged_dir(data_dir: &FsPath) -> PathBuf {
    data_dir.join("updates")
}

fn staged_manifest_path(data_dir: &FsPath) -> PathBuf {
    staged_dir(data_dir).join("staged.json")
}

/// The data directory, taken from the database path the app already resolved.
fn data_dir(state: &AppState) -> Result<PathBuf, String> {
    state
        .database_path
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "The database has no parent directory to stage an update in.".to_string())
}

/// Why a stage attempt stopped. The distinction is the HTTP status the panel
/// shows: a refusal is about this machine or this release, a failure is about
/// the network.
#[derive(Debug)]
pub(crate) enum StageError {
    Refused(String),
    Failed(String),
}

impl StageError {
    fn message(&self) -> &str {
        match self {
            Self::Refused(message) | Self::Failed(message) => message,
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::Refused(_) => StatusCode::CONFLICT,
            Self::Failed(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

/// A staged file is only ever addressed by a plain asset name. The manifest is a
/// file on disk, so a name with a separator or `..` in it is refused rather than
/// joined.
fn safe_staged_file(data_dir: &FsPath, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    Some(staged_dir(data_dir).join(name))
}

fn read_manifest(data_dir: &FsPath) -> Option<Value> {
    let bytes = fs::read(staged_manifest_path(data_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// What is staged right now, with whether the file is still where the manifest
/// says it is and still the size it recorded. Re-hashing a 100 MB archive on
/// every read is not worth it; the hash was checked at download time.
pub(crate) fn staged(data_dir: &FsPath) -> Value {
    let Some(manifest) = read_manifest(data_dir) else {
        return json!({"ok": true, "staged": Value::Null, "present": false, "size_matches": false});
    };
    let name = manifest
        .get("asset")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let file = safe_staged_file(data_dir, name);
    let present = file.as_ref().map(|path| path.is_file()).unwrap_or(false);
    let size_on_disk = file
        .as_ref()
        .filter(|_| present)
        .and_then(|path| fs::metadata(path).ok())
        .map(|meta| meta.len());
    let expected = manifest.get("bytes").and_then(Value::as_u64);
    json!({
        "ok": true,
        "staged": manifest,
        "present": present,
        "size_on_disk": size_on_disk,
        "size_matches": matches!((size_on_disk, expected), (Some(actual), Some(wanted)) if actual == wanted),
    })
}

/// Remove the staged file and its manifest. Answers whether anything was there.
pub(crate) fn discard_staged(data_dir: &FsPath) -> Result<bool, String> {
    let mut removed = false;
    if let Some(manifest) = read_manifest(data_dir) {
        if let Some(name) = manifest.get("asset").and_then(Value::as_str) {
            if let Some(file) = safe_staged_file(data_dir, name) {
                if file.is_file() {
                    fs::remove_file(&file)
                        .map_err(|error| format!("Could not remove {}: {error}", file.display()))?;
                    removed = true;
                }
            }
        }
    }
    let manifest_path = staged_manifest_path(data_dir);
    if manifest_path.is_file() {
        fs::remove_file(&manifest_path)
            .map_err(|error| format!("Could not remove {}: {error}", manifest_path.display()))?;
        removed = true;
    }
    Ok(removed)
}

/// Download one asset into the staging folder, verify it against `checksum`, and
/// record it.
///
/// The download goes to a `.part` name and is only renamed once the hash matches
/// (`installer::download_verified`), so a failure leaves the previous staged
/// update untouched instead of replacing a good file with a broken one.
pub(crate) async fn stage_asset(
    dir: &FsPath,
    name: &str,
    url: &str,
    checksum: &str,
    meta: &Value,
) -> Result<Value, StageError> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(StageError::Refused(format!(
            "{name:?} is not a plain asset name."
        )));
    }
    fs::create_dir_all(dir).map_err(|error| {
        StageError::Failed(format!("Could not create {}: {error}", dir.display()))
    })?;
    let dest = dir.join(name);
    let bytes = installer::download_verified(url, checksum, &dest)
        .await
        .map_err(StageError::Failed)?;

    // One staged update at a time: everything else in the folder belonged to an
    // older candidate and is now stale.
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path != dest {
                let _ = fs::remove_file(path);
            }
        }
    }

    let mut manifest = meta.clone();
    manifest["asset"] = json!(name);
    manifest["sha256"] = json!(checksum);
    manifest["bytes"] = json!(bytes);
    manifest["staged_at"] = json!(now_seconds());
    manifest["path"] = json!(dest.to_string_lossy());
    let encoded = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        StageError::Failed(format!("Could not encode the staged manifest: {error}"))
    })?;
    // `dir` is the staging folder itself; the manifest lives beside the file it
    // describes, and `staged()`/`discard_staged()` reach it through `staged_dir`.
    let manifest_path = dir.join("staged.json");
    fs::write(&manifest_path, encoded).map_err(|error| {
        StageError::Failed(format!(
            "The download verified but the manifest could not be written to {}: {error}",
            manifest_path.display()
        ))
    })?;
    Ok(manifest)
}

/// The published sha256 for one asset: GitHub's own `digest` when the release
/// carries one, otherwise the entry in the release's `SHA256SUMS.txt`. A release
/// that publishes neither is refused — an unverifiable download is not installed.
async fn published_checksum(
    client: &reqwest::Client,
    release: &Value,
    name: &str,
) -> Result<String, StageError> {
    let asset = release
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset.get("name").and_then(Value::as_str) == Some(name))
        });
    if let Some(digest) = asset
        .and_then(|asset| asset.get("digest"))
        .and_then(Value::as_str)
    {
        let hash = digest
            .trim()
            .trim_start_matches("sha256:")
            .to_ascii_lowercase();
        if hash.len() == 64 && hash.chars().all(|character| character.is_ascii_hexdigit()) {
            return Ok(hash);
        }
    }

    let sums_url = release
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset.get("name").and_then(Value::as_str) == Some("SHA256SUMS.txt"))
        })
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            StageError::Refused(format!(
                "The release publishes no checksum for {name}, so it was not downloaded."
            ))
        })?;
    let response = client
        .get(sums_url)
        .timeout(TIMEOUT)
        .header(
            "user-agent",
            format!("saarthi-studio/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|error| StageError::Failed(format!("Could not read SHA256SUMS.txt: {error}")))?;
    if !response.status().is_success() {
        return Err(StageError::Failed(format!(
            "SHA256SUMS.txt answered {}.",
            response.status().as_u16()
        )));
    }
    let text = response.text().await.map_err(|error| {
        StageError::Failed(format!("SHA256SUMS.txt stopped mid-answer: {error}"))
    })?;
    installer::parse_sha256sums(&text)
        .get(&name.to_ascii_lowercase())
        .cloned()
        .ok_or_else(|| {
            StageError::Refused(format!(
                "{name} is not listed in the release's SHA256SUMS.txt, so it was not downloaded."
            ))
        })
}

/// Decide, verify and stage — everything a stage request does after the release
/// JSON is in hand, which is also everything a test can drive without GitHub.
pub(crate) async fn stage_from(
    client: &reqwest::Client,
    data_dir: &FsPath,
    release: &Value,
) -> Result<Value, StageError> {
    let (os, arch) = platform();
    let current = env!("CARGO_PKG_VERSION");
    let report = report(release, os, arch, current).map_err(StageError::Refused)?;
    let latest = report["latest"].as_str().unwrap_or_default().to_string();
    if report["update_available"] != Value::Bool(true) {
        return Err(StageError::Refused(format!(
            "This build is already at the newest release ({current}); nothing was downloaded."
        )));
    }
    let asset = report
        .get("asset")
        .cloned()
        .filter(|asset| !asset.is_null())
        .ok_or_else(|| {
            StageError::Refused(format!(
                "No {} build is published for {latest}, so nothing was downloaded.",
                report["platform"].as_str().unwrap_or("platform")
            ))
        })?;
    let name = asset["name"].as_str().unwrap_or_default().to_string();
    let url = asset["url"].as_str().unwrap_or_default().to_string();
    // Downloaded binaries are the one place a plain-http redirect must not be
    // followed quietly.
    if !url.starts_with("https://") {
        return Err(StageError::Refused(format!(
            "{name} has no https download URL, so nothing was downloaded."
        )));
    }
    let checksum = published_checksum(client, release, &name).await?;
    let meta = json!({
        "format": "saarthi.update.staged",
        "version": 1,
        "latest": latest,
        "current": current,
        "release_url": report["release"]["url"].clone(),
    });
    stage_asset(&staged_dir(data_dir), &name, &url, &checksum, &meta).await
}

/// The `update` gate shared by staging and preparing.
///
/// `Ok(scope)` means the attempt may run and the audit row records that scope;
/// `Err` is the response to send instead. `needs_approval` is what the UI's
/// `api()` helper looks for before it asks the person and retries once with
/// `approve: true`.
enum Gate {
    Allowed(permissions::Scope),
    Stop(axum::response::Response),
}

fn gate_update(state: &AppState, approve: bool, what: &str) -> Gate {
    let target = "release";
    let verdict = permissions::check(state, permissions::Action::Update, target, "");
    if verdict.decision == permissions::Decision::Deny {
        permissions::record(
            state,
            permissions::Action::Update,
            target,
            permissions::Decision::Deny,
            verdict.scope,
            "",
            &format!("{what} refused by a saved rule"),
        );
        return Gate::Stop(json_error(
            StatusCode::FORBIDDEN,
            format!("A saved permission rule denies {what}. Revoke it in Settings → Permissions, then retry."),
        ));
    }
    if verdict.decision == permissions::Decision::Ask && !approve {
        permissions::record(
            state,
            permissions::Action::Update,
            target,
            permissions::Decision::Ask,
            verdict.scope,
            "",
            &format!("{what} awaits approval"),
        );
        return Gate::Stop(
            (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": format!("{what} writes into this app's data directory. Approve it once, or save an allow rule for update."),
                    "needs_approval": true,
                })),
            )
                .into_response(),
        );
    }
    Gate::Allowed(verdict.scope)
}

/// `POST /api/update/stage` — download, verify and stage the asset the check
/// named. Gated on `update`: `ask` needs `{"approve": true}`, `deny` refuses by
/// rule, and a saved allow lets it run.
pub(crate) async fn stage_route(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> axum::response::Response {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let scope = match gate_update(&state, approve, "downloading an update") {
        Gate::Allowed(scope) => scope,
        Gate::Stop(response) => return response,
    };

    let dir = match data_dir(&state) {
        Ok(dir) => dir,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let release = match fetch(&state).await {
        Ok(release) => release,
        Err(error) => return json_error(StatusCode::BAD_GATEWAY, error),
    };
    match stage_from(&state.client, &dir, &release).await {
        Ok(manifest) => {
            permissions::record(
                &state,
                permissions::Action::Update,
                "release",
                permissions::Decision::Allow,
                scope,
                "",
                "update downloaded, verified and staged",
            );
            Json(json!({"ok": true, "staged": manifest})).into_response()
        }
        Err(error) => json_error(error.status(), error.message().to_string()),
    }
}

/// `GET /api/update/staged` — what is waiting, and whether the file is still
/// there. Read-only over the app's own data directory, so it is not gated.
pub(crate) async fn staged_route(State(state): State<AppState>) -> axum::response::Response {
    match data_dir(&state) {
        Ok(dir) => Json(staged(&dir)).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

/// `POST /api/update/staged/discard` — delete a staged download. Removing a file
/// this app wrote itself is never a privileged action, and it must stay possible
/// even when `update` is denied.
pub(crate) async fn discard_route(State(state): State<AppState>) -> axum::response::Response {
    let dir = match data_dir(&state) {
        Ok(dir) => dir,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    match discard_staged(&dir) {
        Ok(removed) => Json(json!({"ok": true, "removed": removed})).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

/* ---- preparing a staged download ---------------------------------- */

/// Where an unpacked release waits: inside the same `updates` folder as staging,
/// so it travels with the data directory and never lands in the install tree the
/// running build was started from.
pub(crate) fn prepared_dir(data_dir: &FsPath) -> PathBuf {
    staged_dir(data_dir).join("prepared")
}

fn prepared_manifest_path(data_dir: &FsPath) -> PathBuf {
    prepared_dir(data_dir).join("prepared.json")
}

/// The archive kind an asset name describes. `None` means this build has no
/// extractor for it, which is refused rather than guessed at.
fn archive_of(name: &str) -> Option<installer::Archive> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        Some(installer::Archive::Zip)
    } else if lower.ends_with(".tar.gz") {
        Some(installer::Archive::TarGz)
    } else if lower.ends_with(".tar.zst") {
        Some(installer::Archive::TarZst)
    } else {
        None
    }
}

/// A version becomes a directory name, so only characters that cannot escape or
/// confuse a path survive.
fn version_dir_name(version: &str) -> Option<String> {
    let trimmed = version.trim().trim_start_matches(['v', 'V']);
    let usable = !trimmed.is_empty()
        && trimmed.len() <= 40
        && !trimmed.contains("..")
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        });
    usable.then(|| trimmed.to_string())
}

/// How strongly a file name looks like the binary that makes a release runnable.
/// The crate's own name wins; a bundle named after the product is accepted second.
fn app_binary_rank(path: &FsPath) -> Option<u8> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    match stem {
        "free-ai-studio" | "saarthi-studio" => Some(0),
        _ if stem.starts_with("free-ai-studio") => Some(1),
        _ if stem.starts_with("saarthi") => Some(2),
        _ => None,
    }
}

/// Find the binary inside an unpacked release. Bounded in depth and entries so a
/// hostile archive cannot make the app walk forever.
fn find_app_binary(root: &FsPath) -> Option<PathBuf> {
    let mut best: Option<(u8, PathBuf)> = None;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if depth > 6 || seen > 5_000 {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push((entry.path(), depth + 1));
                continue;
            }
            let path = entry.path();
            let Some(rank) = app_binary_rank(&path) else {
                continue;
            };
            if best
                .as_ref()
                .map(|(best_rank, _)| rank < *best_rank)
                .unwrap_or(true)
            {
                best = Some((rank, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// Verify a staged download again and unpack it into `updates/prepared/<version>`.
///
/// The hash is re-checked because this is the first step that acts on the bytes:
/// `/api/update/staged` compares sizes only, which notices a deletion and is not
/// enough to trust the file again. A tree without a runnable binary is removed and
/// refused — unpacking something that cannot start is not a prepared update.
pub(crate) fn prepare_from(data_dir: &FsPath, staged: &Value) -> Result<Value, StageError> {
    let name = staged
        .get("asset")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let recorded = staged
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version = staged
        .get("latest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(file) = safe_staged_file(data_dir, name) else {
        return Err(StageError::Refused(
            "Nothing is staged: download an update first.".to_string(),
        ));
    };
    if !file.is_file() {
        return Err(StageError::Refused(format!(
            "The staged file {name} is missing; download the update again."
        )));
    }
    let Some(version) = version_dir_name(version) else {
        return Err(StageError::Refused(format!(
            "{version:?} is not a version this app will unpack into a folder."
        )));
    };
    let Some(archive) = archive_of(name) else {
        return Err(StageError::Refused(format!(
            "{name} is not an archive this build can unpack (.zip, .tar.gz or .tar.zst)."
        )));
    };
    if recorded.len() != 64
        || !recorded
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(StageError::Refused(format!(
            "The staged record has no usable sha256 for {name}; download the update again."
        )));
    }
    let actual = installer::sha256_file(&file).map_err(StageError::Failed)?;
    if !actual.eq_ignore_ascii_case(recorded) {
        return Err(StageError::Refused(
            "The staged file no longer matches the sha256 it was verified at, so it was not unpacked. Discard it and download the update again.".to_string(),
        ));
    }

    let root = prepared_dir(data_dir);
    let target = root.join(&version);
    if target.exists() {
        fs::remove_dir_all(&target).map_err(|error| {
            StageError::Failed(format!("Could not clear {}: {error}", target.display()))
        })?;
    }
    let files = installer::extract(archive, &file, &target).map_err(|error| {
        let _ = fs::remove_dir_all(&target);
        StageError::Failed(error)
    })?;
    let Some(binary) = find_app_binary(&target) else {
        let _ = fs::remove_dir_all(&target);
        return Err(StageError::Refused(format!(
            "{name} unpacks to {files} files but none of them is a Saarthi Studio binary, so it was not prepared."
        )));
    };
    let relative = binary
        .strip_prefix(&target)
        .unwrap_or(&binary)
        .to_string_lossy()
        .to_string();
    let prepared = json!({
        "format": "saarthi.update.prepared",
        "version": 1,
        "latest": version,
        "current": env!("CARGO_PKG_VERSION"),
        "asset": name,
        "sha256": actual,
        "bytes": staged.get("bytes").cloned().unwrap_or(Value::Null),
        "root": target.to_string_lossy(),
        "binary": relative,
        "binary_path": binary.to_string_lossy(),
        "files": files,
        "prepared_at": now_seconds(),
        // The running build is untouched on purpose, so the payload says so.
        "replaces_running_binary": false,
    });
    let encoded = serde_json::to_vec_pretty(&prepared).map_err(|error| {
        StageError::Failed(format!("Could not encode the prepared record: {error}"))
    })?;
    fs::write(prepared_manifest_path(data_dir), encoded).map_err(|error| {
        StageError::Failed(format!(
            "The release unpacked but its record could not be written: {error}"
        ))
    })?;
    Ok(prepared)
}

/// What is prepared right now, with whether the unpacked tree and its binary are
/// still there.
pub(crate) fn prepared(data_dir: &FsPath) -> Value {
    let manifest = fs::read(prepared_manifest_path(data_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(manifest) = manifest else {
        return json!({"ok": true, "prepared": Value::Null, "present": false, "binary_exists": false});
    };
    let present = manifest
        .get("root")
        .and_then(Value::as_str)
        .map(|root| !root.is_empty() && FsPath::new(root).is_dir())
        .unwrap_or(false);
    let binary_exists = manifest
        .get("binary_path")
        .and_then(Value::as_str)
        .map(|path| FsPath::new(path).is_file())
        .unwrap_or(false);
    json!({"ok": true, "prepared": manifest, "present": present, "binary_exists": binary_exists})
}

/// Remove an unpacked release and its record. Like discarding a staged download,
/// this only touches files the app wrote itself, so it is never gated.
pub(crate) fn discard_prepared(data_dir: &FsPath) -> Result<bool, String> {
    let mut removed = false;
    let version = fs::read(prepared_manifest_path(data_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|manifest| {
            manifest
                .get("latest")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .and_then(|version| version_dir_name(&version));
    if let Some(version) = version {
        let target = prepared_dir(data_dir).join(version);
        if target.is_dir() {
            fs::remove_dir_all(&target)
                .map_err(|error| format!("Could not remove {}: {error}", target.display()))?;
            removed = true;
        }
    }
    let manifest = prepared_manifest_path(data_dir);
    if manifest.is_file() {
        fs::remove_file(&manifest)
            .map_err(|error| format!("Could not remove {}: {error}", manifest.display()))?;
        removed = true;
    }
    Ok(removed)
}

/// `POST /api/update/prepare` — verify a staged download again, unpack it beside
/// the data directory and name the binary that makes it runnable. Gated on
/// `update` exactly like the download, because writing an executable this app made
/// from a release is the same kind of act.
pub(crate) async fn prepare_route(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> axum::response::Response {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let scope = match gate_update(&state, approve, "unpacking the staged update") {
        Gate::Allowed(scope) => scope,
        Gate::Stop(response) => return response,
    };
    let dir = match data_dir(&state) {
        Ok(dir) => dir,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let staged = read_manifest(&dir);
    let Some(staged) = staged.filter(|staged| !staged.is_null()) else {
        return json_error(
            StatusCode::CONFLICT,
            "Nothing is staged to prepare. Download an update first.",
        );
    };
    match prepare_from(&dir, &staged) {
        Ok(prepared) => {
            permissions::record(
                &state,
                permissions::Action::Update,
                "release",
                permissions::Decision::Allow,
                scope,
                "",
                "staged update verified and unpacked",
            );
            Json(json!({"ok": true, "prepared": prepared})).into_response()
        }
        Err(error) => json_error(error.status(), error.message().to_string()),
    }
}

/// `GET /api/update/prepared` — what is unpacked and whether its binary is still
/// there. Read-only over the app's own data directory, so it is not gated.
pub(crate) async fn prepared_route(State(state): State<AppState>) -> axum::response::Response {
    match data_dir(&state) {
        Ok(dir) => Json(prepared(&dir)).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

/// `POST /api/update/prepared/discard` — remove an unpacked release.
pub(crate) async fn discard_prepared_route(
    State(state): State<AppState>,
) -> axum::response::Response {
    let dir = match data_dir(&state) {
        Ok(dir) => dir,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    match discard_prepared(&dir) {
        Ok(removed) => Json(json!({"ok": true, "removed": removed})).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "free-ai-studio-update-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn staging_is_refused_when_there_is_no_newer_release() {
        let dir = scratch("same-version");
        let client = reqwest::Client::new();
        let error = stage_from(&client, &dir, &release("2.35.0", &[]))
            .await
            .expect_err("already newest");
        assert!(matches!(error, StageError::Refused(_)), "{error:?}");
        assert!(
            error.message().contains("already at the newest release"),
            "{}",
            error.message()
        );
        // Nothing was created for a request that never downloaded anything.
        assert!(!staged_dir(&dir).exists());
    }

    #[tokio::test]
    async fn staging_is_refused_when_the_platform_has_no_asset() {
        let dir = scratch("no-asset");
        let client = reqwest::Client::new();
        // A newer release, but nothing this machine could install: the release
        // only carries a Windows zip and freebsd is the platform under test
        // through choose_asset, which stage_from reaches via report().
        let payload = report(
            &release("9.9.9", &["free-ai-suite_9.9.9_freebsd-x86_64.tar.gz"]),
            "freebsd",
            "x86_64",
            "1.0.0",
        )
        .expect("report");
        assert!(payload["asset"].is_null());
        // On this host the asset is chosen for the real platform, so a release
        // with only a foreign asset must still refuse rather than guess.
        let error = stage_from(&client, &dir, &release("9.9.9", &["SHA256SUMS.txt"]))
            .await
            .expect_err("no asset");
        assert!(matches!(error, StageError::Refused(_)), "{error:?}");
        assert!(
            error.message().contains("No ") && error.message().contains("build is published"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn a_manifest_only_names_plain_assets_inside_the_staging_folder() {
        let dir = scratch("containment");
        assert!(safe_staged_file(&dir, "../../etc/passwd").is_none());
        assert!(safe_staged_file(&dir, "a/b.zip").is_none());
        assert!(safe_staged_file(&dir, "a\\b.zip").is_none());
        assert!(safe_staged_file(&dir, "").is_none());
        assert_eq!(
            safe_staged_file(&dir, "free-ai-suite_2.37.0_windows-x86_64.zip"),
            Some(staged_dir(&dir).join("free-ai-suite_2.37.0_windows-x86_64.zip"))
        );
    }

    #[test]
    fn a_malformed_digest_is_never_used_as_the_expected_hash() {
        let mut broken = release("9.9.9", &["free-ai-suite_9.9.9_windows-x86_64.zip"]);
        broken["assets"][0]["digest"] = json!("sha256:not-a-hash");
        // No SHA256SUMS.txt in this release either, so the refusal is the only
        // correct answer: an unverifiable download cannot be installed.
        let client = reqwest::Client::new();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let error = runtime
            .block_on(published_checksum(
                &client,
                &broken,
                "free-ai-suite_9.9.9_windows-x86_64.zip",
            ))
            .expect_err("no usable checksum");
        assert!(
            error.message().contains("publishes no checksum"),
            "{}",
            error.message()
        );
    }

    async fn serve(body: Vec<u8>) -> String {
        use axum::{routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/asset.bin",
            get(move || {
                let body = body.clone();
                async move { body }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{address}/asset.bin")
    }

    #[tokio::test]
    async fn a_verified_asset_is_staged_and_a_mismatch_leaves_the_previous_one_alone() {
        use sha2::{Digest, Sha256};
        let body = b"release bytes".to_vec();
        let mut hasher = Sha256::new();
        hasher.update(&body);
        let checksum: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let url = serve(body.clone()).await;
        let dir = scratch("stage");
        let meta = json!({"format": "saarthi.update.staged", "version": 1, "latest": "2.37.0", "current": "2.36.0"});
        let name = "free-ai-suite_2.37.0_windows-x86_64.zip";

        let manifest = stage_asset(&staged_dir(&dir), name, &url, &checksum, &meta)
            .await
            .expect("stage");
        assert_eq!(manifest["bytes"], body.len() as u64);
        assert_eq!(manifest["sha256"], checksum);
        assert!(staged_dir(&dir).join(name).is_file());
        let seen = staged(&dir);
        assert_eq!(seen["present"], true);
        assert_eq!(seen["size_matches"], true);
        assert_eq!(seen["staged"]["latest"], "2.37.0");

        // A hash mismatch is refused and leaves the verified file and its
        // manifest exactly where they were.
        let error = stage_asset(&staged_dir(&dir), name, &url, &"0".repeat(64), &meta)
            .await
            .expect_err("mismatch");
        assert!(matches!(error, StageError::Failed(_)), "{error:?}");
        assert!(staged_dir(&dir).join(name).is_file());
        let after = staged(&dir);
        assert_eq!(after["present"], true);
        assert_eq!(after["staged"]["sha256"], checksum);

        // Discarding removes both the file and the record.
        assert!(discard_staged(&dir).expect("discard"));
        assert!(!staged_dir(&dir).join(name).exists());
        let empty = staged(&dir);
        assert_eq!(empty["staged"], Value::Null);
        assert_eq!(empty["present"], false);
        let _ = fs::remove_dir_all(&dir);
    }

    fn release(version: &str, assets: &[&str]) -> Value {
        json!({
            "tag_name": format!("v{version}"),
            "name": format!("Saarthi Studio {version}"),
            "html_url": format!("https://github.com/sairajKhilariArch/Saarthi-Studio/releases/tag/v{version}"),
            "published_at": "2026-11-10T00:00:00Z",
            "body": "Release notes here.",
            "assets": assets
                .iter()
                .map(|name| json!({
                    "name": name,
                    "size": 1024,
                    "browser_download_url": format!("https://example.invalid/{name}"),
                    "digest": "sha256:abc",
                }))
                .collect::<Vec<_>>(),
        })
    }

    #[test]
    fn versions_compare_numerically_not_alphabetically() {
        assert_eq!(compare_versions("2.10.0", "2.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("2.9.0", "2.10.0"), Ordering::Less);
        assert_eq!(compare_versions("v2.35.0", "2.35.0"), Ordering::Equal);
        assert_eq!(compare_versions("10.0.0", "9.9.9"), Ordering::Greater);
        // A tag that is not a version is older than anything, never newer.
        assert_eq!(compare_versions("nightly", "2.35.0"), Ordering::Less);
        assert_eq!(compare_versions("2.36.0-rc.1", "2.36.0"), Ordering::Equal);
    }

    #[test]
    fn the_asset_choice_follows_the_platform_and_refuses_an_unsupported_arch() {
        let names: Vec<String> = [
            "free-ai-suite_2.36.0_linux-x86_64.tar.gz",
            "free-ai-suite_2.36.0_windows-x86_64.zip",
            "free-ai-suite_2.36.0_macos-universal.tar.gz",
            "free-ai-suite_2.36.0_x86_64.AppImage",
            "free-ai-studio_2.36.0-1_amd64.deb",
            "SHA256SUMS.txt",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect();

        assert_eq!(
            choose_asset(&names, "windows", "x86_64").as_deref(),
            Some("free-ai-suite_2.36.0_windows-x86_64.zip")
        );
        // The portable tarball, not the .deb or the AppImage: no root, no fuse,
        // and the same file the Linux release already smokes.
        assert_eq!(
            choose_asset(&names, "linux", "x86_64").as_deref(),
            Some("free-ai-suite_2.36.0_linux-x86_64.tar.gz")
        );
        assert_eq!(
            choose_asset(&names, "macos", "aarch64").as_deref(),
            Some("free-ai-suite_2.36.0_macos-universal.tar.gz")
        );
        // Nothing is published for this target, so nothing is offered.
        assert_eq!(choose_asset(&names, "linux", "aarch64"), None);
    }

    #[test]
    fn a_newer_release_reports_the_asset_digest_and_checksum_file() {
        let payload = report(
            &release(
                "2.36.0",
                &["free-ai-suite_2.36.0_windows-x86_64.zip", "SHA256SUMS.txt"],
            ),
            "windows",
            "x86_64",
            "2.35.0",
        )
        .expect("report");

        assert_eq!(payload["update_available"], true);
        assert_eq!(payload["current"], "2.35.0");
        assert_eq!(payload["latest"], "2.36.0");
        assert_eq!(
            payload["asset"]["name"],
            "free-ai-suite_2.36.0_windows-x86_64.zip"
        );
        assert_eq!(payload["asset"]["digest"], "sha256:abc");
        assert_eq!(payload["asset"]["size"], 1024);
        assert_eq!(
            payload["checksum_url"],
            "https://example.invalid/SHA256SUMS.txt"
        );
        assert_eq!(payload["installs_nothing"], true);
        assert_eq!(payload["release"]["tag"], "v2.36.0");
    }

    #[test]
    fn the_same_or_an_older_release_says_there_is_no_update() {
        let same = report(
            &release("2.35.0", &["free-ai-suite_2.35.0_windows-x86_64.zip"]),
            "windows",
            "x86_64",
            "2.35.0",
        )
        .expect("report");
        assert_eq!(same["update_available"], false);

        let older = report(
            &release("2.34.0", &["free-ai-suite_2.34.0_windows-x86_64.zip"]),
            "windows",
            "x86_64",
            "2.35.0",
        )
        .expect("report");
        assert_eq!(older["update_available"], false);
    }

    #[test]
    fn a_release_without_a_tag_is_an_error_not_a_zero_version() {
        let broken = json!({ "name": "no tag", "assets": [] });
        let error = report(&broken, "windows", "x86_64", "2.35.0").expect_err("no tag");
        assert!(error.contains("no tag"), "{error}");
    }

    #[test]
    fn an_unsupported_platform_still_reports_the_release_without_an_asset() {
        let payload = report(
            &release("2.36.0", &["free-ai-suite_2.36.0_windows-x86_64.zip"]),
            "freebsd",
            "x86_64",
            "2.35.0",
        )
        .expect("report");
        assert_eq!(payload["update_available"], true);
        assert_eq!(payload["asset"], Value::Null);
        // What the release does carry is still named, so the panel is not blank.
        assert!(payload["other_assets"]
            .as_array()
            .expect("list")
            .iter()
            .any(|name| name == "free-ai-suite_2.36.0_windows-x86_64.zip"));
    }

    fn write_zip(path: &FsPath, entries: &[(&str, &[u8])]) {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).expect("zip"));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            writer.start_file(*name, options).expect("entry");
            writer.write_all(body).expect("write");
        }
        writer.finish().expect("finish");
    }

    fn staged_record(version: &str, asset: &str, sha: &str) -> Value {
        json!({
            "format": "saarthi.update.staged",
            "version": 1,
            "latest": version,
            "asset": asset,
            "sha256": sha,
            "bytes": 12,
        })
    }

    #[test]
    fn preparing_is_refused_when_nothing_is_staged() {
        let dir = scratch("prepare-empty");
        // No asset name at all: there is nothing to point at.
        let error = prepare_from(&dir, &staged_record("9.9.9", "", &"0".repeat(64)))
            .expect_err("nothing staged");
        assert!(
            error.message().contains("Nothing is staged"),
            "{}",
            error.message()
        );
        // A record whose file was removed says so instead of unpacking nothing.
        let error = prepare_from(
            &dir,
            &staged_record("9.9.9", "release.zip", &"0".repeat(64)),
        )
        .expect_err("missing file");
        assert!(
            error.message().contains("is missing"),
            "{}",
            error.message()
        );
        assert!(!prepared_dir(&dir).exists());
    }

    #[test]
    fn preparing_refuses_a_staged_file_whose_bytes_changed() {
        let dir = scratch("prepare-changed");
        let staging = staged_dir(&dir);
        fs::create_dir_all(&staging).expect("staging");
        let name = "free-ai-suite_9.9.9_windows-x86_64.zip";
        fs::write(staging.join(name), b"replaced after verification").expect("write");
        // A well-formed hash that is simply not this file's hash.
        let error = prepare_from(&dir, &staged_record("9.9.9", name, &"a".repeat(64)))
            .expect_err("changed bytes");
        assert!(matches!(error, StageError::Refused(_)), "{error:?}");
        assert!(
            error.message().contains("no longer matches"),
            "{}",
            error.message()
        );
        // Nothing is unpacked from a file that cannot be trusted again.
        assert!(!prepared_dir(&dir).join("9.9.9").exists());
    }

    #[test]
    fn a_verified_zip_is_unpacked_and_its_binary_named() {
        let dir = scratch("prepare-zip");
        let staging = staged_dir(&dir);
        fs::create_dir_all(&staging).expect("staging");
        let name = "free-ai-suite_9.9.9_windows-x86_64.zip";
        let archive = staging.join(name);
        write_zip(
            &archive,
            &[
                ("free-ai-studio.exe", b"binary"),
                ("static/index.html", b"<html></html>"),
                ("lib/webview.dll", b"dll"),
            ],
        );
        let sha = installer::sha256_file(&archive).expect("hash");
        let prepared = prepare_from(&dir, &staged_record("9.9.9", name, &sha)).expect("prepare");

        assert_eq!(prepared["latest"], "9.9.9");
        assert_eq!(prepared["binary"], "free-ai-studio.exe");
        assert_eq!(prepared["files"], 3);
        assert_eq!(prepared["sha256"], sha);
        assert_eq!(prepared["replaces_running_binary"], false);
        assert!(FsPath::new(prepared["binary_path"].as_str().expect("path")).is_file());

        let seen = self::prepared(&dir);
        assert_eq!(seen["present"], true);
        assert_eq!(seen["binary_exists"], true);
        assert_eq!(seen["prepared"]["latest"], "9.9.9");

        assert!(discard_prepared(&dir).expect("discard"));
        let gone = self::prepared(&dir);
        assert_eq!(gone["present"], false);
        assert_eq!(gone["prepared"], Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_without_a_binary_is_refused_and_cleaned_up() {
        let dir = scratch("prepare-no-binary");
        let staging = staged_dir(&dir);
        fs::create_dir_all(&staging).expect("staging");
        let name = "free-ai-suite_9.9.9_windows-x86_64.zip";
        let archive = staging.join(name);
        write_zip(&archive, &[("README.txt", b"notes")]);
        let sha = installer::sha256_file(&archive).expect("hash");
        let error = prepare_from(&dir, &staged_record("9.9.9", name, &sha)).expect_err("no binary");
        assert!(
            error.message().contains("Saarthi Studio binary"),
            "{}",
            error.message()
        );
        // A tree that cannot start is not left behind as a "prepared update".
        assert!(!prepared_dir(&dir).join("9.9.9").exists());
        assert_eq!(self::prepared(&dir)["present"], false);
    }

    #[test]
    fn a_version_used_as_a_folder_name_cannot_escape() {
        assert_eq!(version_dir_name("v2.38.0").as_deref(), Some("2.38.0"));
        assert_eq!(
            version_dir_name("2.38.0-rc.1").as_deref(),
            Some("2.38.0-rc.1")
        );
        assert!(version_dir_name("../../etc").is_none());
        assert!(version_dir_name("2.38/0").is_none());
        assert!(version_dir_name("").is_none());
        assert!(version_dir_name(&"9".repeat(41)).is_none());
    }

    #[test]
    fn a_gzipped_tarball_prepares_the_same_way() {
        let dir = scratch("prepare-gz");
        let staging = staged_dir(&dir);
        fs::create_dir_all(&staging).expect("staging");
        let name = "free-ai-suite_9.9.9_linux-x86_64.tar.gz";
        let archive = staging.join(name);
        {
            let encoder = flate2::write::GzEncoder::new(
                std::fs::File::create(&archive).expect("gz"),
                flate2::Compression::fast(),
            );
            let mut builder = tar::Builder::new(encoder);
            let body = b"binary";
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "free-ai-studio", body.as_slice())
                .expect("append");
            builder.finish().expect("tar finish");
            builder
                .into_inner()
                .expect("inner")
                .finish()
                .expect("gz finish");
        }
        let sha = installer::sha256_file(&archive).expect("hash");
        let prepared = prepare_from(&dir, &staged_record("9.9.9", name, &sha)).expect("prepare");
        assert_eq!(prepared["binary"], "free-ai-studio");
        assert_eq!(prepared["files"], 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
