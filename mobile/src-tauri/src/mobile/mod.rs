use tauri::{AppHandle, Runtime};
use tauri_plugin_lumo_mobile::{LumoMobileExt, MobileStatus};

use crate::commands::error::{CommandError, CommandResult};

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

fn validate_role(role: &str) -> CommandResult<()> {
    if matches!(role, "controlled" | "controller") {
        Ok(())
    } else {
        Err(CommandError {
            code: "invalid_input",
            message: "unsupported mobile role".to_owned(),
        })
    }
}

#[tauri::command]
pub fn mobile_get_status(app: AppHandle) -> CommandResult<MobileStatus> {
    app.lumo_mobile().get_status().map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_request_permissions(app: AppHandle, role: String) -> CommandResult<MobileStatus> {
    validate_role(&role)?;
    app.lumo_mobile()
        .request_permissions(&role)
        .map_err(bridge_error)
}

#[tauri::command]
pub fn mobile_configure_tracking(
    app: AppHandle,
    role: String,
    enabled: bool,
    interval_seconds: Option<u64>,
) -> CommandResult<MobileStatus> {
    validate_role(&role)?;
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
pub fn mobile_open_battery_settings(app: AppHandle) -> CommandResult<()> {
    app.lumo_mobile()
        .open_battery_settings()
        .map_err(bridge_error)
}
