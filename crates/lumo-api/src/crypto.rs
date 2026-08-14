use std::fmt;

use argon2::{
    password_hash::{
        rand_core::OsRng as PasswordOsRng, PasswordHash, PasswordHasher, PasswordVerifier,
        SaltString,
    },
    Algorithm, Argon2, Params, Version,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use lumo_core::{LumoError, LumoResult};
use rand::{rngs::OsRng, RngCore};
use sha2::Sha256;
use zeroize::Zeroizing;

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;

#[derive(Clone)]
pub struct MasterKey {
    wrap_key: Zeroizing<Vec<u8>>,
    member_wrap_key: Zeroizing<Vec<u8>>,
    credential_key: Zeroizing<Vec<u8>>,
    rate_key: Zeroizing<Vec<u8>>,
    pin_key: Zeroizing<Vec<u8>>,
    idempotency_digest_key: Zeroizing<Vec<u8>>,
    idempotency_replay_key: Zeroizing<Vec<u8>>,
    database_check_key: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for MasterKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MasterKey")
            .field("keys", &"[REDACTED]")
            .finish()
    }
}

impl MasterKey {
    pub fn new(secret: &str) -> LumoResult<Self> {
        if secret.len() < KEY_BYTES {
            return Err(LumoError::Configuration(
                "server master key must contain at least 32 bytes".to_owned(),
            ));
        }
        let hkdf = Hkdf::<Sha256>::new(Some(b"lumo-server-v2"), secret.as_bytes());
        Ok(Self {
            wrap_key: Zeroizing::new(expand(&hkdf, b"group-state-key-wrap")?.to_vec()),
            member_wrap_key: Zeroizing::new(expand(&hkdf, b"controlled-member-key-wrap")?.to_vec()),
            credential_key: Zeroizing::new(expand(&hkdf, b"credential-token-hash")?.to_vec()),
            rate_key: Zeroizing::new(expand(&hkdf, b"bootstrap-rate-key")?.to_vec()),
            pin_key: Zeroizing::new(expand(&hkdf, b"group-pin-pepper")?.to_vec()),
            idempotency_digest_key: Zeroizing::new(
                expand(&hkdf, b"idempotency-request-digest")?.to_vec(),
            ),
            idempotency_replay_key: Zeroizing::new(
                expand(&hkdf, b"idempotency-response-replay")?.to_vec(),
            ),
            database_check_key: Zeroizing::new(
                expand(&hkdf, b"database-master-key-check")?.to_vec(),
            ),
        })
    }

    pub fn generate_state_key(&self) -> [u8; KEY_BYTES] {
        random_bytes()
    }

    pub fn random_token(&self) -> String {
        URL_SAFE_NO_PAD.encode(random_bytes())
    }

    pub fn token_hash(&self, token: &str) -> Vec<u8> {
        hmac(&self.credential_key, token.as_bytes())
    }

    pub fn verify_token_hash(&self, token: &str, expected: &[u8]) -> bool {
        let Ok(mut verifier) = <Hmac<Sha256> as Mac>::new_from_slice(&self.credential_key) else {
            return false;
        };
        verifier.update(token.as_bytes());
        verifier.verify_slice(expected).is_ok()
    }

    pub fn bootstrap_key(&self, peer: &str) -> String {
        URL_SAFE_NO_PAD.encode(hmac(&self.rate_key, peer.as_bytes()))
    }

    pub fn hash_group_pin(&self, group_id: &str, pin: &str) -> LumoResult<String> {
        lumo_core::security::validate_pin(pin)?;
        let material = self.group_pin_material(group_id, pin);
        let salt = SaltString::generate(&mut PasswordOsRng);
        pin_argon2()?
            .hash_password(&material, &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| LumoError::Configuration(error.to_string()))
    }

    pub fn verify_group_pin(&self, group_id: &str, pin: &str, encoded: &str) -> bool {
        if lumo_core::security::validate_pin(pin).is_err() {
            return false;
        }
        let Ok(parsed) = PasswordHash::new(encoded) else {
            return false;
        };
        let Ok(algorithm) = pin_argon2() else {
            return false;
        };
        algorithm
            .verify_password(&self.group_pin_material(group_id, pin), &parsed)
            .is_ok()
    }

