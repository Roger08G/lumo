mod commands;
mod device;
mod mobile;
mod state;

use std::sync::{Arc, Mutex};

use lumo_core::domain::RuntimeProfile;
use lumo_runtime::{
    ConfiguredRepository, DeviceCredential, DeviceRole, LocalBackend, RuntimeConfig, RuntimeMode,
    SystemClock,
};
use tauri::Manager;

use device::{DeviceBinding, DeviceCredentialVault, PendingOnboardingStore};
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
            let config = RuntimeConfig::from_mobile_values(
                option_env!("LUMO_RUNTIME_MODE"),
                Some(data_dir.clone()),
                option_env!("LUMO_API_URL"),
                !cfg!(debug_assertions),
            )?;
            let repository = ConfiguredRepository::open(&config)?;
            let binding = DeviceBinding::open(data_dir.join("device-binding.json"))?;
            let vault = DeviceCredentialVault::new(data_dir.join("device-credential.json"));
            let onboarding = PendingOnboardingStore::new(data_dir.join("pending-onboarding.json"));
            if config.mode == RuntimeMode::Remote {
                if onboarding.is_leave_pending()? {
                    let _ = repository.clear_credential();
                    binding.clear()?;
                    if vault.clear(app.handle()).is_ok() {
                        onboarding.confirm_onboarding()?;
                    }
                } else {
                    let api_origin = config.api_url.as_deref().ok_or_else(|| {
                        lumo_core::LumoError::Configuration("LUMO_API_URL is required".to_owned())
                    })?;
                    let credential = match vault.load(app.handle(), api_origin) {
                        Ok(credential) => credential,
                        Err(_) => {
                            let _ = repository.clear_credential();
                            let _ = vault.clear(app.handle());
                            binding.clear()?;
                            None
                        }
                    };
                    reconcile_startup_credential(
                        app.handle(),
                        &repository,
                        &binding,
                        &vault,
                        &onboarding,
                        credential,
                    )?;
                }
            }
            app.manage(BackendState(
                LocalBackend::new(repository.clone(), SystemClock),
                binding,
                config.mode,
                repository,
                vault,
                onboarding,
                Arc::new(Mutex::new(())),
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
            commands::groups::group_list_devices,
            commands::groups::group_revoke_device,
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
            mobile::mobile_reverse_geocode,
            mobile::mobile_show_notification,
            mobile::mobile_start_emergency_alarm,
            mobile::mobile_get_pending_alarm,
            mobile::mobile_stop_emergency_alarm,
            mobile::mobile_open_battery_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lumo");
}

fn reconcile_startup_credential<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    repository: &ConfiguredRepository,
    binding: &DeviceBinding,
    vault: &DeviceCredentialVault,
    onboarding: &PendingOnboardingStore,
    credential: Option<DeviceCredential>,
) -> lumo_core::LumoResult<()> {
    let bound = binding.profile()?;
    let action = startup_reconciliation(bound, credential.as_ref().map(DeviceCredential::role));
    match action {
        StartupReconciliation::Unbound => Ok(()),
        StartupReconciliation::Clear => {
            let _ = repository.clear_credential();
            let _ = vault.clear(app);
            binding.clear()
        }
        StartupReconciliation::RecoverBinding(profile) => {
            repository.install_credential(
                credential.ok_or(lumo_core::LumoError::AuthenticationFailed)?,
            )?;
            binding.bind(profile)?;
            onboarding.confirm_onboarding()
        }
        StartupReconciliation::Install => {
            repository.install_credential(
                credential.ok_or(lumo_core::LumoError::AuthenticationFailed)?,
            )?;
            onboarding.confirm_onboarding()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupReconciliation {
    Unbound,
    RecoverBinding(RuntimeProfile),
    Install,
    Clear,
}

fn startup_reconciliation(
    bound: Option<RuntimeProfile>,
    role: Option<DeviceRole>,
) -> StartupReconciliation {
    match (bound, role) {
        (None, None) => StartupReconciliation::Unbound,
        (Some(_), None) => StartupReconciliation::Clear,
        (None, Some(role)) => StartupReconciliation::RecoverBinding(profile_for_role(role)),
        (Some(bound), Some(role)) if bound == profile_for_role(role) => {
            StartupReconciliation::Install
        }
        (Some(_), Some(_)) => StartupReconciliation::Clear,
    }
}

fn profile_for_role(role: DeviceRole) -> RuntimeProfile {
    match role {
        DeviceRole::Controller => RuntimeProfile::Controller,
        DeviceRole::Controlled => RuntimeProfile::Controlled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_credential_clears_a_stale_binding() {
        assert_eq!(
            startup_reconciliation(Some(RuntimeProfile::Controller), None),
            StartupReconciliation::Clear
        );
    }

    #[test]
    fn vault_authority_recovers_only_its_bound_role() {
        assert_eq!(
            startup_reconciliation(None, Some(DeviceRole::Controlled)),
            StartupReconciliation::RecoverBinding(RuntimeProfile::Controlled)
        );
        assert_eq!(
            startup_reconciliation(
                Some(RuntimeProfile::Controller),
                Some(DeviceRole::Controlled)
            ),
            StartupReconciliation::Clear
        );
    }
}
