use std::{collections::HashMap, sync::Mutex};

use axum::http::{header::AUTHORIZATION, HeaderMap, Method};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use lumo_core::{LumoError, LumoResult};
use lumo_protocol::{
    SignedHeaders, DEVICE_ID_HEADER, MAX_CLOCK_SKEW_MS, NONCE_HEADER, TIMESTAMP_HEADER,
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{storage::AuthenticatedDevice, ApiState};

const MAX_REPLAY_NONCES: usize = 4_096;

pub const SIGNATURE_HEADER: &str = "x-lumo-signature";

pub(crate) struct DeviceAuthAttempt {
    pub device_id: String,
    pub token: Zeroizing<String>,
    pub nonce: String,
}

impl std::fmt::Debug for DeviceAuthAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceAuthAttempt")
            .field("device_id", &self.device_id)
            .field("token", &"[REDACTED]")
            .field("nonce", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct ReplayProtection {
    nonces: Mutex<HashMap<String, i64>>,
}

impl ReplayProtection {
    fn accept(&self, nonce: &str, now_ms: i64) -> LumoResult<()> {
        let mut nonces = self
            .nonces
            .lock()
            .map_err(|_| LumoError::Storage("replay cache lock poisoned".to_owned()))?;
        nonces.retain(|_, timestamp| now_ms.saturating_sub(*timestamp) <= MAX_CLOCK_SKEW_MS);
        if nonces.contains_key(nonce) {
            return Err(LumoError::ReplayDetected);
        }
        if nonces.len() >= MAX_REPLAY_NONCES {
            return Err(LumoError::RateLimited);
        }
        nonces.insert(nonce.to_owned(), now_ms);
        Ok(())
    }
}

pub fn authenticate(
    state: &ApiState,
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
    now_ms: i64,
) -> LumoResult<()> {
    let signed = signed_headers(headers)?;
    state
        .legacy_authenticator
        .as_ref()
        .ok_or(LumoError::AuthenticationFailed)?
        .verify(method.as_str(), path, body, now_ms, &signed)?;
    state.legacy_replay.accept(&signed.nonce, now_ms)
}

pub async fn authenticate_device_read(
    state: &ApiState,
    group_id: &str,
    headers: &HeaderMap,
    now_ms: i64,
) -> LumoResult<AuthenticatedDevice> {
    authenticate_device_inner(state, group_id, headers, now_ms, false).await
}

pub async fn authenticate_device_mutation(
    state: &ApiState,
    group_id: &str,
    headers: &HeaderMap,
    now_ms: i64,
) -> LumoResult<AuthenticatedDevice> {
    authenticate_device_inner(state, group_id, headers, now_ms, true).await
}

async fn authenticate_device_inner(
    state: &ApiState,
    group_id: &str,
    headers: &HeaderMap,
    now_ms: i64,
    persist_nonce: bool,
) -> LumoResult<AuthenticatedDevice> {
    let attempt = parse_device_auth(group_id, headers, now_ms)?;
    let DeviceAuthAttempt {
        device_id,
        token,
        nonce,
    } = attempt;
    let store = state.store.clone();
    let master = state.master.clone();
    let group_id = group_id.to_owned();
    tokio::task::spawn_blocking(move || {
        if persist_nonce {
            store.authenticate_device_mutation_v2(
                &master, &group_id, &device_id, &token, &nonce, now_ms,
            )
        } else {
            store.authenticate_device_read_v2(&master, &group_id, &device_id, &token, now_ms)
        }
    })
    .await
    .map_err(|_| LumoError::Storage("API authentication task failed".to_owned()))?
}

pub(crate) fn parse_device_auth(
    group_id: &str,
    headers: &HeaderMap,
    now_ms: i64,
) -> LumoResult<DeviceAuthAttempt> {
    Uuid::parse_str(group_id).map_err(|_| LumoError::AuthenticationFailed)?;
    let device_id = header(headers, DEVICE_ID_HEADER)?.to_owned();
    Uuid::parse_str(&device_id).map_err(|_| LumoError::AuthenticationFailed)?;
    let token = Zeroizing::new(bearer_token(headers)?.to_owned());
    if URL_SAFE_NO_PAD
        .decode(&token)
        .ok()
        .filter(|bytes| bytes.len() == 32)
        .is_none()
    {
        return Err(LumoError::AuthenticationFailed);
    }
    let timestamp_ms = header(headers, TIMESTAMP_HEADER)?
        .parse::<i64>()
        .map_err(|_| LumoError::AuthenticationFailed)?;
    if now_ms.abs_diff(timestamp_ms) > MAX_CLOCK_SKEW_MS as u64 {
        return Err(LumoError::ExpiredMessage);
    }
    let nonce = header(headers, NONCE_HEADER)?.to_owned();
    if URL_SAFE_NO_PAD
        .decode(&nonce)
        .ok()
        .filter(|bytes| bytes.len() == 24)
        .is_none()
    {
        return Err(LumoError::AuthenticationFailed);
    }
    Ok(DeviceAuthAttempt {
        device_id,
        token,
        nonce,
    })
}

fn signed_headers(headers: &HeaderMap) -> LumoResult<SignedHeaders> {
    let timestamp_ms = header(headers, TIMESTAMP_HEADER)?
        .parse()
        .map_err(|_| LumoError::AuthenticationFailed)?;
    Ok(SignedHeaders {
        timestamp_ms,
        nonce: header(headers, NONCE_HEADER)?.to_owned(),
        signature: header(headers, SIGNATURE_HEADER)?.to_owned(),
    })
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> LumoResult<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or(LumoError::AuthenticationFailed)
}

fn bearer_token(headers: &HeaderMap) -> LumoResult<&str> {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(LumoError::AuthenticationFailed)?;
    authorization
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace))
        .ok_or(LumoError::AuthenticationFailed)
}

pub fn system_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
