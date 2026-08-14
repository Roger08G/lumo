use lumo_core::domain::{AppSnapshot, RuntimeProfile, RuntimeState};
use lumo_core::LumoError;
use tauri::{AppHandle, State};

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn app_bootstrap(
    app: AppHandle,
    state: State<'_, BackendState>,
    profile: RuntimeProfile,
) -> CommandResult<AppSnapshot> {
    if state.2 == lumo_runtime::RuntimeMode::Remote && profile == RuntimeProfile::Debug {
        return Err(LumoError::Unauthorized.into());
    }
    let Some(bound_profile) = state.1.bootstrap_profile(profile)? else {
        return Ok(RuntimeState::default().snapshot(profile));
    };
    let backend = state.0.clone();
    let binding = state.1.clone();
    let mode = state.2;
    let repository = state.3.clone();
    let vault = state.4.clone();
    run_blocking(move || {
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
                clear_remote_session(&app, &repository, &binding, &vault);
                return Ok(RuntimeState::default().snapshot(profile));
            }
        }

        match backend.snapshot(bound_profile) {
            Ok(snapshot) => Ok(snapshot),
            Err(error)
                if mode == lumo_runtime::RuntimeMode::Remote
                    && is_terminal_session_error(&error) =>
            {
                clear_remote_session(&app, &repository, &binding, &vault);
                Ok(RuntimeState::default().snapshot(profile))
            }
            Err(error) => Err(error.into()),
        }
    })
    .await
}

fn clear_remote_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    repository: &lumo_runtime::ConfiguredRepository,
    binding: &crate::device::DeviceBinding,
    vault: &crate::device::DeviceCredentialVault,
) {
    let _ = repository.clear_credential();
    let _ = vault.clear(app);
    let _ = binding.clear();
}

fn is_terminal_session_error(error: &LumoError) -> bool {
    matches!(error, LumoError::AuthenticationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoked_or_invalid_credentials_are_terminal_but_offline_is_not() {
        assert!(is_terminal_session_error(&LumoError::AuthenticationFailed));
        assert!(!is_terminal_session_error(&LumoError::Unauthorized));
        assert!(!is_terminal_session_error(&LumoError::TrackingDisabled));
        assert!(!is_terminal_session_error(&LumoError::NotFound(
            "group".into()
        )));
        assert!(!is_terminal_session_error(&LumoError::RemoteUnavailable));
        assert!(!is_terminal_session_error(&LumoError::RateLimited));
    }
}
