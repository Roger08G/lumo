use std::{panic, path::PathBuf, ptr};

use jni::{
    objects::{JClass, JString},
    sys::jstring,
    JNIEnv,
};
use lumo_core::{
    application::ReportLocationInput,
    domain::{Connectivity, EventKind, RuntimeProfile},
};
use lumo_runtime::{ConfiguredRepository, LocalBackend, RuntimeConfig, SystemClock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundTick {
    role: String,
    timestamp_ms: i64,
    data_dir: PathBuf,
    battery_percent: u8,
    location: Option<BackgroundLocation>,
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
}

#[derive(Debug, Serialize)]
struct BackgroundNotification {
    id: String,
    title: String,
    body: String,
    urgent: bool,
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
            .map_err(|error| error.to_string())?;
        process_tick(payload)
    }))
    .unwrap_or_else(|_| Err("background runtime failed safely".to_owned()))
    .unwrap_or_else(|error| {
        serde_json::to_string(&BackgroundResponse {
            notifications: Vec::new(),
            error: Some(error),
        })
        .unwrap_or_else(|_| "{\"notifications\":[],\"error\":\"serialization failure\"}".to_owned())
    });

    environment
        .new_string(response)
        .map(JString::into_raw)
        .unwrap_or(ptr::null_mut())
}

fn process_tick(payload: String) -> Result<String, String> {
    let tick: BackgroundTick = serde_json::from_str(&payload).map_err(|error| error.to_string())?;
    let profile = match tick.role.as_str() {
        "controlled" => RuntimeProfile::Controlled,
        "controller" => RuntimeProfile::Controller,
        _ => return Err("unsupported background role".to_owned()),
    };
    if !tick.data_dir.is_absolute() {
        return Err("background data directory must be absolute".to_owned());
    }
    let config = RuntimeConfig::from_values(
        option_env!("LUMO_RUNTIME_MODE"),
        Some(tick.data_dir.join("runtime")),
        option_env!("LUMO_API_URL"),
        option_env!("LUMO_API_PASSWORD"),
    )
    .map_err(|error| error.to_string())?;
    let repository = ConfiguredRepository::open(&config).map_err(|error| error.to_string())?;
    let backend = LocalBackend::new(repository, SystemClock);

    let mut snapshot = if profile == RuntimeProfile::Controlled {
        if let Some(location) = tick.location {
            backend
                .report_location(ReportLocationInput {
                    latitude: location.latitude,
                    longitude: location.longitude,
                    accuracy_m: location.accuracy,
                    battery_percent: tick.battery_percent,
                    captured_at_ms: Some(location.timestamp_ms),
                })
                .map_err(|error| error.to_string())?
        } else {
            backend
                .snapshot(profile)
                .map_err(|error| error.to_string())?
        }
    } else {
        backend
            .snapshot(profile)
            .map_err(|error| error.to_string())?
    };

    if profile == RuntimeProfile::Controller
        && snapshot.controlled.connectivity == Connectivity::Online
        && snapshot
            .controlled
            .last_seen_at_ms
            .is_some_and(|last_seen| tick.timestamp_ms.saturating_sub(last_seen) > 120_000)
    {
        snapshot = backend
            .set_connectivity(Connectivity::Offline)
            .map_err(|error| error.to_string())?;
    }

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
            .map(|event| BackgroundNotification {
                id: event.id,
                title: event.title,
                body: event.detail,
                urgent: event.kind == EventKind::Help,
            })
            .collect()
    } else {
        Vec::new()
    };

    serde_json::to_string(&BackgroundResponse {
        notifications,
        error: None,
    })
    .map_err(|error| error.to_string())
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
            process_tick(payload),
            Err("unsupported background role".to_owned())
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
            process_tick(payload),
            Err("background data directory must be absolute".to_owned())
        );
    }
}
