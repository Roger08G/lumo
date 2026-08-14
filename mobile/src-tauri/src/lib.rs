mod commands;
mod device;
mod mobile;
mod state;

use lumo_runtime::{ConfiguredRepository, LocalBackend, RuntimeConfig, SystemClock};
use tauri::Manager;

use device::DeviceBinding;
use state::BackendState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(mobile::init())
        .setup(|app| {
            #[cfg(mobile)]
            {
                app.handle().plugin(tauri_plugin_barcode_scanner::init())?;
                app.handle().plugin(tauri_plugin_geolocation::init())?;
            }
            let data_dir = app.path().app_data_dir()?.join("runtime");
            let config = RuntimeConfig::from_values(
                option_env!("LUMO_RUNTIME_MODE"),
                Some(data_dir.clone()),
                option_env!("LUMO_API_URL"),
                option_env!("LUMO_API_PASSWORD"),
            )?;
            let repository = ConfiguredRepository::open(&config)?;
            let binding = DeviceBinding::open(data_dir.join("device-binding.json"))?;
            app.manage(BackendState(
                LocalBackend::new(repository, SystemClock),
                binding,
                config.mode,
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_bootstrap,
            commands::groups::group_create,
            commands::groups::group_verify_pin,
            commands::groups::group_create_invitation,
            commands::groups::group_consume_invitation,
            commands::groups::group_leave,
            commands::places::place_create,
            commands::places::place_update,
            commands::places::place_delete,
            commands::tracking::tracker_set_tracking,
            commands::tracking::tracker_report_location,
            commands::tracking::tracker_process_pending,
            commands::tracking::tracker_send_help,
            commands::tracking::controller_request_location,
            commands::tracking::events_mark_read,
            commands::debug::debug_apply_scenario,
            mobile::mobile_get_status,
            mobile::mobile_request_permissions,
            mobile::mobile_configure_tracking,
            mobile::mobile_open_phone_dialer,
            mobile::mobile_show_notification,
            mobile::mobile_open_battery_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lumo");
}
