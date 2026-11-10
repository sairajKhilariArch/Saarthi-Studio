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

use crate::{json_error, now_seconds, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
