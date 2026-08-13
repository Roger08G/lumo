use serde::{Deserialize, Serialize};

pub const EVENT_TTL_MS: i64 = 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Arrival,
    Departure,
    Location,
    Warning,
    Help,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub id: String,
    pub sequence: u64,
    pub kind: EventKind,
    pub occurred_at_ms: i64,
    pub title: String,
    pub detail: String,
    pub place_id: Option<String>,
    pub read_at_ms: Option<i64>,
}
