use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileStatus {
    pub platform: String,
    pub tracking_enabled: bool,
    pub controlled_tracking_may_auto_recover: bool,
    pub role: Option<String>,
    pub precise_location: String,
    pub background_location: String,
    pub notifications: String,
    pub battery_optimization_disabled: bool,
    pub battery_percent: u8,
    pub location_services_enabled: bool,
    pub controller_notifications_configured: bool,
    pub controller_notifications_enabled: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCredential {
    pub version: u8,
    pub api_origin: String,
    pub group_id: String,
    pub device_id: String,
    pub role: String,
    pub device_token: String,
    pub state_key: String,
}

impl fmt::Debug for DeviceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceCredential")
            .field("version", &self.version)
            .field("api_origin", &self.api_origin)
            .field("group_id", &self.group_id)
            .field("device_id", &self.device_id)
            .field("role", &self.role)
            .field("device_token", &"[REDACTED]")
            .field("state_key", &"[REDACTED]")
            .finish()
    }
}

impl Drop for DeviceCredential {
    fn drop(&mut self) {
        self.device_token.zeroize();
        self.state_key.zeroize();
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct CredentialLoadResponse {
    pub credential: Option<DeviceCredential>,
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
pub(crate) struct CoordinatesPayload {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AddressResponse {
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationPayload<'a> {
    pub id: Option<&'a str>,
    pub title: &'a str,
    pub body: &'a str,
    pub urgent: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EmergencyAlarmPayload<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub phone: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAlarm {
    pub id: String,
    pub title: String,
    pub body: String,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PendingAlarmResponse {
    pub alarm: Option<PendingAlarm>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> DeviceCredential {
        DeviceCredential {
            version: 1,
            api_origin: "https://api.example.test".to_owned(),
            group_id: "group-id".to_owned(),
            device_id: "device-id".to_owned(),
            role: "controller".to_owned(),
            device_token: "device-token-that-must-never-appear-in-debug".to_owned(),
            state_key: "state-key-that-must-never-appear-in-debug-output".to_owned(),
        }
    }

    #[test]
    fn credential_debug_redacts_secrets() {
        let credential = credential();
        let debug = format!("{credential:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains(&credential.device_token));
        assert!(!debug.contains(&credential.state_key));
    }

    #[test]
    fn credential_uses_the_vault_v1_wire_shape() {
        let value = serde_json::to_value(credential()).expect("credential JSON");
        assert_eq!(value["version"], 1);
        assert_eq!(value["apiOrigin"], "https://api.example.test");
        assert_eq!(value["groupId"], "group-id");
        assert_eq!(value["deviceId"], "device-id");
        assert_eq!(value["role"], "controller");
        assert!(value.get("deviceToken").is_some());
        assert!(value.get("stateKey").is_some());
    }

    #[test]
    fn mobile_status_uses_durable_controller_notification_fields() {
        let value = serde_json::to_value(MobileStatus {
            platform: "android".to_owned(),
            tracking_enabled: false,
            controlled_tracking_may_auto_recover: false,
            role: None,
            precise_location: "denied".to_owned(),
            background_location: "denied".to_owned(),
            notifications: "denied".to_owned(),
            battery_optimization_disabled: false,
            battery_percent: 50,
            location_services_enabled: true,
            controller_notifications_configured: true,
            controller_notifications_enabled: false,
        })
        .expect("mobile status JSON");

        assert_eq!(value["controllerNotificationsConfigured"], true);
        assert_eq!(value["controllerNotificationsEnabled"], false);
    }
}
