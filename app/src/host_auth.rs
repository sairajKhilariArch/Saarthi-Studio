//! Local lifecycle for the single active desktop-host bearer token.
//!
//! This module deliberately does not open a network listener. The future remote
//! router should call `verify()` from authentication middleware and should use
//! only the allow-listed host API routes.

use getrandom::getrandom;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

const SCHEMA_VERSION: u32 = 1;
const TOKEN_PREFIX: &str = "saarthi_host_v1_";
const TOKEN_BYTES: usize = 32;
const MAX_VERIFIER_FILE_BYTES: u64 = 4096;

#[derive(Clone, Debug)]
pub struct HostTokenStore {
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenStatus {
    pub created_at_unix_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredVerifier {
    schema_version: u32,
    token_sha256: String,
    created_at_unix_secs: u64,
}

impl HostTokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Create a token only if there is no active verifier already stored.
    /// The raw token is returned exactly once and is never written to disk.
    pub fn create(&self) -> io::Result<String> {
        if self.read_record()?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a host token is already configured; use `host-token rotate` to replace it",
            ));
        }
        self.rotate()
    }

    /// Generate and persist a new token. The previous token is invalidated as
    /// soon as the atomic replacement completes.
    pub fn rotate(&self) -> io::Result<String> {
        let mut entropy = [0_u8; TOKEN_BYTES];
        getrandom(&mut entropy).map_err(|error| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("operating-system random generator failed: {error}"),
            )
        })?;

        let token = format!("{TOKEN_PREFIX}{}", encode_hex(&entropy));
        let digest = Sha256::digest(token.as_bytes());
        let record = StoredVerifier {
            schema_version: SCHEMA_VERSION,
            token_sha256: encode_hex(digest.as_slice()),
            created_at_unix_secs: now_unix_secs()?,
        };
        self.write_record(&record)?;
        Ok(token)
    }

    /// Revoke the active token. This is idempotent and also removes a corrupt
    /// verifier file so an administrator can recover from invalid state.
    pub fn revoke(&self) -> io::Result<bool> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                if metadata.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "host-token path points to a directory, not a verifier file",
                    ));
                }
                validate_parent_directory(parent_directory(&self.path))?;
                fs::remove_file(&self.path)?;
                sync_parent_directory(&self.path)?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Verify a presented bearer token against the stored SHA-256 verifier.
    /// The record is re-read on every check, so rotation/revocation takes
    /// effect without restarting the desktop process.
    pub fn verify(&self, candidate: &str) -> io::Result<bool> {
        if !is_well_formed_token(candidate) {
            return Ok(false);
        }
        let Some(record) = self.read_record()? else {
            return Ok(false);
        };
        let expected = decode_hash(&record.token_sha256).map_err(invalid_data)?;
        let actual = Sha256::digest(candidate.as_bytes());
        Ok(bool::from(actual.as_slice().ct_eq(expected.as_slice())))
    }

    pub fn status(&self) -> io::Result<Option<TokenStatus>> {
        Ok(self.read_record()?.map(|record| TokenStatus {
            created_at_unix_secs: record.created_at_unix_secs,
        }))
    }

    fn read_record(&self) -> io::Result<Option<StoredVerifier>> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };

        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid_data(
                "host-token verifier must be a regular, non-symlink file",
            ));
        }
        if metadata.len() > MAX_VERIFIER_FILE_BYTES {
            return Err(invalid_data(
                "host-token verifier file is unexpectedly large",
            ));
        }
        enforce_existing_file_permissions(&metadata)?;
        let parent_metadata = validate_parent_directory(parent_directory(&self.path))?;
        enforce_existing_directory_permissions(&parent_metadata)?;

        let bytes = fs::read(&self.path)?;
        let record: StoredVerifier = serde_json::from_slice(&bytes).map_err(invalid_data)?;
        if record.schema_version != SCHEMA_VERSION {
            return Err(invalid_data(
                "unsupported host-token verifier schema version",
            ));
        }
        decode_hash(&record.token_sha256).map_err(invalid_data)?;
        Ok(Some(record))
    }

    fn write_record(&self, record: &StoredVerifier) -> io::Result<()> {
        let serialized = serde_json::to_vec_pretty(record).map_err(invalid_data)?;
        let parent = parent_directory(&self.path);
        ensure_private_directory(parent)?;

        let file_name = self.path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "host-token path must name a file",
            )
        })?;
        let file_name = file_name.to_string_lossy();

        // Create the temporary file in the same private directory so the final
        // rename is atomic on supported local filesystems.
        for _ in 0..8 {
            let mut suffix = [0_u8; 8];
            getrandom(&mut suffix).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("operating-system random generator failed: {error}"),
                )
            })?;
            let temporary_path = parent.join(format!(".{file_name}.{}.tmp", encode_hex(&suffix)));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }

            let mut file = match options.open(&temporary_path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };

            let result = (|| -> io::Result<()> {
                file.write_all(&serialized)?;
                file.sync_all()?;
                drop(file);
                enforce_file_permissions(&temporary_path)?;
                replace_verifier_file(&temporary_path, &self.path)?;
                sync_parent_directory(&self.path)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary_path);
            }
            return result;
        }

        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique host-token temporary file",
        ))
    }
}

