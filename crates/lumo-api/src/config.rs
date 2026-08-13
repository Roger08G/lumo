use std::{env, fmt, net::SocketAddr, path::PathBuf};

use lumo_core::{LumoError, LumoResult};
use lumo_protocol::MIN_API_SECRET_BYTES;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub database_path: PathBuf,
    pub tls_cert_path: PathBuf,
    pub tls_key_path: PathBuf,
    pub password: Zeroizing<String>,
}

impl fmt::Debug for ApiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiConfig")
            .field("bind", &self.bind)
            .field("database_path", &self.database_path)
            .field("tls_cert_path", &self.tls_cert_path)
            .field("tls_key_path", &self.tls_key_path)
            .field("password", &"[REDACTED]")
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
        let password = env::var("LUMO_API_PASSWORD")
            .map_err(|_| LumoError::Configuration("LUMO_API_PASSWORD is required".to_owned()))?;
        if password.len() < MIN_API_SECRET_BYTES {
            return Err(LumoError::Configuration(format!(
                "LUMO_API_PASSWORD must contain at least {MIN_API_SECRET_BYTES} bytes"
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
            password: Zeroizing::new(password),
        })
    }
}
