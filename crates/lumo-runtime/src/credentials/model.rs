use std::{
    fmt,
    sync::{Arc, RwLock},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use lumo_core::{security::SessionCipher, LumoError, LumoResult};
use lumo_protocol::DeviceRole;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub const DEVICE_CREDENTIAL_VERSION: u8 = 1;
const SECRET_BYTES: usize = 32;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredDeviceCredential {
    pub version: u8,
    pub api_origin: String,
    pub group_id: String,
    pub device_id: String,
    pub role: DeviceRole,
    pub device_token: String,
    pub state_key: String,
}

impl fmt::Debug for StoredDeviceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredDeviceCredential")
            .field("version", &self.version)
            .field("api_origin", &self.api_origin)
            .field("group_id", &self.group_id)
            .field("device_id", &self.device_id)
            .field("role", &self.role)
            .field("device_token", &"[REDACTED]")
            .field("state_key", &"[REDACTED]")
            .finish()
    }
}

impl Drop for StoredDeviceCredential {
    fn drop(&mut self) {
        self.device_token.zeroize();
        self.state_key.zeroize();
    }
}

#[derive(Clone)]
pub struct DeviceCredential {
    api_origin: String,
    group_id: String,
    device_id: String,
    role: DeviceRole,
    device_token: Zeroizing<String>,
    state_key: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for DeviceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceCredential")
            .field("api_origin", &self.api_origin)
            .field("group_id", &self.group_id)
            .field("device_id", &self.device_id)
            .field("role", &self.role)
            .field("device_token", &"[REDACTED]")
            .field("state_key", &"[REDACTED]")
            .finish()
    }
}

impl DeviceCredential {
    pub fn from_stored(
        stored: &StoredDeviceCredential,
        expected_api_origin: &str,
        allow_insecure_http: bool,
    ) -> LumoResult<Self> {
        if stored.version != DEVICE_CREDENTIAL_VERSION {
            return Err(LumoError::Configuration(
                "unsupported device credential version".to_owned(),
            ));
        }
        let expected_origin = normalize_api_origin(expected_api_origin, allow_insecure_http)?;
        let stored_origin = normalize_api_origin(&stored.api_origin, allow_insecure_http)?;
        if stored_origin != expected_origin {
            return Err(LumoError::AuthenticationFailed);
        }
        validate_identifier("group", &stored.group_id)?;
        validate_identifier("device", &stored.device_id)?;
        let token = decode_secret("device token", &stored.device_token)?;
        let state_key = decode_secret("state key", &stored.state_key)?;

        Ok(Self {
            api_origin: stored_origin,
            group_id: stored.group_id.clone(),
            device_id: stored.device_id.clone(),
            role: stored.role,
            device_token: Zeroizing::new(URL_SAFE_NO_PAD.encode(token)),
            state_key: Zeroizing::new(state_key.to_vec()),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        api_origin: &str,
        group_id: impl Into<String>,
        device_id: impl Into<String>,
        role: DeviceRole,
        device_token: impl Into<String>,
        state_key: impl Into<String>,
        allow_insecure_http: bool,
    ) -> LumoResult<Self> {
        let stored = StoredDeviceCredential {
            version: DEVICE_CREDENTIAL_VERSION,
            api_origin: api_origin.to_owned(),
            group_id: group_id.into(),
            device_id: device_id.into(),
            role,
            device_token: device_token.into(),
            state_key: state_key.into(),
        };
        Self::from_stored(&stored, api_origin, allow_insecure_http)
    }

    pub fn to_stored(&self) -> StoredDeviceCredential {
        StoredDeviceCredential {
            version: DEVICE_CREDENTIAL_VERSION,
            api_origin: self.api_origin.clone(),
            group_id: self.group_id.clone(),
            device_id: self.device_id.clone(),
            role: self.role,
            device_token: self.device_token.to_string(),
            state_key: URL_SAFE_NO_PAD.encode(self.state_key.as_slice()),
        }
    }

    pub fn api_origin(&self) -> &str {
        &self.api_origin
    }

    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn role(&self) -> DeviceRole {
        self.role
    }

    pub(crate) fn device_token(&self) -> &str {
        self.device_token.as_str()
    }

    pub(crate) fn cipher(&self) -> LumoResult<SessionCipher> {
        let mut key: [u8; SECRET_BYTES] = self
            .state_key
            .as_slice()
            .try_into()
            .map_err(|_| LumoError::Configuration("invalid state key length".to_owned()))?;
        let cipher = SessionCipher::from_key(key);
        key.zeroize();
        Ok(cipher)
    }

    pub(crate) fn cache_fingerprint(&self) -> u64 {
        u64::from_le_bytes(
            self.state_key[..8]
                .try_into()
                .expect("validated state keys contain at least eight bytes"),
        )
    }
}

#[derive(Clone, Default)]
pub struct CredentialSlot {
    inner: Arc<RwLock<Option<DeviceCredential>>>,
}

impl fmt::Debug for CredentialSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialSlot")
            .field("credential", &self.get().ok().flatten())
            .finish()
    }
}

