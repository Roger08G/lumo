use std::{panic, path::PathBuf, ptr};

use jni::{
    objects::{JClass, JString},
    sys::jstring,
    JNIEnv,
};
use lumo_core::{
    application::{ReportLocationInput, SetTrackingInput},
    domain::{Connectivity, EventKind, RuntimeProfile},
};
use lumo_runtime::{ConfiguredRepository, LocalBackend, RuntimeConfig, SystemClock};
use lumo_runtime::{DeviceCredential, DeviceRole, RuntimeMode, StoredDeviceCredential};
use serde::{Deserialize, Serialize};

use crate::device::DeviceBinding;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundTick {
    role: String,
    timestamp_ms: i64,
    data_dir: PathBuf,
    battery_percent: u8,
    #[serde(default)]
    precise_location_granted: bool,
    #[serde(default)]
    background_location_granted: bool,
    #[serde(default)]
    battery_optimization_disabled: bool,
    location: Option<BackgroundLocation>,
    device_credential: Option<StoredDeviceCredential>,
    #[serde(default)]
    acknowledge_alarm_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundLocation {
    latitude: f64,
    longitude: f64,
    accuracy: f32,
    timestamp_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundResponse {
    notifications: Vec<BackgroundNotification>,
    error: Option<String>,
    error_code: Option<String>,
}

#[derive(Debug, Serialize)]
struct BackgroundNotification {
    id: String,
    title: String,
    body: String,
    urgent: bool,
    phone: Option<String>,
    address: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

#[no_mangle]
pub extern "system" fn Java_app_lumo_family_mobile_LumoRustBridge_processBackgroundTick(
    mut environment: JNIEnv<'_>,
    _class: JClass<'_>,
    payload: JString<'_>,
) -> jstring {
    let response = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let payload = environment
            .get_string(&payload)
            .map(|value| value.into())
            .map_err(|_| BackgroundFailure::new("invalid_payload", "invalid background payload"))?;
        process_tick(payload)
    }))
    .unwrap_or_else(|_| {
        Err(BackgroundFailure::new(
            "runtime_error",
            "background runtime failed safely",
        ))
    })
    .unwrap_or_else(|error| {
        serde_json::to_string(&BackgroundResponse {
            notifications: Vec::new(),
            error: Some(error.message),
            error_code: Some(error.code.to_owned()),
        })
        .unwrap_or_else(|_| {
            "{\"notifications\":[],\"error\":\"serialization failure\",\"errorCode\":\"runtime_error\"}".to_owned()
        })
    });

    environment
        .new_string(response)
        .map(JString::into_raw)
        .unwrap_or(ptr::null_mut())
}

