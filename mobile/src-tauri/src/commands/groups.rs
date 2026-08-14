use lumo_core::{
    application::{CreateGroupInput, InvitationView},
    domain::{AppSnapshot, RuntimeProfile},
};
use lumo_protocol::DeviceSummary;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedView {
    verified: bool,
}

#[tauri::command]
pub async fn group_create(
    app: AppHandle,
    state: State<'_, BackendState>,
    input: CreateGroupInput,
) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    let binding = state.1.clone();
    let mode = state.2;
    let repository = state.3.clone();
    let vault = state.4.clone();
    let onboarding = state.5.clone();
    let lifecycle = state.6.clone();
    run_blocking(move || {
        let _guard = lifecycle.lock().map_err(|_| {
            lumo_core::LumoError::Storage("group lifecycle lock poisoned".to_owned())
        })?;
        if binding.profile()?.is_some() {
            return Err(lumo_core::LumoError::InvalidInput(
                "this device is already paired".to_owned(),
            )
            .into());
        }
        if mode == lumo_runtime::RuntimeMode::Local {
            let snapshot = backend.create_group(input, RuntimeProfile::Controller)?;
            binding.bind(RuntimeProfile::Controller)?;
            return Ok(snapshot);
        }

        let pin = input.pin.clone();
        let request_id = onboarding.begin_create()?;
        let credential = repository.provision_group(&request_id, &pin, &input.supervisor_name)?;
        let snapshot = match create_or_recover_remote_group(&backend, &repository, &input) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if repository.delete_remote_group(&pin).is_ok() {
                    let _ = vault.clear(&app);
                    let _ = onboarding.confirm_onboarding();
                }
                return Err(error.into());
            }
        };
        if let Err(error) = vault.store(&app, &credential) {
            if repository.delete_remote_group(&pin).is_ok() {
                let _ = vault.clear(&app);
                let _ = onboarding.confirm_onboarding();
            }
            return Err(error.into());
        }
        binding.bind(RuntimeProfile::Controller)?;
        let _ = onboarding.confirm_onboarding();
        Ok(snapshot)
    })
    .await
}

#[tauri::command]
pub async fn group_verify_pin(
    state: State<'_, BackendState>,
    pin: String,
) -> CommandResult<VerifiedView> {
    let backend = state.0.clone();
    let mode = state.2;
    let repository = state.3.clone();
    state.1.require_bound()?;
    run_blocking(move || {
        if mode == lumo_runtime::RuntimeMode::Remote {
            repository.verify_remote_pin(&pin)?;
        } else {
            backend.verify_pin(&pin)?;
        }
        Ok(VerifiedView { verified: true })
    })
    .await
}

#[tauri::command]
pub async fn group_create_invitation(
    state: State<'_, BackendState>,
    pin: String,
) -> CommandResult<InvitationView> {
    let backend = state.0.clone();
    let mode = state.2;
    let repository = state.3.clone();
    state.1.require_controller()?;
    run_blocking(move || {
        if mode == lumo_runtime::RuntimeMode::Local {
            return backend.create_invitation(&pin).map_err(Into::into);
        }
        let invitation = repository.create_remote_invitation(&pin)?;
        let session = backend
            .snapshot(RuntimeProfile::Controller)?
            .session
            .ok_or(lumo_core::LumoError::GroupNotInitialized)?;
        Ok(InvitationView {
            invitation_id: invitation.invitation_id,
            token: invitation.token,
            group_name: session.group_name,
            group_code: session.group_code,
            expires_at_ms: invitation.expires_at_ms,
        })
    })
    .await
}

#[tauri::command]
pub async fn group_consume_invitation(
    app: AppHandle,
    state: State<'_, BackendState>,
    invitation_id: String,
    token: String,
    pin: String,
) -> CommandResult<VerifiedView> {
    let backend = state.0.clone();
    let binding = state.1.clone();
    let mode = state.2;
    let repository = state.3.clone();
    let vault = state.4.clone();
    let onboarding = state.5.clone();
    let lifecycle = state.6.clone();
    run_blocking(move || {
        let _guard = lifecycle.lock().map_err(|_| {
            lumo_core::LumoError::Storage("group lifecycle lock poisoned".to_owned())
        })?;
        if binding.profile()?.is_some() {
            return Err(lumo_core::LumoError::InvalidInput(
                "this device is already paired".to_owned(),
            )
            .into());
        }
        if mode == lumo_runtime::RuntimeMode::Local {
            backend.consume_invitation(&token, &pin)?;
            binding.bind(RuntimeProfile::Controlled)?;
            return Ok(VerifiedView { verified: true });
        }
        let request_id = onboarding.begin_join(&invitation_id)?;
        let credential = repository.consume_invitation(
            &request_id,
            &invitation_id,
            &token,
            &pin,
            "Dispositivo controlado",
        )?;
        if let Err(error) = backend.snapshot(RuntimeProfile::Controlled) {
            if repository.leave_remote_group(&pin).is_ok() {
                let _ = vault.clear(&app);
                let _ = onboarding.confirm_onboarding();
            }
            return Err(error.into());
        }
        if let Err(error) = vault.store(&app, &credential) {
            if repository.leave_remote_group(&pin).is_ok() {
                let _ = vault.clear(&app);
                let _ = onboarding.confirm_onboarding();
            }
            return Err(error.into());
        }
        binding.bind(RuntimeProfile::Controlled)?;
        let _ = onboarding.confirm_onboarding();
        Ok(VerifiedView { verified: true })
    })
    .await
}