impl CredentialSlot {
    pub fn new(credential: Option<DeviceCredential>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(credential)),
        }
    }

    pub fn get(&self) -> LumoResult<Option<DeviceCredential>> {
        self.inner
            .read()
            .map(|credential| credential.clone())
            .map_err(|_| LumoError::Storage("device credential lock poisoned".to_owned()))
    }

    pub fn require(&self) -> LumoResult<DeviceCredential> {
        self.get()?.ok_or(LumoError::AuthenticationFailed)
    }

    pub fn install(&self, credential: DeviceCredential) -> LumoResult<()> {
        *self
            .inner
            .write()
            .map_err(|_| LumoError::Storage("device credential lock poisoned".to_owned()))? =
            Some(credential);
        Ok(())
    }

    pub fn clear(&self) -> LumoResult<()> {
        *self
            .inner
            .write()
            .map_err(|_| LumoError::Storage("device credential lock poisoned".to_owned()))? = None;
        Ok(())
    }
}

pub fn normalize_api_origin(origin: &str, allow_insecure_http: bool) -> LumoResult<String> {
    let url = Url::parse(origin.trim())
        .map_err(|error| LumoError::Configuration(format!("LUMO_API_URL is invalid: {error}")))?;
    let secure = url.scheme() == "https";
    if !secure && !(allow_insecure_http && url.scheme() == "http") {
        return Err(LumoError::Configuration(
            "remote API requires HTTPS".to_owned(),
        ));
    }
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(LumoError::Configuration(
            "LUMO_API_URL must be an origin URL without credentials, path, query, or fragment"
                .to_owned(),
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn validate_identifier(name: &str, value: &str) -> LumoResult<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| LumoError::Configuration(format!("invalid {name} identifier")))
}

fn decode_secret(name: &str, encoded: &str) -> LumoResult<[u8; SECRET_BYTES]> {
    let mut decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| LumoError::Configuration(format!("invalid {name} encoding")))?,
    );
    let secret = decoded
        .as_slice()
        .try_into()
        .map_err(|_| LumoError::Configuration(format!("invalid {name} length")))?;
    decoded.zeroize();
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(role: DeviceRole) -> StoredDeviceCredential {
        StoredDeviceCredential {
            version: DEVICE_CREDENTIAL_VERSION,
            api_origin: "https://api.example.test".to_owned(),
            group_id: Uuid::new_v4().to_string(),
            device_id: Uuid::new_v4().to_string(),
            role,
            device_token: URL_SAFE_NO_PAD.encode([7_u8; SECRET_BYTES]),
            state_key: URL_SAFE_NO_PAD.encode([9_u8; SECRET_BYTES]),
        }
    }

    #[test]
    fn credentials_validate_and_debug_output_is_redacted() {
        let stored = stored(DeviceRole::Controller);
        let token = stored.device_token.clone();
        let key = stored.state_key.clone();
        let credential = DeviceCredential::from_stored(&stored, "https://api.example.test", false)
            .expect("credential");
        assert_eq!(credential.role(), DeviceRole::Controller);
        for debug in [format!("{stored:?}"), format!("{credential:?}")] {
            assert!(debug.contains("[REDACTED]"));
            assert!(!debug.contains(&token));
            assert!(!debug.contains(&key));
        }
        assert!(matches!(
            DeviceCredential::from_stored(&stored, "https://other.example.test", false),
            Err(LumoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn credential_slot_can_be_rotated_and_cleared() {
        let first = DeviceCredential::from_stored(
            &stored(DeviceRole::Controlled),
            "https://api.example.test",
            false,
        )
        .expect("credential");
        let slot = CredentialSlot::default();
        slot.install(first.clone()).expect("install");
        assert_eq!(
            slot.require().expect("credential").device_id(),
            first.device_id()
        );
        slot.clear().expect("clear");
        assert!(slot.get().expect("empty").is_none());
    }
}
