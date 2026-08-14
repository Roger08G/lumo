pub mod auth;
pub mod model;

pub use auth::{
    derive_state_key, RequestAuthenticator, SignedHeaders, MAX_CLOCK_SKEW_MS, MIN_API_SECRET_BYTES,
};
pub use model::{
    ApiErrorBody, CompactPutStateRequest, CompactRemoteStateRecord, CompactSealedPayload,
    ConsumeInvitationRequest, ControlledOperation, ControlledOperationRequest,
    ControlledOperationResponse, CreateGroupRequest, CreateInvitationRequest,
    DeviceCredentialResponse, DeviceListResponse, DeviceRole, DeviceSummary, HealthResponse,
    InvitationResponse, MemberOperationEnvelopeRequest, ProtectedActionRequest, PutStateRequest,
    RemoteStateRecord, MAX_ENCRYPTED_STATE_BYTES,
};

pub const API_VERSION: &str = "v2";
pub const HEALTH_PATH: &str = "/health";
pub const STATE_PATH: &str = "/v1/state";
pub const COMPACT_STATE_PATH: &str = "/v1/state/compact";
pub const GROUPS_PATH: &str = "/v2/groups";
pub const INVITATIONS_PATH_PREFIX: &str = "/v2/invitations";

pub const DEVICE_ID_HEADER: &str = "x-lumo-device-id";
pub const TIMESTAMP_HEADER: &str = "x-lumo-timestamp";
pub const NONCE_HEADER: &str = "x-lumo-nonce";

pub fn group_path(group_id: &str) -> String {
    format!("{GROUPS_PATH}/{group_id}")
}

pub fn group_state_path(group_id: &str) -> String {
    format!("{}/{group_id}/state/compact", GROUPS_PATH)
}

pub fn group_member_path(group_id: &str) -> String {
    format!("{}/{group_id}/member", GROUPS_PATH)
}

pub fn group_member_operations_path(group_id: &str) -> String {
    format!("{}/{group_id}/member/operations", GROUPS_PATH)
}

pub fn group_verify_pin_path(group_id: &str) -> String {
    format!("{}/{group_id}/verify-pin", GROUPS_PATH)
}

pub fn group_invitations_path(group_id: &str) -> String {
    format!("{}/{group_id}/invitations", GROUPS_PATH)
}

pub fn invitation_consume_path(invitation_id: &str) -> String {
    format!("{INVITATIONS_PATH_PREFIX}/{invitation_id}/consume")
}

pub fn group_devices_path(group_id: &str) -> String {
    format!("{}/{group_id}/devices", GROUPS_PATH)
}

pub fn group_device_path(group_id: &str, device_id: &str) -> String {
    format!("{}/{group_id}/devices/{device_id}", GROUPS_PATH)
}

pub fn group_leave_path(group_id: &str) -> String {
    format!("{}/{group_id}/leave", GROUPS_PATH)
}
