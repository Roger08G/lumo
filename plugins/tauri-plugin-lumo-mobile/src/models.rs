use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileStatus {
    pub platform: String,
    pub tracking_enabled: bool,
    pub role: Option<String>,
    pub precise_location: String,
    pub background_location: String,
    pub notifications: String,
    pub battery_optimization_disabled: bool,
    pub battery_percent: u8,
    pub location_services_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RolePayload<'a> {
    pub role: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackingPayload<'a> {
    pub role: &'a str,
    pub enabled: bool,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PhonePayload<'a> {
    pub number: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationPayload<'a> {
    pub id: Option<&'a str>,
    pub title: &'a str,
    pub body: &'a str,
    pub urgent: bool,
}
