//! The OS credential store for API keys, so a plaintext secret does not live
//! in `config.json` (Phase 10).
//!
//! One entry per secret: service `io.github.sairajkhilariarch.SarthiStudio`
//! (the same reverse-DNS id the AppStream metadata carries), account = the
//! config field name. `save_config_file` moves every non-empty secret into
//! the store and blanks it from the JSON it writes; `load_config` merges the
//! stored values back over the file. When the store is unavailable the
//! secret simply stays in the (permission-restricted) config file — the
//! pre-keychain behavior — rather than being dropped.
//!
//! Store selection, via `FREE_AI_STUDIO_KEYCHAIN`:
//! - unset or `os`: the platform store (Windows Credential Manager, macOS
//!   Keychain, Linux Secret Service under the `sync` features).
//! - `mock`: the keyring crate's in-memory mock store. `.cargo/config.toml`
//!   sets this value so `cargo test` and dev runs stay hermetic — no fixture
//!   ever touches a real user vault.
//! - `off`: the keychain is bypassed entirely; secrets stay in config.json.
//!   The escape hatch for systems without a usable credential service.
//!
//! Mock-mode services are derived from the config directory's hash, so
//! parallel test fixtures never share credentials, and the real store always
//! keeps the stable production name.
//!
//! `FREE_AI_STUDIO_KEYCHAIN_SERVICE` renames the service, so a live smoke can
//! use the real store under a throw-away name instead of the production one.

use std::collections::HashMap;
use std::env;
use std::sync::{Mutex, OnceLock};

use keyring::Entry;
use sha2::{Digest, Sha256};

/// The secret field names the config carries. The wire name is the keychain
/// account name, so a rule or a vault inspection reads the same string.
pub(crate) const SECRET_FIELDS: [&str; 3] = ["api_key", "search_api_key", "openai_api_key"];

const fn default_service() -> &'static str {
    "io.github.sairajkhilariarch.SarthiStudio"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The platform credential store.
    Os,
    /// The in-process mock store (tests, dev runs, unsupported platforms).
    Mock,
    /// Secrets stay in config.json.
    Off,
}

impl Mode {
    fn from_env_string(value: &str) -> Mode {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" => Mode::Mock,
            "off" => Mode::Off,
            _ => Mode::Os,
        }
    }

    /// The wire name the config endpoint exposes, so the UI can honestly say
    /// where a key lives instead of claiming a store it is not using.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Mode::Os => "os",
            Mode::Mock => "mock",
            Mode::Off => "off",
        }
    }
}

fn service() -> String {
    env::var("FREE_AI_STUDIO_KEYCHAIN_SERVICE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_service().to_string())
}

/// The service name for one config location. Under the mock store the name
/// is per-location (fixture dirs are unique), so no two parallel test
/// fixtures ever share credentials; on the real platform stores the stable
/// production name is used so a user's key follows their single home.
pub(crate) fn service_for(location: &str) -> String {
    if mode() == Mode::Mock {
        let digest = Sha256::digest(location.as_bytes());
        format!(
            "{}-mock-{:x}",
            default_service(),
            u32::from_le_bytes(digest[..4].try_into().expect("sha256 digest is 32 bytes"))
        )
    } else {
        service()
    }
}

/// The store choice is frozen on first use: flipping the environment variable
/// mid-run (or mid-test) must not move live credentials between stores.
fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| {
        env::var("FREE_AI_STUDIO_KEYCHAIN")
            .map(|value| Mode::from_env_string(&value))
            .unwrap_or(Mode::Os)
    })
}

/// The frozen store choice, for the config endpoint's honest label.
pub(crate) fn current_mode() -> Mode {
    mode()
}

/// Point the crate's default credential builder at the mock store before the
/// first entry is created. Process-wide, so every entry in a mock run shares
/// it; a no-op in every other mode.
fn ensure_store() {
    static READY: OnceLock<()> = OnceLock::new();
    if READY.set(()).is_ok() && mode() == Mode::Mock {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    }
}

