use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
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
    let salt = SaltString::generate(&mut OsRng);
    algorithm()?
        .hash_password(pin.as_bytes(), &salt)
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
}
