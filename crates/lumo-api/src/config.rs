use std::{env, fmt, net::SocketAddr, path::PathBuf};

use lumo_core::{LumoError, LumoResult};
use lumo_protocol::MIN_API_SECRET_BYTES;
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiLimits {
    pub max_groups: u32,
    pub max_devices_per_group: u32,
    pub max_active_invites_per_group: u32,
    pub bootstrap_per_ip: u32,
    pub bootstrap_global: u32,
    pub bootstrap_window_ms: i64,
    pub invite_ttl_ms: i64,
}

impl Default for ApiLimits {
    fn default() -> Self {
        Self {
            max_groups: 1_000,
            max_devices_per_group: 8,
            max_active_invites_per_group: 8,
            bootstrap_per_ip: 5,
            bootstrap_global: 100,
            bootstrap_window_ms: 60 * 60 * 1_000,
            invite_ttl_ms: 10 * 60 * 1_000,
        }
    }
}

#[derive(Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub database_path: PathBuf,
    pub tls_cert_path: PathBuf,
    pub tls_key_path: PathBuf,
    pub master_key: Zeroizing<String>,
    pub enable_legacy_v1: bool,
    pub legacy_password: Option<Zeroizing<String>>,
    pub trust_proxy_headers: bool,
    pub limits: ApiLimits,
}

impl fmt::Debug for ApiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiConfig")
            .field("bind", &self.bind)
            .field("database_path", &self.database_path)
            .field("tls_cert_path", &self.tls_cert_path)
            .field("tls_key_path", &self.tls_key_path)
            .field("master_key", &"[REDACTED]")
            .field("enable_legacy_v1", &self.enable_legacy_v1)
            .field(
                "legacy_password",
                &self.legacy_password.as_ref().map(|_| "[REDACTED]"),
            )
            .field("trust_proxy_headers", &self.trust_proxy_headers)
            .field("limits", &self.limits)
            .finish()
    }
}

impl ApiConfig {
    pub fn from_env() -> LumoResult<Self> {
        let _ = dotenvy::dotenv();
        let bind = env::var("LUMO_API_BIND")
            .unwrap_or_else(|_| "0.0.0.0:8443".to_owned())
            .parse()
            .map_err(|error| LumoError::Configuration(format!("invalid LUMO_API_BIND: {error}")))?;
        let master_key = env::var("LUMO_SERVER_MASTER_KEY").map_err(|_| {
            LumoError::Configuration("LUMO_SERVER_MASTER_KEY is required".to_owned())
        })?;
        if master_key.len() < MIN_API_SECRET_BYTES {
            return Err(LumoError::Configuration(format!(
                "LUMO_SERVER_MASTER_KEY must contain at least {MIN_API_SECRET_BYTES} bytes"
            )));
        }
        let enable_legacy_v1 = env_bool("LUMO_ENABLE_LEGACY_V1", false)?;
        let legacy_password = env::var("LUMO_LEGACY_API_PASSWORD")
            .ok()
            .filter(|value| !value.is_empty())
            .map(Zeroizing::new);
        if enable_legacy_v1
            && legacy_password.as_deref().map_or(0, |value| value.len()) < MIN_API_SECRET_BYTES
        {
            return Err(LumoError::Configuration(format!(
                "LUMO_LEGACY_API_PASSWORD must contain at least {MIN_API_SECRET_BYTES} bytes when v1 is enabled"
            )));
        }
        Ok(Self {
            bind,
            database_path: env::var_os("LUMO_API_DATABASE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/data/lumo-api.sqlite3")),
            tls_cert_path: env::var_os("LUMO_TLS_CERT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/certs/fullchain.pem")),
            tls_key_path: env::var_os("LUMO_TLS_KEY_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/certs/privkey.pem")),
            master_key: Zeroizing::new(master_key),
            enable_legacy_v1,
            legacy_password,
            trust_proxy_headers: env_bool("LUMO_TRUST_PROXY_HEADERS", false)?,
            limits: ApiLimits {
                max_groups: env_u32("LUMO_MAX_GROUPS", 1_000)?,
                max_devices_per_group: env_u32("LUMO_MAX_DEVICES_PER_GROUP", 8)?,
                max_active_invites_per_group: env_u32("LUMO_MAX_ACTIVE_INVITES_PER_GROUP", 8)?,
                bootstrap_per_ip: env_u32("LUMO_BOOTSTRAP_PER_IP", 5)?,
                bootstrap_global: env_u32("LUMO_BOOTSTRAP_GLOBAL", 100)?,
                bootstrap_window_ms: env_i64("LUMO_BOOTSTRAP_WINDOW_SECONDS", 3_600)?
                    .saturating_mul(1_000),
                invite_ttl_ms: env_i64("LUMO_INVITE_TTL_SECONDS", 600)?.saturating_mul(1_000),
            },
        })
    }
}

fn env_bool(name: &str, default: bool) -> LumoResult<bool> {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Ok(true),
            "0" | "false" | "no" => Ok(false),
            _ => Err(LumoError::Configuration(format!(
                "{name} must be true or false"
            ))),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(LumoError::Configuration(format!(
            "unable to read {name}: {error}"
        ))),
    }
}

fn env_u32(name: &str, default: u32) -> LumoResult<u32> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| LumoError::Configuration(format!("{name} must be a positive integer")))?
        .unwrap_or(default);
    if value == 0 {
        return Err(LumoError::Configuration(format!(
            "{name} must be greater than zero"
        )));
    }
    Ok(value)
}

fn env_i64(name: &str, default: i64) -> LumoResult<i64> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| LumoError::Configuration(format!("{name} must be a positive integer")))?
        .unwrap_or(default);
    if value <= 0 {
        return Err(LumoError::Configuration(format!(
            "{name} must be greater than zero"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_server_secrets() {
        let config = ApiConfig {
            bind: "127.0.0.1:8443".parse().expect("bind"),
            database_path: PathBuf::from("test.sqlite3"),
            tls_cert_path: PathBuf::from("cert.pem"),
            tls_key_path: PathBuf::from("key.pem"),
            master_key: Zeroizing::new("master-key-that-must-not-leak-123456".to_owned()),
            enable_legacy_v1: true,
            legacy_password: Some(Zeroizing::new(
                "legacy-key-that-must-not-leak-123456".to_owned(),
            )),
            trust_proxy_headers: false,
            limits: ApiLimits::default(),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("master-key-that-must-not-leak"));
        assert!(!debug.contains("legacy-key-that-must-not-leak"));
    }
}