#[tauri::command]
pub async fn group_leave(
    app: AppHandle,
    state: State<'_, BackendState>,
    pin: String,
) -> CommandResult<VerifiedView> {
    let backend = state.0.clone();
    let binding = state.1.clone();
    let mode = state.2;
    let repository = state.3.clone();
    let vault = state.4.clone();
    let onboarding = state.5.clone();
    let lifecycle = state.6.clone();
    run_blocking(move || {
        let _guard = lifecycle.lock().map_err(|_| {
            lumo_core::LumoError::Storage("group lifecycle lock poisoned".to_owned())
        })?;
        let profile = binding.require_bound()?;
        if mode == lumo_runtime::RuntimeMode::Local {
            backend.leave_group(&pin)?;
        } else {
            match profile {
                RuntimeProfile::Controller => repository.delete_remote_group(&pin)?,
                RuntimeProfile::Controlled => repository.leave_remote_group(&pin)?,
                RuntimeProfile::Debug => return Err(lumo_core::LumoError::Unauthorized.into()),
            }
            let marker_result = onboarding.begin_leave();
            let vault_result = vault.clear(&app);
            if !leave_cleanup_is_durable(marker_result.is_ok(), vault_result.is_ok()) {
                return Err(vault_result
                    .err()
                    .unwrap_or_else(|| {
                        lumo_core::LumoError::Storage(
                            "leave cleanup could not be made durable".to_owned(),
                        )
                    })
                    .into());
            }
            binding.clear()?;
            if vault_result.is_ok() {
                onboarding.confirm_onboarding()?;
            }
            return Ok(VerifiedView { verified: true });
        }
        binding.clear()?;
        Ok(VerifiedView { verified: true })
    })
    .await
}

fn leave_cleanup_is_durable(marker_persisted: bool, vault_cleared: bool) -> bool {
    marker_persisted || vault_cleared
}

fn create_or_recover_remote_group(
    backend: &lumo_runtime::LocalBackend<lumo_runtime::ConfiguredRepository>,
    repository: &lumo_runtime::ConfiguredRepository,
    input: &CreateGroupInput,
) -> lumo_core::LumoResult<AppSnapshot> {
    let existing = backend.snapshot(RuntimeProfile::Controller)?;
    let Some(session) = existing.session.as_ref() else {
        return backend.create_group(input.clone(), RuntimeProfile::Controller);
    };
    let matches_input = session.group_name == input.name.trim()
        && session.supervisor_name == input.supervisor_name.trim()
        && session.supervisor_phone == input.supervisor_phone.trim()
        && session.tracked_person_name == input.tracked_person_name.trim()
        && session.tracked_person_phone == input.tracked_person_phone.trim();
    if !matches_input {
        return Err(lumo_core::LumoError::InvalidInput(
            "the pending group does not match this onboarding request".to_owned(),
        ));
    }
    repository.verify_remote_pin(&input.pin)?;
    Ok(existing)
}

#[tauri::command]
pub async fn group_list_devices(
    state: State<'_, BackendState>,
) -> CommandResult<Vec<DeviceSummary>> {
    state.1.require_controller()?;
    let repository = state.3.clone();
    run_blocking(move || repository.list_remote_devices().map_err(Into::into)).await
}

#[tauri::command]
pub async fn group_revoke_device(
    state: State<'_, BackendState>,
    device_id: String,
    pin: String,
) -> CommandResult<VerifiedView> {
    state.1.require_controller()?;
    let repository = state.3.clone();
    run_blocking(move || {
        repository.revoke_remote_device(&device_id, &pin)?;
        Ok(VerifiedView { verified: true })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::leave_cleanup_is_durable;

    #[test]
    fn remote_leave_clears_binding_only_after_vault_or_tombstone_is_durable() {
        assert!(leave_cleanup_is_durable(false, true));
        assert!(leave_cleanup_is_durable(true, false));
        assert!(leave_cleanup_is_durable(true, true));
        assert!(!leave_cleanup_is_durable(false, false));
    }
}