/// Handle the local `host-token` CLI. The CLI is intentionally separate from
/// HTTP routes, and does not start or change the desktop's network listener.
pub fn run_cli(args: &[String], store: &HostTokenStore) -> Result<(), Box<dyn std::error::Error>> {
    match args {
        [command] if command.as_str() == "create" => {
            let token = store.create()?;
            print_one_time_token(&token);
        }
        [command] if command.as_str() == "rotate" => {
            let token = store.rotate()?;
            print_one_time_token(&token);
            eprintln!("The previously active host token is now invalid.");
        }
        [command] if command.as_str() == "revoke" => {
            if store.revoke()? {
                println!("Desktop host token revoked.");
            } else {
                println!("No active desktop host token was configured.");
            }
        }
        [command] if command.as_str() == "status" => match store.status()? {
            Some(status) => {
                println!("Host token configured: yes");
                println!("Created at (Unix seconds): {}", status.created_at_unix_secs);
            }
            None => println!("Host token configured: no"),
        },
        _ => {
            println!(
                "Usage:\n  free-ai-studio host-token create\n  free-ai-studio host-token rotate\n  free-ai-studio host-token status\n  free-ai-studio host-token revoke"
            );
        }
    }
    Ok(())
}

fn print_one_time_token(token: &str) {
    println!("Desktop host token (shown once; keep it private):\n{token}");
    println!("The raw token is not saved to disk or included in application logs.");
    println!(
        "Store it in the Android app's encrypted host-token setting when that mode is implemented."
    );
}

fn is_well_formed_token(token: &str) -> bool {
    let Some(suffix) = token.strip_prefix(TOKEN_PREFIX) else {
        return false;
    };
    suffix.len() == TOKEN_BYTES * 2
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hash(value: &str) -> Result<[u8; 32], &'static str> {
    if value.len() != 64 {
        return Err(
            "stored host-token hash must contain exactly 64 lowercase hexadecimal characters",
        );
    }
    let mut decoded = [0_u8; 32];
    let bytes = value.as_bytes();
    for index in 0..32 {
        let high = hex_nibble(bytes[index * 2])
            .ok_or("stored host-token hash is not lowercase hexadecimal")?;
        let low = hex_nibble(bytes[index * 2 + 1])
            .ok_or("stored host-token hash is not lowercase hexadecimal")?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn now_unix_secs() -> io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error))
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn invalid_data(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let _ = validate_parent_directory(path)?;
    enforce_directory_permissions(path)
}

fn validate_parent_directory(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid_data(
            "host-token parent must be a regular directory, not a symlink",
        ));
    }
    Ok(metadata)
}

#[cfg(unix)]
fn enforce_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn enforce_directory_permissions(_path: &Path) -> io::Result<()> {
    // Windows ACL hardening must be provided by platform-specific code before
    // remote access is enabled there. This module's strict mode checks apply to Unix.
    Ok(())
}

