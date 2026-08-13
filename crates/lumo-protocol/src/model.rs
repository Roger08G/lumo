use lumo_core::{security::SealedPayload, LumoError, LumoResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const ENVELOPE_VERSION: u8 = 1;
const XCHACHA20_NONCE_LENGTH: usize = 24;
const POLY1305_TAG_LENGTH: usize = 16;
pub const MAX_ENCRYPTED_STATE_BYTES: usize = 512 * 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStateRecord {
    pub revision: u64,
    pub envelope: SealedPayload,
}

impl RemoteStateRecord {
    pub fn validate(&self) -> LumoResult<()> {
        if self.revision == 0 || self.revision > i64::MAX as u64 {
            return Err(LumoError::InvalidInput(
                "state revision is outside the supported range".to_owned(),
            ));
        }
        if self.envelope.version != ENVELOPE_VERSION
            || Uuid::parse_str(&self.envelope.message_id).is_err()
            || self.envelope.nonce.len() != XCHACHA20_NONCE_LENGTH
            || self.envelope.ciphertext.len() < POLY1305_TAG_LENGTH
            || self.envelope.ciphertext.len() > MAX_ENCRYPTED_STATE_BYTES
            || self.envelope.expires_at_ms < self.envelope.issued_at_ms
        {
            return Err(LumoError::InvalidInput(
                "state envelope is malformed".to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> RemoteStateRecord {
        RemoteStateRecord {
            revision: 1,
            envelope: SealedPayload {
                version: ENVELOPE_VERSION,
                message_id: Uuid::new_v4().to_string(),
                issued_at_ms: 10,
                expires_at_ms: 20,
                nonce: vec![0; XCHACHA20_NONCE_LENGTH],
                ciphertext: vec![0; POLY1305_TAG_LENGTH],
            },
        }
    }

    #[test]
    fn remote_state_record_rejects_malformed_metadata() {
        assert!(record().validate().is_ok());

        let mut malformed = record();
        malformed.envelope.nonce.pop();
        assert!(matches!(
            malformed.validate(),
            Err(LumoError::InvalidInput(_))
        ));

        let mut invalid_revision = record();
        invalid_revision.revision = 0;
        assert!(invalid_revision.validate().is_err());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutStateRequest {
    pub expected_revision: Option<u64>,
    pub record: RemoteStateRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub api_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
}