    pub fn wrap_state_key(
        &self,
        group_id: &str,
        state_key: &[u8; KEY_BYTES],
    ) -> LumoResult<(Vec<u8>, Vec<u8>)> {
        let cipher = XChaCha20Poly1305::new_from_slice(&self.wrap_key)
            .map_err(|_| LumoError::Configuration("invalid master key".to_owned()))?;
        let mut nonce = vec![0_u8; NONCE_BYTES];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: state_key,
                    aad: group_key_aad(group_id).as_bytes(),
                },
            )
            .map_err(|_| LumoError::AuthenticationFailed)?;
        Ok((nonce, ciphertext))
    }

    pub fn unwrap_state_key(
        &self,
        group_id: &str,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> LumoResult<[u8; KEY_BYTES]> {
        if nonce.len() != NONCE_BYTES {
            return Err(LumoError::AuthenticationFailed);
        }
        let cipher = XChaCha20Poly1305::new_from_slice(&self.wrap_key)
            .map_err(|_| LumoError::Configuration("invalid master key".to_owned()))?;
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: group_key_aad(group_id).as_bytes(),
                },
            )
            .map_err(|_| LumoError::AuthenticationFailed)?;
        plaintext
            .try_into()
            .map_err(|_| LumoError::AuthenticationFailed)
    }

    pub fn wrap_member_key(
        &self,
        group_id: &str,
        device_id: &str,
        member_key: &[u8; KEY_BYTES],
    ) -> LumoResult<(Vec<u8>, Vec<u8>)> {
        encrypt(
            &self.member_wrap_key,
            member_key,
            member_key_aad(group_id, device_id).as_bytes(),
        )
    }

    pub fn unwrap_member_key(
        &self,
        group_id: &str,
        device_id: &str,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> LumoResult<[u8; KEY_BYTES]> {
        decrypt(
            &self.member_wrap_key,
            nonce,
            ciphertext,
            member_key_aad(group_id, device_id).as_bytes(),
        )?
        .try_into()
        .map_err(|_| LumoError::AuthenticationFailed)
    }

    /// Returns a keyed, length-delimited digest of a semantic request. The context prevents a
    /// request identifier from being replayed across endpoint families.
    pub fn idempotency_digest(&self, context: &str, fields: &[&[u8]]) -> Vec<u8> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.idempotency_digest_key)
            .expect("HMAC accepts 32-byte keys");
        mac.update(b"lumo:v2:idempotency\0");
        update_len_prefixed(&mut mac, context.as_bytes());
        for field in fields {
            update_len_prefixed(&mut mac, field);
        }
        mac.finalize().into_bytes().to_vec()
    }

    pub fn seal_replay_response(
        &self,
        kind: &str,
        request_id: &str,
        digest: &[u8],
        plaintext: &[u8],
    ) -> LumoResult<(Vec<u8>, Vec<u8>)> {
        encrypt(
            &self.idempotency_replay_key,
            plaintext,
            replay_aad(kind, request_id, digest).as_bytes(),
        )
    }

    pub fn open_replay_response(
        &self,
        kind: &str,
        request_id: &str,
        digest: &[u8],
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> LumoResult<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(decrypt(
            &self.idempotency_replay_key,
            nonce,
            ciphertext,
            replay_aad(kind, request_id, digest).as_bytes(),
        )?))
    }

    pub fn database_key_check(&self) -> Vec<u8> {
        hmac(
            &self.database_check_key,
            b"lumo:v2:database-master-key-check",
        )
    }

    pub fn verify_database_key_check(&self, stored: &[u8]) -> bool {
        let Ok(mut verifier) = <Hmac<Sha256> as Mac>::new_from_slice(&self.database_check_key)
        else {
            return false;
        };
        verifier.update(b"lumo:v2:database-master-key-check");
        verifier.verify_slice(stored).is_ok()
    }

    fn group_pin_material(&self, group_id: &str, pin: &str) -> Zeroizing<Vec<u8>> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.pin_key)
            .expect("HMAC accepts 32-byte keys");
        mac.update(b"lumo:v2:group-pin\0");
        mac.update(group_id.as_bytes());
        mac.update(b"\0");
        mac.update(pin.as_bytes());
        Zeroizing::new(mac.finalize().into_bytes().to_vec())
    }
}

fn pin_argon2() -> LumoResult<Argon2<'static>> {
    let params = Params::new(19_456, 2, 1, None)
        .map_err(|error| LumoError::Configuration(error.to_string()))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn expand(hkdf: &Hkdf<Sha256>, context: &[u8]) -> LumoResult<[u8; KEY_BYTES]> {
    let mut key = [0_u8; KEY_BYTES];
    hkdf.expand(context, &mut key)
        .map_err(|_| LumoError::Configuration("unable to derive server key".to_owned()))?;
    Ok(key)
}

fn random_bytes() -> [u8; KEY_BYTES] {
    let mut bytes = [0_u8; KEY_BYTES];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

fn hmac(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts 32-byte keys");
    mac.update(value);
    mac.finalize().into_bytes().to_vec()
}

fn encrypt(key: &[u8], plaintext: &[u8], aad: &[u8]) -> LumoResult<(Vec<u8>, Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| LumoError::Configuration("invalid master key".to_owned()))?;
    let mut nonce = vec![0_u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| LumoError::AuthenticationFailed)?;
    Ok((nonce, ciphertext))
}