#[cfg(unix)]
fn enforce_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn enforce_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn enforce_existing_file_permissions(metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "host-token verifier permissions are too broad; set the file mode to 600",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_existing_file_permissions(_metadata: &fs::Metadata) -> io::Result<()> {
    // See the platform-specific ACL note in `enforce_directory_permissions`.
    Ok(())
}

#[cfg(unix)]
fn enforce_existing_directory_permissions(metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "host-token directory permissions are too broad; set the directory mode to 700",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_existing_directory_permissions(_metadata: &fs::Metadata) -> io::Result<()> {
    // See the platform-specific ACL note in `enforce_directory_permissions`.
    Ok(())
}

#[cfg(windows)]
fn replace_verifier_file(temporary_path: &Path, destination: &Path) -> io::Result<()> {
    // std::fs::rename does not replace an existing destination on Windows.
    // Remote host mode stays disabled on Windows until ACL hardening is added;
    // failing closed between removal and rename is preferable to retaining an
    // old verifier after a requested rotation.
    match fs::remove_file(destination) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(temporary_path, destination)
}

#[cfg(not(windows))]
fn replace_verifier_file(temporary_path: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary_path, destination)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    File::open(parent_directory(path))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestStore {
        directory: PathBuf,
        store: HostTokenStore,
    }

    impl TestStore {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("saarthi-host-auth-{}", Uuid::new_v4()));
            fs::create_dir_all(&directory).expect("create test directory");
            enforce_directory_permissions(&directory).expect("secure test directory");
            let store = HostTokenStore::new(directory.join("host-auth.json"));
            Self { directory, store }
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn create_persists_only_a_verifier_and_token_verifies() {
        let fixture = TestStore::new();
        let token = fixture.store.create().expect("create token");
        assert!(token.starts_with(TOKEN_PREFIX));
        assert_eq!(token.len(), TOKEN_PREFIX.len() + TOKEN_BYTES * 2);
        assert!(fixture.store.verify(&token).expect("verify token"));

        let stored = fs::read_to_string(&fixture.store.path).expect("read verifier");
        assert!(!stored.contains(&token), "raw token must never be stored");
        assert!(stored.contains("token_sha256"));
    }

    #[cfg(unix)]
    #[test]
    fn created_verifier_file_and_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = TestStore::new();
        fixture.store.create().expect("create token");
        let dir_mode = fs::metadata(&fixture.directory)
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&fixture.store.path)
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn incorrect_and_malformed_tokens_are_rejected() {
        let fixture = TestStore::new();
        fixture.store.create().expect("create token");
        assert!(!fixture
            .store
            .verify("wrong")
            .expect("reject malformed token"));
        let almost_valid = format!("{TOKEN_PREFIX}{}", "a".repeat(TOKEN_BYTES * 2));
        assert!(!fixture
            .store
            .verify(&almost_valid)
            .expect("reject wrong token"));
    }

    #[test]
    fn create_refuses_to_overwrite_an_active_token() {
        let fixture = TestStore::new();
        let first = fixture.store.create().expect("create first token");
        assert_eq!(
            fixture.store.create().unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(fixture
            .store
            .verify(&first)
            .expect("first token remains active"));
    }

    #[test]
    fn rotation_invalidates_the_previous_token() {
        let fixture = TestStore::new();
        let first = fixture.store.create().expect("create first token");
        let second = fixture.store.rotate().expect("rotate token");
        assert_ne!(first, second);
        assert!(!fixture.store.verify(&first).expect("old token check"));
        assert!(fixture.store.verify(&second).expect("new token check"));
    }

    #[test]
    fn revoke_is_idempotent_and_invalidates_tokens() {
        let fixture = TestStore::new();
        let token = fixture.store.create().expect("create token");
        assert!(fixture.store.revoke().expect("revoke token"));
        assert!(!fixture.store.verify(&token).expect("revoked token check"));
        assert!(!fixture.store.revoke().expect("second revoke"));
    }

    #[test]
    fn status_exposes_metadata_but_not_the_secret() {
        let fixture = TestStore::new();
        assert!(fixture.store.status().expect("empty status").is_none());
        let token = fixture.store.create().expect("create token");
        let status = fixture.store.status().expect("configured status").unwrap();
        assert!(status.created_at_unix_secs > 0);
        assert!(!format!("{status:?}").contains(&token));
    }

    #[test]
    fn corrupt_verifier_fails_closed() {
        let fixture = TestStore::new();
        fs::create_dir_all(fixture.store.path.parent().unwrap()).expect("test directory exists");
        fs::write(&fixture.store.path, b"not-json").expect("write corruption");
        enforce_file_permissions(&fixture.store.path).expect("secure corrupt file");
        assert_eq!(
            fixture
                .store
                .verify(&format!("{TOKEN_PREFIX}{}", "a".repeat(64)))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[test]
    fn permissive_parent_directory_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = TestStore::new();
        fixture.store.create().expect("create token");
        fs::set_permissions(&fixture.directory, fs::Permissions::from_mode(0o755))
            .expect("make directory permissive");
        assert_eq!(
            fixture.store.status().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_verifier_path_is_rejected() {
        use std::os::unix::fs::symlink;
        let fixture = TestStore::new();
        let outside_target = fixture.directory.join("elsewhere.json");
        fs::write(&outside_target, b"{}\n").expect("write target file");
        enforce_file_permissions(&outside_target).expect("secure target file");
        symlink(&outside_target, &fixture.store.path).expect("create verifier symlink");
        let candidate = format!("{TOKEN_PREFIX}{}", "a".repeat(TOKEN_BYTES * 2));
        assert_eq!(
            fixture.store.verify(&candidate).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[test]
    fn permissive_verifier_file_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = TestStore::new();
        fixture.store.create().expect("create token");
        fs::set_permissions(&fixture.store.path, fs::Permissions::from_mode(0o644))
            .expect("make permissions too broad");
        assert_eq!(
            fixture.store.status().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
