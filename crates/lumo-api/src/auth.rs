use std::{collections::HashMap, sync::Mutex};

use axum::http::{HeaderMap, Method};
use lumo_core::{LumoError, LumoResult};
use lumo_protocol::{SignedHeaders, MAX_CLOCK_SKEW_MS};

use crate::ApiState;

const MAX_REPLAY_NONCES: usize = 4_096;

pub const TIMESTAMP_HEADER: &str = "x-lumo-timestamp";
pub const NONCE_HEADER: &str = "x-lumo-nonce";
pub const SIGNATURE_HEADER: &str = "x-lumo-signature";

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
        .authenticator
        .verify(method.as_str(), path, body, now_ms, &signed)?;
    state.replay.accept(&signed.nonce, now_ms)
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

pub fn system_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
