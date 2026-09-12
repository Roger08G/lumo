use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Algorithm, Argon2, Params, Version,
};

use crate::{LumoError, LumoResult};

fn algorithm() -> LumoResult<Argon2<'static>> {
    let params = Params::new(19_456, 2, 1, None)
        .map_err(|error| LumoError::Configuration(error.to_string()))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

pub fn validate_pin(pin: &str) -> LumoResult<()> {
    if pin.len() == 6 && pin.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(LumoError::InvalidInput(
            "PIN must contain exactly six digits".to_owned(),
        ))
    }
}

pub fn hash_pin(pin: &str) -> LumoResult<String> {
    validate_pin(pin)?;
    algorithm()?
        .hash_password(pin.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| LumoError::Configuration(error.to_string()))
}

pub fn verify_pin(pin: &str, encoded: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded) else {
        return false;
    };
    let Ok(algorithm) = algorithm() else {
        return false;
    };
    algorithm.verify_password(pin.as_bytes(), &parsed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_is_hashed_and_verified() {
        let encoded = hash_pin("123456").expect("pin should hash");
        assert_ne!(encoded, "123456");
        assert!(encoded.starts_with("$argon2id$"));
        assert!(verify_pin("123456", &encoded));
        assert!(!verify_pin("654321", &encoded));
    }

    #[test]
    fn pin_format_is_strict() {
        for invalid in ["12345", "1234567", "abcdef", "12 456"] {
            assert!(validate_pin(invalid).is_err());
        }
    }

    #[test]
    fn argon2_05_credentials_remain_valid_after_the_upgrade() {
        // Public test fixture generated with argon2 0.5.3, PIN 123456, salt
        // b"lumo-test-salt-05", Argon2id v19 and the application's m=19456,t=2,p=1.
        const LEGACY_PHC: &str = concat!(
            "$argon2id$v=19$m=19456,t=2,p=1$bHVtby10ZXN0LXNhbHQtMDU$",
            "PBmlEA5DaZUWHFMxlPrMbMJnC0UHolzPGS/jr7DDn34"
        );
        assert!(verify_pin("123456", LEGACY_PHC));
        assert!(!verify_pin("654321", LEGACY_PHC));
    }

    #[test]
    fn each_hash_gets_a_fresh_salt_without_changing_security_parameters() {
        let first = hash_pin("123456").expect("first hash");
        let second = hash_pin("123456").expect("second hash");
        for encoded in [&first, &second] {
            assert!(encoded.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
            assert!(verify_pin("123456", encoded));
        }
        let first_salt = PasswordHash::new(&first)
            .expect("first PHC")
            .salt
            .expect("first salt");
        let second_salt = PasswordHash::new(&second)
            .expect("second PHC")
            .salt
            .expect("second salt");
        assert_eq!(first_salt.len(), 16);
        assert_eq!(second_salt.len(), 16);
        assert_ne!(first_salt, second_salt);
    }
}