fn decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> LumoResult<Vec<u8>> {
    if nonce.len() != NONCE_BYTES {
        return Err(LumoError::AuthenticationFailed);
    }
    XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| LumoError::Configuration("invalid master key".to_owned()))?
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| LumoError::AuthenticationFailed)
}

fn update_len_prefixed(mac: &mut Hmac<Sha256>, value: &[u8]) {
    mac.update(&(value.len() as u64).to_be_bytes());
    mac.update(value);
}

fn group_key_aad(group_id: &str) -> String {
    format!("lumo:v2:group-state-key:{group_id}")
}

fn member_key_aad(group_id: &str, device_id: &str) -> String {
    format!("lumo:v2:member-key:{group_id}:{device_id}")
}

fn replay_aad(kind: &str, request_id: &str, digest: &[u8]) -> String {
    format!(
        "lumo:v2:replay:{kind}:{request_id}:{}",
        URL_SAFE_NO_PAD.encode(digest)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_keys_are_wrapped_and_bound_to_the_group() {
        let master = MasterKey::new("test-master-key-with-at-least-32-bytes").expect("master");
        let other = MasterKey::new("other-master-key-with-at-least-32-byte").expect("master");
        let key = master.generate_state_key();
        let (nonce, wrapped) = master.wrap_state_key("group-a", &key).expect("wrap");
        assert_ne!(wrapped, key);
        assert_eq!(
            master
                .unwrap_state_key("group-a", &nonce, &wrapped)
                .expect("unwrap"),
            key
        );
        assert!(master
            .unwrap_state_key("group-b", &nonce, &wrapped)
            .is_err());
        assert!(other.unwrap_state_key("group-a", &nonce, &wrapped).is_err());
    }

    #[test]
    fn member_keys_use_distinct_group_and_device_bound_wrapping() {
        let master = MasterKey::new("test-master-key-with-at-least-32-bytes").expect("master");
        let member_key = master.generate_state_key();
        let (nonce, wrapped) = master
            .wrap_member_key("group-a", "device-a", &member_key)
            .expect("wrap member key");
        assert_eq!(
            master
                .unwrap_member_key("group-a", "device-a", &nonce, &wrapped)
                .expect("unwrap member key"),
            member_key
        );
        assert!(master
            .unwrap_member_key("group-b", "device-a", &nonce, &wrapped)
            .is_err());
        assert!(master
            .unwrap_member_key("group-a", "device-b", &nonce, &wrapped)
            .is_err());
        assert!(master
            .unwrap_state_key("group-a", &nonce, &wrapped)
            .is_err());
    }

    #[test]
    fn replay_responses_are_encrypted_and_bound_to_the_request_digest() {
        let master = MasterKey::new("test-master-key-with-at-least-32-bytes").expect("master");
        let digest = master.idempotency_digest("create_group", &[b"123456", b"Device"]);
        let plaintext = b"credential-secret";
        let (nonce, ciphertext) = master
            .seal_replay_response("create_group", "request-a", &digest, plaintext)
            .expect("seal replay");
        assert_ne!(ciphertext, plaintext);
        assert_eq!(
            master
                .open_replay_response("create_group", "request-a", &digest, &nonce, &ciphertext,)
                .expect("open replay")
                .as_slice(),
            plaintext
        );
        let changed = master.idempotency_digest("create_group", &[b"654321", b"Device"]);
        assert!(master
            .open_replay_response("create_group", "request-a", &changed, &nonce, &ciphertext,)
            .is_err());
    }

    #[test]
    fn credential_hashes_are_keyed_and_verified_without_storing_tokens() {
        let master = MasterKey::new("test-master-key-with-at-least-32-bytes").expect("master");
        let token = master.random_token();
        let digest = master.token_hash(&token);
        assert_ne!(digest, token.as_bytes());
        assert!(master.verify_token_hash(&token, &digest));
        assert!(!master.verify_token_hash("wrong-token", &digest));
        let other = MasterKey::new("other-master-key-with-at-least-32-byte").expect("master");
        assert!(!other.verify_token_hash(&token, &digest));
    }

    #[test]
    fn pin_hash_requires_the_same_master_key_and_group_binding() {
        let master = MasterKey::new("test-master-key-with-at-least-32-bytes").expect("master");
        let other = MasterKey::new("other-master-key-with-at-least-32-byte").expect("master");
        let encoded = master
            .hash_group_pin("group-a", "123456")
            .expect("pin hash");

        assert!(master.verify_group_pin("group-a", "123456", &encoded));
        assert!(!master.verify_group_pin("group-b", "123456", &encoded));
        assert!(!other.verify_group_pin("group-a", "123456", &encoded));
        assert!(!lumo_core::security::verify_pin("123456", &encoded));
    }
}
