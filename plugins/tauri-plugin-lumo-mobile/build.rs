const COMMANDS: &[&str] = &[
    "get_status",
    "request_permissions",
    "configure_tracking",
    "open_phone_dialer",
    "show_notification",
    "open_battery_settings",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
