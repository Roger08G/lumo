use tauri::{AppHandle, Runtime, State};
use tauri_plugin_lumo_mobile::{LumoMobileExt, MobileStatus, PendingAlarm};

use crate::{
    commands::error::{CommandError, CommandResult},
    device::DeviceBinding,
    state::BackendState,
};

#[cfg(target_os = "android")]
mod background;

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_lumo_mobile::init()
}

fn bridge_error(error: tauri_plugin_lumo_mobile::Error) -> CommandError {
    CommandError {
        code: "mobile_error",
        message: error.to_string(),
    }
}

fn authorize_role(binding: &DeviceBinding, role: &str) -> CommandResult<()> {
    match role {
        "controlled" => binding.require_controlled().map_err(Into::into),
        "controller" => binding.require_controller().map_err(Into::into),
        _ => Err(CommandError {
            code: "invalid_input",
            message: "unsupported mobile role".to_owned(),
        }),
    }
}

#[tauri::command]
pub fn mobile_get_status(app: AppHandle) -> CommandResult<MobileStatus> {
    app.lumo_mobile().get_status().map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_request_permissions(
    app: AppHandle,
    state: State<'_, BackendState>,
    role: String,
) -> CommandResult<MobileStatus> {
    authorize_role(&state.1, &role)?;
    app.lumo_mobile()
        .request_permissions(&role)
        .map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_configure_tracking(
    app: AppHandle,
    state: State<'_, BackendState>,
    role: String,
    enabled: bool,
    interval_seconds: Option<u64>,
) -> CommandResult<MobileStatus> {
    authorize_role(&state.1, &role)?;
    app.lumo_mobile()
        .configure_tracking(&role, enabled, interval_seconds.unwrap_or(30))
        .map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_open_phone_dialer(app: AppHandle, number: String) -> CommandResult<()> {
    let digits = number.chars().filter(char::is_ascii_digit).count();
    if !(7..=15).contains(&digits) {
        return Err(CommandError {
            code: "invalid_input",
            message: "invalid phone number".to_owned(),
        });
    }
    app.lumo_mobile()
        .open_phone_dialer(&number)
        .map_err(bridge_error)
}

#[derive(Debug, serde::Serialize)]
pub struct ReverseGeocodeView {
    address: Option<String>,
}

#[tauri::command]
pub fn mobile_reverse_geocode(
    app: AppHandle,
    latitude: f64,
    longitude: f64,
) -> CommandResult<ReverseGeocodeView> {
    if !latitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !longitude.is_finite()
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err(CommandError {
            code: "invalid_input",
            message: "invalid coordinates".to_owned(),
        });
    }
    app.lumo_mobile()
        .reverse_geocode(latitude, longitude)
        .map(|address| ReverseGeocodeView { address })
        .map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_show_notification(
    app: AppHandle,
    id: Option<String>,
    title: String,
    body: String,
    urgent: Option<bool>,
) -> CommandResult<()> {
    app.lumo_mobile()
        .show_notification(id.as_deref(), &title, &body, urgent.unwrap_or(false))
        .map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_start_emergency_alarm(
    app: AppHandle,
    state: State<'_, BackendState>,
    alarm: PendingAlarm,
) -> CommandResult<()> {
    state.1.require_controller()?;
    if alarm.id.trim().is_empty() || alarm.title.trim().is_empty() || alarm.body.trim().is_empty() {
        return Err(CommandError {
            code: "invalid_input",
            message: "invalid emergency alarm".to_owned(),
        });
    }
    let coordinates_valid = match (alarm.latitude, alarm.longitude) {
        (None, None) => true,
        (Some(latitude), Some(longitude)) => {
            latitude.is_finite()
                && (-90.0..=90.0).contains(&latitude)
                && longitude.is_finite()
                && (-180.0..=180.0).contains(&longitude)
        }
        _ => false,
    };
    if !coordinates_valid
        || alarm
            .address
            .as_ref()
            .is_some_and(|value| value.len() > 240)
    {
        return Err(CommandError {
            code: "invalid_input",
            message: "invalid emergency location".to_owned(),
        });
    }
    app.lumo_mobile()
        .start_emergency_alarm(&alarm)
        .map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_get_pending_alarm(
    app: AppHandle,
    state: State<'_, BackendState>,
) -> CommandResult<Option<PendingAlarm>> {
    state.1.require_controller()?;
    app.lumo_mobile().pending_alarm().map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_stop_emergency_alarm(
    app: AppHandle,
    state: State<'_, BackendState>,
) -> CommandResult<()> {
    state.1.require_controller()?;
    app.lumo_mobile()
        .stop_emergency_alarm()
        .map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_open_battery_settings(app: AppHandle) -> CommandResult<()> {
    app.lumo_mobile()
        .open_battery_settings()
        .map_err(bridge_error)
}

#[cfg(test)]
mod tests {
    use lumo_core::domain::RuntimeProfile;

    use super::*;

    #[test]
    fn rejects_unknown_mobile_role_before_binding_lookup() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");

        let error = authorize_role(&binding, "debug").expect_err("reject unknown role");
        assert_eq!(error.code, "invalid_input");
    }

    #[test]
    fn unbound_device_cannot_request_a_mobile_role() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");

        for role in ["controller", "controlled"] {
            let error = authorize_role(&binding, role).expect_err("reject unbound device");
            assert_eq!(error.code, "unauthorized");
        }
    }

    #[test]
    fn controller_binding_only_authorizes_controller_mobile_role() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");
        binding
            .bind(RuntimeProfile::Controller)
            .expect("bind controller");

        assert!(authorize_role(&binding, "controller").is_ok());
        let error = authorize_role(&binding, "controlled").expect_err("reject controlled role");
        assert_eq!(error.code, "unauthorized");
    }

    #[test]
    fn controlled_binding_only_authorizes_controlled_mobile_role() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");
        binding
            .bind(RuntimeProfile::Controlled)
            .expect("bind controlled");

        assert!(authorize_role(&binding, "controlled").is_ok());
        let error = authorize_role(&binding, "controller").expect_err("reject controller role");
        assert_eq!(error.code, "unauthorized");
    }
}
