use std::{env, fmt, path::PathBuf};

use lumo_core::{LumoError, LumoResult};
use lumo_protocol::MIN_API_SECRET_BYTES;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Local,
    Remote,
}

#[derive(Clone)]
pub struct RuntimeConfig {
    pub mode: RuntimeMode,
    pub data_dir: PathBuf,
    pub api_url: Option<String>,
    api_password: Option<Zeroizing<String>>,
}

impl fmt::Debug for RuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeConfig")
            .field("mode", &self.mode)
            .field("data_dir", &self.data_dir)
            .field("api_url", &self.api_url)
            .field(
                "api_password",
                &self.api_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl RuntimeConfig {
    pub fn local(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            mode: RuntimeMode::Local,
            data_dir: data_dir.into(),
            api_url: None,
            api_password: None,
        }
    }

    pub fn from_env() -> LumoResult<Self> {
        let _ = dotenvy::dotenv();
        Self::from_values(
            env::var("LUMO_RUNTIME_MODE").ok().as_deref(),
            env::var_os("LUMO_DATA_DIR").map(PathBuf::from),
            env::var("LUMO_API_URL").ok().as_deref(),
            env::var("LUMO_API_PASSWORD").ok().as_deref(),
        )
    }

    pub fn from_values(
        mode: Option<&str>,
        data_dir: Option<PathBuf>,
        api_url: Option<&str>,
        api_password: Option<&str>,
    ) -> LumoResult<Self> {
        let mode = match mode.unwrap_or("local").to_ascii_lowercase().as_str() {
            "local" => RuntimeMode::Local,
            "remote" => RuntimeMode::Remote,
            value => {
                return Err(LumoError::Configuration(format!(
                    "unsupported LUMO_RUNTIME_MODE: {value}"
                )))
            }
        };
        let data_dir = data_dir.unwrap_or_else(|| PathBuf::from(".lumo-data"));
        let api_url = api_url
            .map(str::to_owned)
            .filter(|value| !value.trim().is_empty());
        let api_password = api_password
            .map(str::to_owned)
            .filter(|value| !value.is_empty())
            .map(Zeroizing::new);

        if mode == RuntimeMode::Remote {
            let url = api_url.as_deref().ok_or_else(|| {
                LumoError::Configuration("LUMO_API_URL is required in remote mode".to_owned())
            })?;
            if !url.starts_with("https://") {
                return Err(LumoError::Configuration(
                    "remote mode requires an https:// API URL".to_owned(),
                ));
            }
            if api_password.as_deref().map_or(0, |value| value.len()) < MIN_API_SECRET_BYTES {
                return Err(LumoError::Configuration(format!(
                    "LUMO_API_PASSWORD must contain at least {MIN_API_SECRET_BYTES} bytes"
                )));
            }
        }

        Ok(Self {
            mode,
            data_dir,
            api_url,
            api_password,
        })
    }

    pub fn require_local(&self) -> LumoResult<()> {
        match self.mode {
            RuntimeMode::Local => Ok(()),
            RuntimeMode::Remote => Err(LumoError::RemoteUnavailable),
        }
    }

    pub fn api_password(&self) -> Option<&str> {
        self.api_password.as_deref().map(String::as_str)
    }

    pub fn with_data_dir(mut self, data_dir: impl Into<PathBuf>) -> Self {
        self.data_dir = data_dir.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_password() {
        let config = RuntimeConfig {
            mode: RuntimeMode::Remote,
            data_dir: PathBuf::from("data"),
            api_url: Some("https://example.test".into()),
            api_password: Some(Zeroizing::new("very-secret-password".into())),
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("very-secret-password"));
    }

    #[test]
    fn local_config_never_requires_remote_values() {
        assert!(RuntimeConfig::local("data").require_local().is_ok());
    }

    #[test]
    fn remote_values_fail_closed_without_https_and_a_strong_password() {
        assert!(RuntimeConfig::from_values(
            Some("remote"),
            None,
            Some("http://api.invalid"),
            Some("a-long-enough-password"),
        )
        .is_err());
        assert!(RuntimeConfig::from_values(
            Some("remote"),
            None,
            Some("https://api.invalid"),
            Some("short"),
        )
        .is_err());
    }
}
