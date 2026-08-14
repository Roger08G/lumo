use std::{env, fmt, path::PathBuf};

use lumo_core::{LumoError, LumoResult};

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
}

impl fmt::Debug for RuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeConfig")
            .field("mode", &self.mode)
            .field("data_dir", &self.data_dir)
            .field("api_url", &self.api_url)
            .finish()
    }
}

impl RuntimeConfig {
    pub fn local(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            mode: RuntimeMode::Local,
            data_dir: data_dir.into(),
            api_url: None,
        }
    }

    pub fn from_env() -> LumoResult<Self> {
        let _ = dotenvy::dotenv();
        Self::from_values(
            env::var("LUMO_RUNTIME_MODE").ok().as_deref(),
            env::var_os("LUMO_DATA_DIR").map(PathBuf::from),
            env::var("LUMO_API_URL").ok().as_deref(),
        )
    }

    pub fn from_values(
        mode: Option<&str>,
        data_dir: Option<PathBuf>,
        api_url: Option<&str>,
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

        if mode == RuntimeMode::Remote {
            let url = api_url.as_deref().ok_or_else(|| {
                LumoError::Configuration("LUMO_API_URL is required in remote mode".to_owned())
            })?;
            if !url.starts_with("https://") {
                return Err(LumoError::Configuration(
                    "remote mode requires an https:// API URL".to_owned(),
                ));
            }
        }

        Ok(Self {
            mode,
            data_dir,
            api_url,
        })
    }

    /// Builds the configuration embedded in a mobile application.
    ///
    /// Debug builds may still opt into the local runtime for development. A release APK must be
    /// explicitly compiled for the remote runtime with an HTTPS origin; silently falling back to
    /// a local database would create an apparently working application that never synchronizes.
    pub fn from_mobile_values(
        mode: Option<&str>,
        data_dir: Option<PathBuf>,
        api_url: Option<&str>,
        release: bool,
    ) -> LumoResult<Self> {
        if release && !mode.is_some_and(|value| value.eq_ignore_ascii_case("remote")) {
            return Err(LumoError::Configuration(
                "mobile release requires LUMO_RUNTIME_MODE=remote".to_owned(),
            ));
        }
        let config = Self::from_values(mode, data_dir, api_url)?;
        if release && config.api_url.is_none() {
            return Err(LumoError::Configuration(
                "mobile release requires LUMO_API_URL".to_owned(),
            ));
        }
        Ok(config)
    }

    pub fn require_local(&self) -> LumoResult<()> {
        match self.mode {
            RuntimeMode::Local => Ok(()),
            RuntimeMode::Remote => Err(LumoError::RemoteUnavailable),
        }
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
    fn debug_output_contains_no_credentials() {
        let config = RuntimeConfig {
            mode: RuntimeMode::Remote,
            data_dir: PathBuf::from("data"),
            api_url: Some("https://example.test".into()),
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("https://example.test"));
        assert!(!debug.to_ascii_lowercase().contains("password"));
    }

    #[test]
    fn local_config_never_requires_remote_values() {
        assert!(RuntimeConfig::local("data").require_local().is_ok());
    }

    #[test]
    fn remote_values_fail_closed_without_an_https_origin() {
        assert!(
            RuntimeConfig::from_values(Some("remote"), None, Some("http://api.invalid"),).is_err()
        );
        assert!(RuntimeConfig::from_values(Some("remote"), None, None).is_err());
        assert!(
            RuntimeConfig::from_values(Some("remote"), None, Some("https://api.invalid"),).is_ok()
        );
    }

    #[test]
    fn mobile_release_never_falls_back_to_local_runtime() {
        assert!(RuntimeConfig::from_mobile_values(None, None, None, true).is_err());
        assert!(RuntimeConfig::from_mobile_values(
            Some("local"),
            None,
            Some("https://api.invalid"),
            true,
        )
        .is_err());
        assert!(RuntimeConfig::from_mobile_values(Some("remote"), None, None, true).is_err());
        let release = RuntimeConfig::from_mobile_values(
            Some("REMOTE"),
            None,
            Some("https://api.invalid"),
            true,
        )
        .expect("remote mobile release");
        assert_eq!(release.mode, RuntimeMode::Remote);
    }

    #[test]
    fn mobile_debug_and_cli_local_tools_keep_explicit_local_mode() {
        assert_eq!(
            RuntimeConfig::from_mobile_values(Some("local"), None, None, false)
                .expect("mobile debug")
                .mode,
            RuntimeMode::Local
        );
        assert_eq!(
            RuntimeConfig::from_values(None, None, None)
                .expect("CLI default")
                .mode,
            RuntimeMode::Local
        );
    }
}
