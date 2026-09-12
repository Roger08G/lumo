use std::sync::atomic::Ordering;

use lumo_core::domain::{AppSnapshot, RuntimeProfile, RuntimeState};
use lumo_core::LumoError;
use tauri::{AppHandle, State};

use crate::state::BackendState;

use super::error::{run_blocking, CommandError, CommandResult};

#[tauri::command]
pub async fn app_bootstrap(
    app: AppHandle,
    state: State<'_, BackendState>,
    profile: RuntimeProfile,
) -> CommandResult<AppSnapshot> {
    if state.mode == lumo_runtime::RuntimeMode::Remote && profile == RuntimeProfile::Debug {
        return Err(LumoError::Unauthorized.into());
    }
    let backend = state.backend.clone();
    let binding = state.binding.clone();
    let mode = state.mode;
    let repository = state.repository.clone();
    let vault = state.vault.clone();
    let onboarding = state.onboarding.clone();
    let lifecycle = state.lifecycle.clone();
    let restore_failed = state.restore_failed.clone();
    run_blocking(move || {
        let _guard = lifecycle
            .lock()
            .map_err(|_| LumoError::Storage("group lifecycle lock poisoned".to_owned()))?;
        if crate::restore_remote_session(&app, &repository, &binding, &vault, &onboarding).is_err() {
            restore_failed.store(true, Ordering::Release);
            return Err(CommandError {
                code: "session_recovery_required",
                message: "No se ha podido abrir la configuración guardada. Reintenta antes de vincular de nuevo.".to_owned(),
            });
        }
        restore_failed.store(false, Ordering::Release);
        let Some(bound_profile) = binding.bootstrap_profile(profile)? else {
            return Ok(RuntimeState::default().snapshot(profile));
        };
        if mode == lumo_runtime::RuntimeMode::Remote {
            let credential = repository.remote()?.credential()?;
            let role_matches = credential.as_ref().is_some_and(|credential| {
                matches!(
                    (bound_profile, credential.role()),
                    (
                        RuntimeProfile::Controller | RuntimeProfile::Debug,
                        lumo_runtime::DeviceRole::Controller
                    ) | (
                        RuntimeProfile::Controlled,
                        lumo_runtime::DeviceRole::Controlled
                    )
                )
            });
            if !role_matches {
                return Err(LumoError::AuthenticationFailed.into());
            }
        }

        match backend.snapshot(bound_profile) {
            Ok(snapshot)
                if mode == lumo_runtime::RuntimeMode::Remote && snapshot.session.is_none() =>
            {
                Err(LumoError::GroupNotInitialized.into())
            }
            Ok(snapshot) => Ok(snapshot),
            Err(error)
                if mode == lumo_runtime::RuntimeMode::Remote
                    && is_terminal_session_error(&error) =>
            {
                clear_remote_session(&app, &repository, &binding, &vault)?;
                Ok(RuntimeState::default().snapshot(profile))
            }
            Err(error) => Err(error.into()),
        }
    })
    .await
}

/// Explicit recovery after the user confirms that a new controller-issued QR is required.
/// This removes only local state; server membership and other devices are untouched.
#[tauri::command]
pub async fn app_reset_local_session(
    app: AppHandle,
    state: State<'_, BackendState>,
) -> CommandResult<()> {
    if state.mode != lumo_runtime::RuntimeMode::Remote {
        return Err(LumoError::Unauthorized.into());
    }
    let binding = state.binding.clone();
    let repository = state.repository.clone();
    let vault = state.vault.clone();
    let onboarding = state.onboarding.clone();
    let lifecycle = state.lifecycle.clone();
    let restore_failed = state.restore_failed.clone();
    run_blocking(move || {
        let _guard = lifecycle
            .lock()
            .map_err(|_| LumoError::Storage("group lifecycle lock poisoned".to_owned()))?;
        if !restore_failed.load(Ordering::Acquire) {
            return Err(LumoError::Unauthorized.into());
        }
        vault.clear(&app)?;
        repository.clear_credential()?;
        binding.clear()?;
        onboarding.confirm_onboarding()?;
        restore_failed.store(false, Ordering::Release);
        Ok(())
    })
    .await
}

fn clear_remote_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    repository: &lumo_runtime::ConfiguredRepository,
    binding: &crate::device::DeviceBinding,
    vault: &crate::device::DeviceCredentialVault,
) -> lumo_core::LumoResult<()> {
    vault.clear(app)?;
    repository.clear_credential()?;
    binding.clear()
}

fn is_terminal_session_error(error: &LumoError) -> bool {
    matches!(error, LumoError::CredentialRejected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoked_or_invalid_credentials_are_terminal_but_offline_is_not() {
        assert!(is_terminal_session_error(&LumoError::CredentialRejected));
        assert!(!is_terminal_session_error(&LumoError::AuthenticationFailed));
        assert!(!is_terminal_session_error(&LumoError::Unauthorized));
        assert!(!is_terminal_session_error(&LumoError::TrackingDisabled));
        assert!(!is_terminal_session_error(&LumoError::NotFound(
            "group".into()
        )));
        assert!(!is_terminal_session_error(&LumoError::RemoteUnavailable));
        assert!(!is_terminal_session_error(&LumoError::RateLimited));
    }
}
