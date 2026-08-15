use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use lumo_core::{
    application::{ReportLocationInput, SetTrackingInput},
    domain::{AppSnapshot, Connectivity},
    security::SealedPayload,
    LumoError, LumoResult,
};
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

impl From<&SealedPayload> for CompactSealedPayload {
    fn from(envelope: &SealedPayload) -> Self {
        Self {
            version: envelope.version,
            message_id: envelope.message_id.clone(),
            issued_at_ms: envelope.issued_at_ms,
            expires_at_ms: envelope.expires_at_ms,
            nonce: URL_SAFE_NO_PAD.encode(&envelope.nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(&envelope.ciphertext),
        }
    }
}

impl TryFrom<CompactSealedPayload> for SealedPayload {
    type Error = LumoError;

    fn try_from(envelope: CompactSealedPayload) -> Result<Self, Self::Error> {
        let decoded = Self {
            version: envelope.version,
            message_id: envelope.message_id,
            issued_at_ms: envelope.issued_at_ms,
            expires_at_ms: envelope.expires_at_ms,
            nonce: decode_binary("nonce", &envelope.nonce)?,
            ciphertext: decode_binary("ciphertext", &envelope.ciphertext)?,
        };
        validate_envelope(&decoded)?;
        Ok(decoded)
    }
}

impl From<&RemoteStateRecord> for CompactRemoteStateRecord {
    fn from(record: &RemoteStateRecord) -> Self {
        Self {
            revision: record.revision,
            envelope: CompactSealedPayload::from(&record.envelope),
        }
    }
}

impl TryFrom<CompactRemoteStateRecord> for RemoteStateRecord {
    type Error = LumoError;

    fn try_from(record: CompactRemoteStateRecord) -> Result<Self, Self::Error> {
        let decoded = Self {
            revision: record.revision,
            envelope: record.envelope.try_into()?,
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

fn validate_envelope(envelope: &SealedPayload) -> LumoResult<()> {
    if envelope.version != ENVELOPE_VERSION
        || Uuid::parse_str(&envelope.message_id).is_err()
        || envelope.nonce.len() != XCHACHA20_NONCE_LENGTH
        || envelope.ciphertext.len() < POLY1305_TAG_LENGTH
        || envelope.ciphertext.len() > MAX_ENCRYPTED_STATE_BYTES
        || envelope.expires_at_ms < envelope.issued_at_ms
    {
        return Err(LumoError::InvalidInput(
            "sealed payload is malformed".to_owned(),
        ));
    }
    Ok(())
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Controller,
    #[default]
    Controlled,
}

impl DeviceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Controller => "controller",
            Self::Controlled => "controlled",
        }
    }
}

impl std::str::FromStr for DeviceRole {
    type Err = LumoError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "controller" => Ok(Self::Controller),
            "controlled" => Ok(Self::Controlled),
            _ => Err(LumoError::InvalidInput("invalid device role".to_owned())),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupRequest {
    pub request_id: String,
    pub pin: String,
    pub device_name: String,
}

impl std::fmt::Debug for CreateGroupRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateGroupRequest")
            .field("request_id", &self.request_id)
            .field("pin", &"[REDACTED]")
            .field("device_name", &self.device_name)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCredentialResponse {
    pub group_id: String,
    pub device_id: String,
    pub role: DeviceRole,
    pub device_token: String,
    pub state_key: String,
}

impl std::fmt::Debug for DeviceCredentialResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceCredentialResponse")
            .field("group_id", &self.group_id)
            .field("device_id", &self.device_id)
            .field("role", &self.role)
            .field("device_token", &"[REDACTED]")
            .field("state_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInvitationRequest {
    pub pin: String,
    #[serde(default)]
    pub role: DeviceRole,
}

impl std::fmt::Debug for CreateInvitationRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateInvitationRequest")
            .field("pin", &"[REDACTED]")
            .field("role", &self.role)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationResponse {
    pub invitation_id: String,
    pub token: String,
    pub expires_at_ms: i64,
    pub role: DeviceRole,
}

impl std::fmt::Debug for InvitationResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InvitationResponse")
            .field("invitation_id", &self.invitation_id)
            .field("token", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("role", &self.role)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumeInvitationRequest {
    pub request_id: String,
    pub token: String,
    pub pin: String,
    pub device_name: String,
}

impl std::fmt::Debug for ConsumeInvitationRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConsumeInvitationRequest")
            .field("request_id", &self.request_id)
            .field("token", &"[REDACTED]")
            .field("pin", &"[REDACTED]")
            .field("device_name", &self.device_name)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedActionRequest {
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum ControlledOperation {
    SetTracking(SetTrackingInput),
    ReportLocation(ReportLocationInput),
    SetConnectivity { connectivity: Connectivity },
    SendHelp,
    ProcessPending,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlledOperationRequest {
    pub operation_id: String,
    pub operation: ControlledOperation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlledOperationResponse {
    pub snapshot: AppSnapshot,
    pub processed: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberOperationEnvelopeRequest {
    pub envelope: CompactSealedPayload,
}

impl std::fmt::Debug for ProtectedActionRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProtectedActionRequest")
            .field("pin", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSummary {
    pub device_id: String,
    pub device_name: String,
    pub role: DeviceRole,
    pub created_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceSummary>,
}

#[cfg(test)]
mod v2_tests {
    use super::*;

    #[test]
    fn sensitive_v2_models_redact_debug_output() {
        let credential = DeviceCredentialResponse {
            group_id: Uuid::new_v4().to_string(),
            device_id: Uuid::new_v4().to_string(),
            role: DeviceRole::Controller,
            device_token: "device-token-that-must-not-leak".to_owned(),
            state_key: "state-key-that-must-not-leak".to_owned(),
        };
        let debug = format!("{credential:?}");
        assert!(!debug.contains("device-token-that-must-not-leak"));
        assert!(!debug.contains("state-key-that-must-not-leak"));

        let create = CreateGroupRequest {
            request_id: Uuid::new_v4().to_string(),
            pin: "pin-that-must-not-leak".to_owned(),
            device_name: "Device".to_owned(),
        };
        let invite = CreateInvitationRequest {
            pin: "second-pin-that-must-not-leak".to_owned(),
            role: DeviceRole::Controlled,
        };
        assert!(!format!("{create:?}").contains("pin-that-must-not-leak"));
        assert!(!format!("{invite:?}").contains("second-pin-that-must-not-leak"));
    }

    #[test]
    fn legacy_invitation_without_role_defaults_to_controlled() {
        let request: CreateInvitationRequest =
            serde_json::from_str(r#"{"pin":"123456"}"#).expect("deserialize legacy request");

        assert_eq!(request.role, DeviceRole::Controlled);
    }

    #[test]
    fn controlled_operation_wire_shape_is_stable_and_camel_case() {
        let operation_id = Uuid::new_v4().to_string();
        let request = ControlledOperationRequest {
            operation_id: operation_id.clone(),
            operation: ControlledOperation::SetConnectivity {
                connectivity: Connectivity::Online,
            },
        };
        let json = serde_json::to_value(&request).expect("serialize operation");
        assert_eq!(json["operationId"], operation_id);
        assert_eq!(json["operation"]["type"], "setConnectivity");
        assert_eq!(json["operation"]["payload"]["connectivity"], "online");
        assert_eq!(
            serde_json::from_value::<ControlledOperationRequest>(json).expect("deserialize"),
            request
        );
    }
}