fn process_tick(payload: String) -> Result<String, BackgroundFailure> {
    let tick: BackgroundTick = serde_json::from_str(&payload)
        .map_err(|_| BackgroundFailure::new("invalid_payload", "invalid background payload"))?;
    let profile = match tick.role.as_str() {
        "controlled" => RuntimeProfile::Controlled,
        "controller" => RuntimeProfile::Controller,
        _ => {
            return Err(BackgroundFailure::new(
                "credential_invalid",
                "unsupported background role",
            ))
        }
    };
    if !tick.data_dir.is_absolute() {
        return Err(BackgroundFailure::new(
            "credential_invalid",
            "background data directory must be absolute",
        ));
    }
    let config = RuntimeConfig::from_mobile_values(
        option_env!("LUMO_RUNTIME_MODE"),
        Some(tick.data_dir.join("runtime")),
        option_env!("LUMO_API_URL"),
        !cfg!(debug_assertions),
    )
    .map_err(|_| BackgroundFailure::new("credential_invalid", "invalid runtime configuration"))?;
    let repository = ConfiguredRepository::open(&config).map_err(background_runtime_error)?;
    if config.mode == RuntimeMode::Remote {
        let expected_origin = config
            .api_url
            .as_deref()
            .ok_or_else(|| BackgroundFailure::new("credential_invalid", "missing API origin"))?;
        let stored = tick.device_credential.as_ref().ok_or_else(|| {
            BackgroundFailure::new("credential_invalid", "missing device credential")
        })?;
        let credential =
            DeviceCredential::from_stored(stored, expected_origin, false).map_err(|_| {
                BackgroundFailure::new("credential_invalid", "invalid device credential")
            })?;
        let expected_role = match profile {
            RuntimeProfile::Controller => DeviceRole::Controller,
            RuntimeProfile::Controlled => DeviceRole::Controlled,
            RuntimeProfile::Debug => unreachable!(),
        };
        if credential.role() != expected_role {
            return Err(BackgroundFailure::new(
                "credential_invalid",
                "device credential role mismatch",
            ));
        }
        let binding = DeviceBinding::open(config.data_dir.join("device-binding.json"))
            .map_err(|_| BackgroundFailure::new("credential_invalid", "invalid device binding"))?;
        if binding.require_bound().ok() != Some(profile) {
            return Err(BackgroundFailure::new(
                "credential_invalid",
                "device binding role mismatch",
            ));
        }
        repository.install_credential(credential).map_err(|_| {
            BackgroundFailure::new("credential_invalid", "invalid device credential")
        })?;
    }
    let backend = LocalBackend::new(repository, SystemClock);

    let mut snapshot = if profile == RuntimeProfile::Controlled {
        let current = backend
            .snapshot(profile)
            .map_err(background_runtime_error)?;
        if !current.controlled.tracking_enabled
            && tick.precise_location_granted
            && tick.background_location_granted
        {
            backend
                .set_tracking(SetTrackingInput {
                    precise_permission: lumo_core::domain::PermissionState::Granted,
                    background_permission: lumo_core::domain::PermissionState::Granted,
                    battery_optimization_disabled: tick.battery_optimization_disabled,
                    enabled: true,
                })
                .map_err(background_runtime_error)?;
        }
        if let Some(location) = tick.location {
            backend
                .report_location(ReportLocationInput {
                    latitude: location.latitude,
                    longitude: location.longitude,
                    accuracy_m: location.accuracy,
                    battery_percent: tick.battery_percent,
                    captured_at_ms: Some(location.timestamp_ms),
                })
                .map_err(background_runtime_error)?
        } else {
            let snapshot = backend
                .snapshot(profile)
                .map_err(background_runtime_error)?;
            if snapshot.controlled.tracking_enabled {
                backend
                    .set_connectivity(Connectivity::Online)
                    .map_err(background_runtime_error)?
            } else {
                snapshot
            }
        }
    } else {
        backend
            .snapshot(profile)
            .map_err(background_runtime_error)?
    };

    if profile == RuntimeProfile::Controller {
        if let Some(alarm_id) = tick
            .acknowledge_alarm_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            let is_pending_help = snapshot.events.iter().any(|event| {
                event.id == alarm_id && event.kind == EventKind::Help && event.read_at_ms.is_none()
            });
            if is_pending_help {
                snapshot = backend
                    .mark_events_read()
                    .map_err(background_runtime_error)?;
            }
        }
    }

    if profile == RuntimeProfile::Controller
        && snapshot.controlled.connectivity == Connectivity::Online
        && snapshot
            .controlled
            .last_seen_at_ms
            .is_some_and(|last_seen| tick.timestamp_ms.saturating_sub(last_seen) > 300_000)
    {
        snapshot = backend
            .set_connectivity(Connectivity::Offline)
            .map_err(background_runtime_error)?;
    }

    let help_phone = snapshot
        .session
        .as_ref()
        .map(|session| session.tracked_person_phone.clone())
        .filter(|phone| !phone.trim().is_empty());
    let help_location = snapshot.controlled.last_location.as_ref().map(|location| {
        (
            location.latitude,
            location.longitude,
            snapshot
                .controlled
                .current_place_id
                .as_deref()
                .and_then(|place_id| snapshot.places.iter().find(|place| place.id == place_id))
                .map(|place| place.address.trim().to_owned())
                .filter(|address| !address.is_empty()),
        )
    });
    let notifications = if profile == RuntimeProfile::Controller {
        snapshot
            .events
            .into_iter()
            .filter(|event| {
                event.read_at_ms.is_none()
                    && matches!(
                        event.kind,
                        EventKind::Arrival
                            | EventKind::Departure
                            | EventKind::Warning
                            | EventKind::Help
                    )
            })
            .map(|event| {
                let urgent = event.kind == EventKind::Help;
                BackgroundNotification {
                    id: event.id,
                    title: event.title,
                    body: event.detail,
                    urgent,
                    phone: if urgent { help_phone.clone() } else { None },
                    address: if urgent {
                        help_location
                            .as_ref()
                            .and_then(|(_, _, address)| address.clone())
                    } else {
                        None
                    },
                    latitude: if urgent {
                        help_location.as_ref().map(|(latitude, _, _)| *latitude)
                    } else {
                        None
                    },
                    longitude: if urgent {
                        help_location.as_ref().map(|(_, longitude, _)| *longitude)
                    } else {
                        None
                    },
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    serde_json::to_string(&BackgroundResponse {
        notifications,
        error: None,
        error_code: None,
    })
    .map_err(|_| BackgroundFailure::new("runtime_error", "response serialization failed"))
}

#[derive(Debug, PartialEq, Eq)]
struct BackgroundFailure {
    code: &'static str,
    message: String,
}

impl BackgroundFailure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

fn background_runtime_error(error: lumo_core::LumoError) -> BackgroundFailure {
    let code = match error {
        lumo_core::LumoError::AuthenticationFailed => "credential_revoked",
        lumo_core::LumoError::TrackingDisabled => "tracking_disabled",
        lumo_core::LumoError::Unauthorized => "authorization_failed",
        lumo_core::LumoError::RemoteUnavailable
        | lumo_core::LumoError::RevisionConflict
        | lumo_core::LumoError::RateLimited
        | lumo_core::LumoError::NotFound(_) => "transient_remote",
        _ => "runtime_error",
    };
    BackgroundFailure::new(code, "background synchronization failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir() -> String {
        std::env::temp_dir()
            .join("lumo-background-test")
            .display()
            .to_string()
    }

    #[test]
    fn rejects_unknown_background_role_without_contacting_storage() {
        let payload = serde_json::json!({
            "role": "debug",
            "timestampMs": 1,
            "dataDir": test_data_dir(),
            "batteryPercent": 80,
            "location": null
        })
        .to_string();
        assert_eq!(
            process_tick(payload).expect_err("unknown role").code,
            "credential_invalid"
        );
    }

    #[test]
    fn rejects_relative_background_storage_paths() {
        let payload = serde_json::json!({
            "role": "controlled",
            "timestampMs": 1,
            "dataDir": ".lumo-test",
            "batteryPercent": 80,
            "location": null
        })
        .to_string();
        assert_eq!(
            process_tick(payload).expect_err("relative path").code,
            "credential_invalid"
        );
    }

    #[test]
    fn remote_failures_only_classify_auth_as_terminal() {
        assert_eq!(
            background_runtime_error(lumo_core::LumoError::AuthenticationFailed).code,
            "credential_revoked"
        );
        assert_eq!(
            background_runtime_error(lumo_core::LumoError::RemoteUnavailable).code,
            "transient_remote"
        );
        assert_eq!(
            background_runtime_error(lumo_core::LumoError::TrackingDisabled).code,
            "tracking_disabled"
        );
        assert_eq!(
            background_runtime_error(lumo_core::LumoError::Unauthorized).code,
            "authorization_failed"
        );
    }
}
