use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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

    #[test]
    fn compact_state_round_trips_and_is_smaller_than_v1_json() {
        let mut original = record();
        original.envelope.ciphertext = (0..4_096).map(|index| (index % 251) as u8).collect();

        let compact = CompactRemoteStateRecord::from(&original);
        let restored = RemoteStateRecord::try_from(compact.clone()).expect("compact state");
        assert_eq!(restored, original);

        let legacy_json = serde_json::to_vec(&original).expect("legacy json");
        let compact_json = serde_json::to_vec(&compact).expect("compact json");
        assert!(compact_json.len() * 2 < legacy_json.len());
        assert!(serde_json::from_slice::<serde_json::Value>(&compact_json)
            .expect("json")
            .pointer("/envelope/ciphertext")
            .is_some_and(serde_json::Value::is_string));
    }

    #[test]
    fn compact_state_rejects_invalid_base64() {
        let mut compact = CompactRemoteStateRecord::from(&record());
        compact.envelope.nonce = "not+base64".to_owned();
        assert!(matches!(
            RemoteStateRecord::try_from(compact),
            Err(LumoError::InvalidInput(_))
        ));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutStateRequest {
    pub expected_revision: Option<u64>,
    pub record: RemoteStateRecord,
}

/// Compact v1 wire representation. Binary fields are base64url encoded instead
/// of JSON byte arrays, while the original endpoint remains available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactRemoteStateRecord {
    pub revision: u64,
    pub envelope: CompactSealedPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactPutStateRequest {
    pub expected_revision: Option<u64>,
    pub record: CompactRemoteStateRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactSealedPayload {
    version: u8,
    message_id: String,
    issued_at_ms: i64,
    expires_at_ms: i64,
    nonce: String,
    ciphertext: String,
}

impl From<&RemoteStateRecord> for CompactRemoteStateRecord {
    fn from(record: &RemoteStateRecord) -> Self {
        Self {
            revision: record.revision,
            envelope: CompactSealedPayload {
                version: record.envelope.version,
                message_id: record.envelope.message_id.clone(),
                issued_at_ms: record.envelope.issued_at_ms,
                expires_at_ms: record.envelope.expires_at_ms,
                nonce: URL_SAFE_NO_PAD.encode(&record.envelope.nonce),
                ciphertext: URL_SAFE_NO_PAD.encode(&record.envelope.ciphertext),
            },
        }
    }
}

impl TryFrom<CompactRemoteStateRecord> for RemoteStateRecord {
    type Error = LumoError;

    fn try_from(record: CompactRemoteStateRecord) -> Result<Self, Self::Error> {
        let decoded = Self {
            revision: record.revision,
            envelope: SealedPayload {
                version: record.envelope.version,
                message_id: record.envelope.message_id,
                issued_at_ms: record.envelope.issued_at_ms,
                expires_at_ms: record.envelope.expires_at_ms,
                nonce: decode_binary("nonce", &record.envelope.nonce)?,
                ciphertext: decode_binary("ciphertext", &record.envelope.ciphertext)?,
            },
        };
        decoded.validate()?;
        Ok(decoded)
    }
}

impl From<&PutStateRequest> for CompactPutStateRequest {
    fn from(request: &PutStateRequest) -> Self {
        Self {
            expected_revision: request.expected_revision,
            record: CompactRemoteStateRecord::from(&request.record),
        }
    }
}

impl TryFrom<CompactPutStateRequest> for PutStateRequest {
    type Error = LumoError;

    fn try_from(request: CompactPutStateRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            expected_revision: request.expected_revision,
            record: request.record.try_into()?,
        })
    }
}

fn decode_binary(field: &str, encoded: &str) -> LumoResult<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
        LumoError::InvalidInput(format!("compact state {field} is not valid base64url"))
    })
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
