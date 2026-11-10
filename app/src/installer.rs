//! Verified downloads for native installs.
//!
//! A router installed from a release archive is only as trustworthy as the
//! checksum beside it. This module fetches the vendor's asset while hashing it,
//! refuses the file when the published sha256 differs, and only then extracts.
//! A mismatch leaves nothing behind but the `.part` file it removes itself.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::{
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::Path as FsPath,
    time::Duration,
};

/// Largest archive this will fetch. Ollama's biggest asset is ~1.6 GiB, so the
/// cap only stops a wrong URL or a hostile redirect from filling the disk.
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// How an asset is packed, which decides both the extractor and what the plan
/// can promise about the result.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Archive {
    /// A single file dropped in place, nothing to unpack.
    None,
    Zip,
    /// The portable Linux and macOS artifacts are gzipped tarballs.
    TarGz,
    TarZst,
}

impl Archive {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Archive::None => "none",
            Archive::Zip => "zip",
            Archive::TarGz => "tar-gz",
            Archive::TarZst => "tar-zst",
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The vendor's `sha256sum.txt`, as published beside each release: one line per
/// asset, `hash  ./name`. Parsing is strict enough that a stray line cannot
/// silently become the hash of an asset that is being installed.
pub(crate) fn parse_sha256sums(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let hash = hash.trim().to_ascii_lowercase();
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let name = name
            .trim()
            .trim_start_matches('*')
            .trim_start_matches("./")
            .trim_end();
        // Asset names are flat: a line naming a path is not an asset this app
        // downloads, so it must not be able to match one.
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            continue;
        }
        out.insert(name.to_ascii_lowercase(), hash);
    }
    out
}

/// sha256 of a file on disk, for re-checking an archive that is already there.
pub(crate) fn sha256_file(path: &FsPath) -> Result<String, String> {
    let file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Download `url` to `dest`, but only keep it when the bytes hash to `expected`.
///
/// The file is written to `<dest>.part` and renamed on success, so an interrupted
/// or mismatched download can never be mistaken for a finished archive.
pub(crate) async fn download_verified(
    url: &str,
    expected: &str,
    dest: &FsPath,
) -> Result<u64, String> {
    use futures_util::StreamExt;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|error| format!("Could not build an HTTP client: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Could not download {url}: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{url} answered {}.", status.as_u16()));
    }
    if let Some(length) = response.content_length() {
        if length > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "{url} reports {length} bytes, which is larger than this app will download."
            ));
        }
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    let part = dest.with_file_name(format!(
        "{}.part",
        dest.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "download".to_string())
    ));
    let mut file = File::create(&part)
        .map_err(|error| format!("Could not write {}: {error}", part.display()))?;

    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = fs::remove_file(&part);
                return Err(format!("The download from {url} stopped: {error}"));
            }
        };
        total += chunk.len() as u64;
        if total > MAX_ARCHIVE_BYTES {
            let _ = fs::remove_file(&part);
            return Err(format!(
                "The download from {url} grew past {MAX_ARCHIVE_BYTES} bytes and was stopped."
            ));
        }
        hasher.update(&chunk);
        if let Err(error) = file.write_all(&chunk) {
            let _ = fs::remove_file(&part);
            return Err(format!("Could not write {}: {error}", part.display()));
        }
    }
    drop(file);

    let actual = hex(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "The download did not match the published sha256: expected {expected}, got {actual}. It was discarded and nothing was installed."
        ));
    }

    fs::rename(&part, dest).map_err(|error| {
        format!(
            "The download verified but could not be moved into {}: {error}",
            dest.display()
        )
    })?;
    Ok(total)
}

/// Unpack a verified archive into `dest`, refusing entries that would escape it.
/// Returns how many files were written.
pub(crate) fn extract(archive: Archive, source: &FsPath, dest: &FsPath) -> Result<usize, String> {
    match archive {
        Archive::None => Ok(0),
        Archive::Zip => extract_zip(source, dest),
        Archive::TarGz => extract_tar_gz(source, dest),
        Archive::TarZst => extract_tar_zst(source, dest),
    }
}

