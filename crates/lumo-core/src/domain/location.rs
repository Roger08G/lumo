use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Revoked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Connectivity {
    Online,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationSample {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_m: f32,
    pub captured_at_ms: i64,
    pub battery_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TripSummary {
    pub from: String,
    pub to: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub duration_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeofenceCandidate {
    pub place_id: Option<String>,
    pub confirmations: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlledDevice {
    pub precise_permission: PermissionState,
    pub background_permission: PermissionState,
    pub battery_optimization_disabled: bool,
    pub tracking_enabled: bool,
    pub connectivity: Connectivity,
    pub battery_percent: u8,
    pub last_seen_at_ms: Option<i64>,
    pub last_location: Option<LocationSample>,
    pub current_place_id: Option<String>,
    #[serde(default)]
    pub geofence_candidate: Option<GeofenceCandidate>,
    pub departed_place_id: Option<String>,
    pub departed_at_ms: Option<i64>,
    pub last_trip: Option<TripSummary>,
}

impl Default for ControlledDevice {
    fn default() -> Self {
        Self {
            precise_permission: PermissionState::Unknown,
            background_permission: PermissionState::Unknown,
            battery_optimization_disabled: false,
            tracking_enabled: false,
            connectivity: Connectivity::Offline,
            battery_percent: 100,
            last_seen_at_ms: None,
            last_location: None,
            current_place_id: None,
            geofence_candidate: None,
            departed_place_id: None,
            departed_at_ms: None,
            last_trip: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandKind {
    Locate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandStatus {
    Queued,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCommand {
    pub id: String,
    pub kind: CommandKind,
    pub status: CommandStatus,
    pub created_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub error_code: Option<String>,
}
