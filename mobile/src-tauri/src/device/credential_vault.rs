use std::path::PathBuf;
#[cfg(target_os = "android")]
use std::str::FromStr;

#[cfg(target_os = "android")]
use lumo_core::LumoError;
use lumo_core::LumoResult;
#[cfg(target_os = "android")]
use lumo_runtime::DeviceRole;
#[cfg(not(target_os = "android"))]
use lumo_runtime::FileCredentialStore;
use lumo_runtime::{DeviceCredential, StoredDeviceCredential};
use tauri::{AppHandle, Runtime};

#[cfg(target_os = "android")]
use tauri_plugin_lumo_mobile::{DeviceCredential as MobileCredential, LumoMobileExt};

#[derive(Debug, Clone)]
pub struct DeviceCredentialVault {
    #[cfg(not(target_os = "android"))]
    fallback: FileCredentialStore,
}

impl DeviceCredentialVault {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        #[cfg(target_os = "android")]
        {
            let _ = path.into();
            Self {}
        }
        #[cfg(not(target_os = "android"))]
        Self {
            fallback: FileCredentialStore::new(path),
        }
    }

    pub fn load<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        api_origin: &str,
    ) -> LumoResult<Option<DeviceCredential>> {
        let stored = self.load_stored(app)?;
        stored
            .as_ref()
            .map(|value| DeviceCredential::from_stored(value, api_origin, false))
            .transpose()
    }

    pub fn store<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        credential: &DeviceCredential,
    ) -> LumoResult<()> {
        #[cfg(target_os = "android")]
        {
            let stored = credential.to_stored();
            let mobile = MobileCredential {
                version: stored.version,
                api_origin: stored.api_origin.clone(),
                group_id: stored.group_id.clone(),
                device_id: stored.device_id.clone(),
                role: stored.role.as_str().to_owned(),
                device_token: stored.device_token.clone(),
                state_key: stored.state_key.clone(),
            };
            app.lumo_mobile()
                .store_credential(&mobile)
                .map_err(|_| vault_error())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = app;
            self.fallback.store(credential)
        }
    }

    pub fn clear<R: Runtime>(&self, app: &AppHandle<R>) -> LumoResult<()> {
        #[cfg(target_os = "android")]
        {
            app.lumo_mobile()
                .clear_credential()
                .map_err(|_| vault_error())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = app;
            self.fallback.clear()
        }
    }

    fn load_stored<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> LumoResult<Option<StoredDeviceCredential>> {
        #[cfg(target_os = "android")]
        {
            app.lumo_mobile()
                .load_credential()
                .map_err(|_| vault_error())?
                .map(|mobile| {
                    Ok(StoredDeviceCredential {
                        version: mobile.version,
                        api_origin: mobile.api_origin.clone(),
                        group_id: mobile.group_id.clone(),
                        device_id: mobile.device_id.clone(),
                        role: DeviceRole::from_str(&mobile.role).map_err(|_| vault_error())?,
                        device_token: mobile.device_token.clone(),
                        state_key: mobile.state_key.clone(),
                    })
                })
                .transpose()
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = app;
            self.fallback.load()
        }
    }
}

#[cfg(target_os = "android")]
fn vault_error() -> LumoError {
    LumoError::Storage("secure device credential is invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumo_runtime::{DeviceRole, DEVICE_CREDENTIAL_VERSION};

    #[test]
    fn stored_contract_uses_protocol_role_and_version() {
        let stored = StoredDeviceCredential {
            version: DEVICE_CREDENTIAL_VERSION,
            api_origin: "https://api.example.test".to_owned(),
            group_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            device_id: "22222222-2222-4222-8222-222222222222".to_owned(),
            role: DeviceRole::Controlled,
            device_token: "redacted-in-debug".to_owned(),
            state_key: "also-redacted".to_owned(),
        };
        assert_eq!(stored.version, 1);
        assert_eq!(stored.role.as_str(), "controlled");
        assert!(!format!("{stored:?}").contains("redacted-in-debug"));
    }
}
