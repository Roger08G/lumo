use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupRole {
    Supervisor,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub code: String,
    pub supervisor_name: String,
    pub tracked_person_name: String,
    pub(crate) pin_hash: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invitation {
    pub id: String,
    pub(crate) token_hash: String,
    pub expires_at_ms: i64,
    pub used_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinGuard {
    pub failed_attempts: u8,
    pub locked_until_ms: Option<i64>,
}