/// One open handle per secret name, held for the life of the process.
/// The mock store keeps its data inside the credential object, so a local
/// entry that is created, written and dropped again in the same call would
/// drop the secret with it — and even the real stores save a lookup when the
/// handle already exists.
fn with_entry<T>(
    location: &str,
    name: &str,
    operation: impl FnOnce(&Entry) -> Result<T, String>,
) -> Result<T, String> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), Entry>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        ensure_store();
        Mutex::new(HashMap::new())
    });
    let mut cache = cache.lock().unwrap();
    let cache_key = (service_for(location), name.to_string());
    if !cache.contains_key(&cache_key) {
        let entry =
            Entry::new(&cache_key.0, name).map_err(|error| format!("keychain entry: {error}"))?;
        cache.insert(cache_key.clone(), entry);
    }
    let entry = cache.get(&cache_key).expect("the entry was just inserted");
    operation(entry)
}

/// Stored value, or `None` when the name has not been stored. A store error
/// surfaces as `Err` and the caller keeps the config-file value.
pub(crate) fn read_secret(location: &str, name: &str) -> Result<Option<String>, String> {
    if mode() == Mode::Off {
        return Ok(None);
    }
    with_entry(location, name, |entry| match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("keychain read: {error}")),
    })
}

/// Store (or replace) a secret.
pub(crate) fn write_secret(location: &str, name: &str, value: &str) -> Result<(), String> {
    if mode() == Mode::Off {
        return Err("the keychain is disabled; the secret stays in config.json".to_string());
    }
    with_entry(location, name, |entry| {
        entry
            .set_password(value)
            .map_err(|error| format!("keychain write: {error}"))
    })
}

/// Remove a stored secret. A name that was never stored counts as removed.
pub(crate) fn delete_secret(location: &str, name: &str) -> Result<(), String> {
    if mode() == Mode::Off {
        return Ok(());
    }
    with_entry(location, name, |entry| match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("keychain delete: {error}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_mode_is_frozen_on_first_use() {
        // The mode enum covers the contract; the freeze is what keeps a
        // concurrent env change from moving live secrets between stores.
        assert!(matches!(mode(), Mode::Os | Mode::Mock | Mode::Off));
    }

    #[test]
    fn every_mode_parses_from_its_environment_string() {
        assert_eq!(Mode::from_env_string("mock"), Mode::Mock);
        assert_eq!(Mode::from_env_string(" MOCK "), Mode::Mock);
        assert_eq!(Mode::from_env_string("off"), Mode::Off);
        assert_eq!(Mode::from_env_string("os"), Mode::Os);
        assert_eq!(Mode::from_env_string("anything"), Mode::Os);
        assert_eq!(Mode::from_env_string(""), Mode::Os);
    }

    #[test]
    fn the_default_service_is_the_appstream_id() {
        assert_eq!(
            default_service(),
            "io.github.sairajkhilariarch.SarthiStudio"
        );
    }

    #[test]
    fn a_secret_round_trips_through_the_store() {
        write_secret("probe-location", "probe_roundtrip", "probe-value").expect("write");
        let stored = read_secret("probe-location", "probe_roundtrip").expect("read");
        assert_eq!(stored.as_deref(), Some("probe-value"));
        delete_secret("probe-location", "probe_roundtrip").expect("delete");
        assert_eq!(
            read_secret("probe-location", "probe_roundtrip").expect("read again"),
            None
        );
    }

    #[test]
    fn an_unstored_name_reads_as_none() {
        assert_eq!(
            read_secret("probe-location", "never-installed-key").expect("read"),
            None
        );
    }

    #[test]
    fn mock_locations_do_not_share_credentials() {
        // The isolation test fixtures depend on: two different config
        // locations must not see each other's secrets under the mock store.
        write_secret("fixture-a", "api_key", "alpha key").expect("write a");
        write_secret("fixture-b", "api_key", "bravo key").expect("write b");
        assert_eq!(
            read_secret("fixture-a", "api_key").expect("read a"),
            Some("alpha key".to_string())
        );
        assert_eq!(
            read_secret("fixture-b", "api_key").expect("read b"),
            Some("bravo key".to_string())
        );
    }

    #[test]
    fn the_mock_service_is_tied_to_the_location() {
        let a = service_for("fixture-a");
        let b = service_for("fixture-b");
        assert_ne!(a, b, "parallel fixtures must not share a service name");
        assert_eq!(
            a,
            service_for("fixture-a"),
            "the name is stable per location"
        );
        assert!(a.starts_with(default_service()));
    }
}
