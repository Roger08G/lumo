use std::collections::{HashSet, VecDeque};
use std::fmt;

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{LumoError, LumoResult};

const ENVELOPE_VERSION: u8 = 1;
const MAX_REPLAY_IDS: usize = 2_048;

#[derive(Clone)]
pub struct SessionCipher {
    key: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for SessionCipher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionCipher")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedPayload {
    pub version: u8,
    pub message_id: String,
    pub issued_at_ms: i64,
    pub expires_at_ms: i64,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct ReplayGuard {
    order: VecDeque<String>,
    known: HashSet<String>,
}

impl ReplayGuard {
    pub fn contains(&self, message_id: &str) -> bool {
        self.known.contains(message_id)
    }

    fn remember(&mut self, message_id: String) {
        if self.known.insert(message_id.clone()) {
            self.order.push_back(message_id);
        }
        while self.order.len() > MAX_REPLAY_IDS {
            if let Some(expired) = self.order.pop_front() {
                self.known.remove(&expired);
            }
        }
    }
}

impl SessionCipher {
    pub fn generate() -> Self {
        let mut key = vec![0_u8; 32];
        OsRng.fill_bytes(&mut key);
        Self {
            key: Zeroizing::new(key),
        }
    }

    pub fn from_key(key: [u8; 32]) -> Self {
        Self {
            key: Zeroizing::new(key.to_vec()),
        }
    }

    pub fn seal<T: Serialize>(
        &self,
        value: &T,
        now_ms: i64,
        ttl_ms: i64,
    ) -> LumoResult<SealedPayload> {
        if ttl_ms <= 0 {
            return Err(LumoError::InvalidInput(
                "message TTL must be positive".to_owned(),
            ));
        }

        let message_id = Uuid::new_v4().to_string();
        let expires_at_ms = now_ms.saturating_add(ttl_ms);
        let aad = associated_data(ENVELOPE_VERSION, &message_id, now_ms, expires_at_ms);
        let plaintext = serde_json::to_vec(value)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        let mut nonce = vec![0_u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_slice())
            .map_err(|_| LumoError::Configuration("invalid cipher key".to_owned()))?;
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| LumoError::AuthenticationFailed)?;

        Ok(SealedPayload {
            version: ENVELOPE_VERSION,
            message_id,
            issued_at_ms: now_ms,
            expires_at_ms,
            nonce,
            ciphertext,
        })
    }

    pub fn open<T: DeserializeOwned>(
        &self,
        envelope: &SealedPayload,
        now_ms: i64,
        replay_guard: &mut ReplayGuard,
    ) -> LumoResult<T> {
        if envelope.version != ENVELOPE_VERSION || envelope.nonce.len() != 24 {
            return Err(LumoError::AuthenticationFailed);
        }
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_slice())
            .map_err(|_| LumoError::Configuration("invalid cipher key".to_owned()))?;
        let aad = associated_data(
            envelope.version,
            &envelope.message_id,
            envelope.issued_at_ms,
            envelope.expires_at_ms,
        );
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(&envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| LumoError::AuthenticationFailed)?;

        if now_ms > envelope.expires_at_ms {
            return Err(LumoError::ExpiredMessage);
        }
        if replay_guard.contains(&envelope.message_id) {
            return Err(LumoError::ReplayDetected);
        }
        let value = serde_json::from_slice(&plaintext)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        replay_guard.remember(envelope.message_id.clone());
        Ok(value)
    }
}

fn associated_data(
    version: u8,
    message_id: &str,
    issued_at_ms: i64,
    expires_at_ms: i64,
) -> Vec<u8> {
    format!("lumo:{version}:{message_id}:{issued_at_ms}:{expires_at_ms}").into_bytes()
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Message {
        value: String,
    }

    #[test]
    fn authenticated_round_trip_and_replay_protection() {
        let cipher = SessionCipher::generate();
        let sealed = cipher
            .seal(&Message { value: "ok".into() }, 1_000, 5_000)
            .expect("message should seal");
        let mut replay = ReplayGuard::default();
        let opened: Message = cipher
            .open(&sealed, 2_000, &mut replay)
            .expect("message should open");
        assert_eq!(opened, Message { value: "ok".into() });
        assert_eq!(
            cipher.open::<Message>(&sealed, 2_001, &mut replay),
            Err(LumoError::ReplayDetected)
        );
    }

    #[test]
    fn wrong_key_tamper_and_expiry_fail_closed() {
        let cipher = SessionCipher::generate();
        let sealed = cipher.seal(&"secret", 1_000, 100).expect("seal");
        let mut replay = ReplayGuard::default();
        assert_eq!(
            SessionCipher::generate().open::<String>(&sealed, 1_050, &mut replay),
            Err(LumoError::AuthenticationFailed)
        );

        let mut tampered = sealed.clone();
        tampered.ciphertext[0] ^= 1;
        assert_eq!(
            cipher.open::<String>(&tampered, 1_050, &mut replay),
            Err(LumoError::AuthenticationFailed)
        );
        assert_eq!(
            cipher.open::<String>(&sealed, 1_101, &mut replay),
            Err(LumoError::ExpiredMessage)
        );
    }

    #[test]
    fn every_message_uses_a_unique_nonce() {
        let cipher = SessionCipher::generate();
        let first = cipher.seal(&1_u8, 10, 100).expect("first");
        let second = cipher.seal(&1_u8, 10, 100).expect("second");
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.message_id, second.message_id);
    }
}