fn extract_zip(source: &FsPath, dest: &FsPath) -> Result<usize, String> {
    let file = File::open(source)
        .map_err(|error| format!("Could not open {}: {error}", source.display()))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|error| format!("{} is not a readable zip: {error}", source.display()))?;
    fs::create_dir_all(dest)
        .map_err(|error| format!("Could not create {}: {error}", dest.display()))?;

    let mut written = 0usize;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("Could not read entry {index}: {error}"))?;
        // `enclosed_name` is None for absolute paths and `..` traversal.
        let Some(relative) = entry.enclosed_name() else {
            return Err(format!(
                "{} contains an entry that would unpack outside the install directory.",
                source.display()
            ));
        };
        let target = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("Could not create {}: {error}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        }
        let mut out = File::create(&target)
            .map_err(|error| format!("Could not write {}: {error}", target.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|error| format!("Could not unpack {}: {error}", target.display()))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }
        written += 1;
    }
    Ok(written)
}

fn extract_tar_zst(source: &FsPath, dest: &FsPath) -> Result<usize, String> {
    let file = File::open(source)
        .map_err(|error| format!("Could not open {}: {error}", source.display()))?;
    let decoder = zstd::Decoder::new(BufReader::new(file)).map_err(|error| {
        format!(
            "{} is not a readable zstd stream: {error}",
            source.display()
        )
    })?;
    untar(tar::Archive::new(decoder), source, dest)
}

fn extract_tar_gz(source: &FsPath, dest: &FsPath) -> Result<usize, String> {
    let file = File::open(source)
        .map_err(|error| format!("Could not open {}: {error}", source.display()))?;
    // A corrupt gzip frame surfaces as a read error inside `untar`, which names
    // the archive it came from.
    let decoder = flate2::read::GzDecoder::new(BufReader::new(file));
    untar(tar::Archive::new(decoder), source, dest)
}

