//! Opt-in HTTPS listener configuration for the restricted desktop host API.
//!
//! This module deliberately fails closed: the listener is disabled unless explicitly
//! enabled, the bind address must be a specific local/private address, and TLS files
//! must be supplied. There is no plaintext HTTP fallback.

use axum_server::tls_rustls::RustlsConfig;
use std::{
    env,
    error::Error,
    fs, io,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

const DEFAULT_PORT: u16 = 8766;

#[derive(Clone, Debug)]
pub(super) struct RemoteListenerConfig {
    bind_ip: IpAddr,
    port: u16,
    certificate_path: PathBuf,
    private_key_path: PathBuf,
}

impl RemoteListenerConfig {
    /// Read opt-in listener settings. Missing ENABLED means disabled.
    pub(super) fn from_env() -> Result<Option<Self>, Box<dyn Error>> {
        let enabled = match env::var("FREE_AI_STUDIO_HOST_API_ENABLED") {
            Ok(value) => parse_enabled(&value)?,
            Err(env::VarError::NotPresent) => false,
            Err(error) => return Err(Box::new(error)),
        };
        if !enabled {
            return Ok(None);
        }

        let bind_text = env::var("FREE_AI_STUDIO_HOST_API_BIND").map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Host API is enabled, but FREE_AI_STUDIO_HOST_API_BIND is not set.",
            )
        })?;
        let bind_ip = parse_bind_ip(&bind_text)?;

        let port = match env::var("FREE_AI_STUDIO_HOST_API_PORT") {
            Ok(value) => {
                let value: u16 = value.parse().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Host API port must be an integer from 1 to 65535.",
                    )
                })?;
                if value == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Host API port 0 is not allowed.",
                    )
                    .into());
                }
                value
            }
            Err(env::VarError::NotPresent) => DEFAULT_PORT,
            Err(error) => return Err(Box::new(error)),
        };

        let certificate_path = required_path("FREE_AI_STUDIO_HOST_API_TLS_CERT")?;
        let private_key_path = required_path("FREE_AI_STUDIO_HOST_API_TLS_KEY")?;
        validate_regular_file(&certificate_path, false)?;
        validate_regular_file(&private_key_path, true)?;

        Ok(Some(Self {
            bind_ip,
            port,
            certificate_path,
            private_key_path,
        }))
    }

    pub(super) fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.port)
    }

    pub(super) async fn load_tls_config(&self) -> Result<RustlsConfig, Box<dyn Error>> {
        // Rustls fails startup on malformed PEM, missing certificates, or a key that
        // does not match the certificate. Client-side certificate verification is
        // still required; the server never asks Android to bypass it.
        Ok(RustlsConfig::from_pem_file(&self.certificate_path, &self.private_key_path).await?)
    }
}

fn required_path(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let raw = env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Host API is enabled, but {name} is not set."),
        )
    })?;
    if raw.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must not be empty."),
        )
        .into());
    }
    Ok(PathBuf::from(raw))
}

fn parse_enabled(raw: &str) -> Result<bool, io::Error> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "FREE_AI_STUDIO_HOST_API_ENABLED must be exactly true or false.",
        )),
    }
}

fn parse_bind_ip(raw: &str) -> Result<IpAddr, io::Error> {
    let ip: IpAddr = raw.trim().parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Host API bind must be a literal IP address, not a hostname.",
        )
    })?;
    if !is_approved_bind_ip(ip) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Host API may bind only to loopback, private, link-local, or shared VPN-range IPs. Unspecified/public addresses are refused.",
        ));
    }
    Ok(ip)
}

fn is_approved_bind_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            let octets = value.octets();
            value.is_loopback()
                || value.is_private()
                || (octets[0] == 169 && octets[1] == 254) // link-local 169.254.0.0/16
                || (octets[0] == 100 && (64..=127).contains(&octets[1])) // shared 100.64.0.0/10, often used by VPNs
        }
        IpAddr::V6(value) => {
            let first = value.segments()[0];
            value.is_loopback()
                || first & 0xfe00 == 0xfc00 // unique-local fc00::/7
                || first & 0xffc0 == 0xfe80 // link-local fe80::/10
        }
    }
}

fn validate_regular_file(path: &Path, private_key: bool) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "TLS {} must be a non-empty regular file, not a symlink: {}",
                if private_key {
                    "private key"
                } else {
                    "certificate"
                },
                path.display()
            ),
        )
        .into());
    }

    if private_key {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("TLS private key permissions are too broad (mode {mode:o}); run chmod 600 {}.", path.display()),
                ).into());
            }
        }
        #[cfg(not(unix))]
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Remote host API is disabled on this platform until private-key ACL validation is implemented.",
            ).into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_approved_bind_ip, parse_bind_ip, parse_enabled};
    use std::net::IpAddr;
    use std::str::FromStr;

    #[test]
    fn listener_is_opt_in_and_flag_is_strict() {
        assert!(!parse_enabled("false").unwrap());
        assert!(!parse_enabled(" FALSE ").unwrap());
        assert!(parse_enabled("true").unwrap());
        assert!(parse_enabled("yes").is_err());
        assert!(parse_enabled("1").is_err());
    }

    #[test]
    fn bind_policy_rejects_unspecified_and_public_addresses() {
        for value in ["0.0.0.0", "::", "8.8.8.8", "2001:4860:4860::8888"] {
            assert!(parse_bind_ip(value).is_err(), "should reject {value}");
        }
    }

    #[test]
    fn bind_policy_accepts_loopback_private_link_local_and_shared_vpn_range() {
        for value in [
            "127.0.0.1",
            "192.168.1.8",
            "10.8.0.4",
            "169.254.10.2",
            "100.100.2.3",
            "::1",
            "fd12:3456::2",
            "fe80::1234",
        ] {
            assert!(parse_bind_ip(value).is_ok(), "should accept {value}");
        }
    }

    #[test]
    fn hostnames_are_not_accepted_as_bind_addresses() {
        assert!(parse_bind_ip("my-desktop.local").is_err());
    }

    #[test]
    fn address_policy_never_accepts_multicast_or_unspecified() {
        for value in ["224.0.0.1", "ff02::1"] {
            let ip = IpAddr::from_str(value).unwrap();
            assert!(!is_approved_bind_ip(ip));
        }
    }
}
