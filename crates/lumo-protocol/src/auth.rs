use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use lumo_core::{LumoError, LumoResult};

pub const MAX_CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
pub const MIN_API_SECRET_BYTES: usize = 32;
const NONCE_LENGTH: usize = 32;
const SIGNATURE_LENGTH: usize = 43;
const STATE_KEY_CONTEXT: &[u8] = b"lumo-state-envelope-v1";

#[derive(Clone)]
pub struct RequestAuthenticator {
    password: Zeroizing<String>,
}

impl fmt::Debug for RequestAuthenticator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestAuthenticator")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedHeaders {
    pub timestamp_ms: i64,
    pub nonce: String,
    pub signature: String,
}

impl RequestAuthenticator {
    pub fn new(password: impl Into<String>) -> LumoResult<Self> {
        let password = password.into();
        if password.len() < MIN_API_SECRET_BYTES {
            return Err(LumoError::Configuration(format!(
                "API password must contain at least {MIN_API_SECRET_BYTES} bytes"
            )));
        }
        Ok(Self {
            password: Zeroizing::new(password),
        })
    }

    pub fn sign(&self, method: &str, path: &str, body: &[u8], timestamp_ms: i64) -> SignedHeaders {
        let mut nonce_bytes = [0_u8; 24];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
        SignedHeaders {
            signature: self.signature(method, path, body, timestamp_ms, &nonce),
            timestamp_ms,
            nonce,
        }
    }

    pub fn verify(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        now_ms: i64,
        headers: &SignedHeaders,
    ) -> LumoResult<()> {
        if headers.nonce.len() != NONCE_LENGTH || headers.signature.len() != SIGNATURE_LENGTH {
            return Err(LumoError::AuthenticationFailed);
        }
        let received = URL_SAFE_NO_PAD
            .decode(&headers.signature)
            .map_err(|_| LumoError::AuthenticationFailed)?;
        let mut verifier = Hmac::<Sha256>::new_from_slice(self.password.as_bytes())
            .map_err(|_| LumoError::Configuration("invalid HMAC key".to_owned()))?;
        verifier.update(
            canonical_request(method, path, body, headers.timestamp_ms, &headers.nonce).as_bytes(),
        );
        verifier
            .verify_slice(&received)
            .or(Err(LumoError::AuthenticationFailed))?;
        if now_ms.abs_diff(headers.timestamp_ms) > MAX_CLOCK_SKEW_MS as u64 {
            return Err(LumoError::ExpiredMessage);
        }
        Ok(())
    }

    fn signature(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        timestamp_ms: i64,
        nonce: &str,
    ) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.password.as_bytes())
            .expect("HMAC accepts arbitrary key sizes");
        mac.update(canonical_request(method, path, body, timestamp_ms, nonce).as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

fn canonical_request(
    method: &str,
    path: &str,
    body: &[u8],
    timestamp_ms: i64,
    nonce: &str,
) -> String {
    let body_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    format!(
        "{}\n{}\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        path,
        timestamp_ms,
        nonce,
        body_hash
    )
}

pub fn derive_state_key(password: &str) -> LumoResult<[u8; 32]> {
    if password.len() < MIN_API_SECRET_BYTES {
        return Err(LumoError::Configuration(format!(
            "API password must contain at least {MIN_API_SECRET_BYTES} bytes"
        )));
    }
    let hkdf = Hkdf::<Sha256>::new(Some(b"lumo-api-v1"), password.as_bytes());
    let mut key = [0_u8; 32];
    hkdf.expand(STATE_KEY_CONTEXT, &mut key)
        .map_err(|_| LumoError::Configuration("unable to derive state key".to_owned()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_bind_method_path_body_and_time() {
        let auth =
            RequestAuthenticator::new("a-secure-random-password-for-tests-32").expect("auth");
        let signed = auth.sign("PUT", "/v1/state", b"body", 1_000);
        assert!(auth
            .verify("PUT", "/v1/state", b"body", 1_001, &signed)
            .is_ok());
        assert_eq!(
            auth.verify("PUT", "/v1/state", b"tampered", 1_001, &signed),
            Err(LumoError::AuthenticationFailed)
        );
        assert_eq!(
            auth.verify(
                "PUT",
                "/v1/state",
                b"body",
                1_000 + MAX_CLOCK_SKEW_MS + 1,
                &signed,
            ),
            Err(LumoError::ExpiredMessage)
        );

        let mut malformed = signed;
        malformed.nonce.push('a');
        assert_eq!(
            auth.verify("PUT", "/v1/state", b"body", 1_001, &malformed),
            Err(LumoError::AuthenticationFailed)
        );
    }

    #[test]
    fn state_key_is_deterministic_and_context_bound() {
        let first = derive_state_key("a-secure-random-password-for-tests-32").expect("first");
        let second = derive_state_key("a-secure-random-password-for-tests-32").expect("second");
        assert_eq!(first, second);
        assert_ne!(
            first,
            derive_state_key("a-different-random-password-for-test-32").expect("different")
        );
    }
}