/// The tar half both decoders share: `unpack_in` refuses absolute paths and `..`,
/// and preserves modes, so an entry cannot escape `dest`.
fn untar<T: Read>(
    mut tar: tar::Archive<T>,
    source: &FsPath,
    dest: &FsPath,
) -> Result<usize, String> {
    fs::create_dir_all(dest)
        .map_err(|error| format!("Could not create {}: {error}", dest.display()))?;

    let entries = tar
        .entries()
        .map_err(|error| format!("Could not read {}: {error}", source.display()))?;
    let mut written = 0usize;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("Could not read an entry: {error}"))?;
        // `unpack_in` refuses absolute paths and `..`, and preserves modes.
        let unpacked = entry.unpack_in(dest).map_err(|error| {
            format!(
                "Could not unpack an entry of {} into {}: {error}",
                source.display(),
                dest.display()
            )
        })?;
        if !unpacked {
            return Err(format!(
                "{} contains an entry that would unpack outside the install directory.",
                source.display()
            ));
        }
        if entry.header().entry_type().is_file() {
            written += 1;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_file_parsing_ignores_noise() {
        let text = "\
# comment
5b1b0a7a6370755a3472b1fb2a63e569a5b0114acef2a9067047bf8e5d98ecc1  ./ollama-linux-amd64.tar.zst
25F64B810B947145095956533E1BDF56EACEA2673C55A7E586BE4515FC882C9F *ollama-windows-amd64.zip
not-a-hash  something.bin
deadbeef  ./incomplete.tar
./nested/path.tar.zst  0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
";
        let sums = parse_sha256sums(text);
        assert_eq!(
            sums.get("ollama-linux-amd64.tar.zst").map(String::as_str),
            Some("5b1b0a7a6370755a3472b1fb2a63e569a5b0114acef2a9067047bf8e5d98ecc1")
        );
        // A `*` marker and an upper-case hash are both normalised.
        assert_eq!(
            sums.get("ollama-windows-amd64.zip").map(String::as_str),
            Some("25f64b810b947145095956533e1bdf56eacea2673c55a7e586be4515fc882c9f")
        );
        // Short hashes and paths are not assets.
        assert_eq!(sums.len(), 2);
    }

    #[test]
    fn a_zero_byte_file_hashes_to_the_empty_digest() {
        let path = std::env::temp_dir().join(format!("studio-empty-{}.bin", std::process::id()));
        fs::write(&path, b"").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn zip_extraction_writes_files_and_refuses_traversal() {
        let root = std::env::temp_dir().join(format!("studio-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let good = root.join("good.zip");
        {
            let mut writer = zip::ZipWriter::new(File::create(&good).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("inner/app.txt", options).unwrap();
            writer.write_all(b"hello").unwrap();
            writer.finish().unwrap();
        }
        let dest = root.join("out");
        // A checksum of the archive is what a caller verifies; extraction trusts
        // nothing about the contents beyond staying inside `dest`.
        assert_eq!(extract(Archive::Zip, &good, &dest).unwrap(), 1);
        assert_eq!(
            fs::read_to_string(dest.join("inner/app.txt")).unwrap(),
            "hello"
        );

        // A zip that tries to escape is refused, not written outside.
        let bad = root.join("bad.zip");
        {
            let mut writer = zip::ZipWriter::new(File::create(&bad).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("../escaped.txt", options).unwrap();
            writer.write_all(b"nope").unwrap();
            writer.finish().unwrap();
        }
        let escaped = root.join("escaped.txt");
        let outcome = extract(Archive::Zip, &bad, &root.join("out2"));
        assert!(outcome.is_err(), "traversal was accepted: {outcome:?}");
        assert!(!escaped.exists(), "an entry escaped the install directory");

        let _ = fs::remove_dir_all(&root);
    }

    async fn serve_once(body: Vec<u8>) -> String {
        use axum::{routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/file.bin",
            get(move || {
                let body = body.clone();
                async move { body }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{address}/file.bin")
    }

    #[tokio::test]
    async fn a_matching_download_is_kept_and_a_mismatch_is_discarded() {
        let body = b"hello verified world".to_vec();
        let mut hasher = Sha256::new();
        hasher.update(&body);
        let expected = hex(&hasher.finalize());
        let url = serve_once(body.clone()).await;

        let root = std::env::temp_dir().join(format!("studio-dl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let good = root.join("asset.bin");
        assert_eq!(
            download_verified(&url, &expected, &good).await.unwrap(),
            body.len() as u64
        );
        assert_eq!(fs::read(&good).unwrap(), body);

        // A wrong hash must leave nothing that could be mistaken for an archive.
        let bad = root.join("bad.bin");
        let error = download_verified(&url, &"0".repeat(64), &bad)
            .await
            .expect_err("a mismatched download was accepted");
        assert!(error.contains("did not match"), "{error}");
        assert!(!bad.exists());
        assert!(!root.join("bad.bin.part").exists());

        let _ = fs::remove_dir_all(&root);
    }

    /// Not part of the default run: this reaches GitHub and fetches a real release
    /// asset, so it is run deliberately with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "network: fetches a real release asset"]
    async fn a_real_release_asset_verifies_against_its_published_hash() {
        let tag = "v0.40.14";
        let base = format!("https://github.com/ollama/ollama/releases/download/{tag}");
        let sums = reqwest::get(format!("{base}/sha256sum.txt"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let parsed = parse_sha256sums(&sums);
        // install.sh is small enough to check every byte in a unit test.
        let expected = parsed
            .get("install.sh")
            .expect("the release publishes a hash for install.sh")
            .clone();

        let root = std::env::temp_dir().join(format!("studio-real-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dest = root.join("install.sh");
        let bytes = download_verified(&format!("{base}/install.sh"), &expected, &dest)
            .await
            .unwrap();
        assert!(bytes > 0);
        assert_eq!(sha256_file(&dest).unwrap(), expected);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_labels_are_distinct() {
        assert_ne!(Archive::Zip.label(), Archive::TarZst.label());
        assert_ne!(Archive::None.label(), Archive::Zip.label());
    }
}
