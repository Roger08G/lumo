use lumo_core::domain::{AppSnapshot, RuntimeProfile, RuntimeState};
use tauri::State;

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn app_bootstrap(
    state: State<'_, BackendState>,
    profile: RuntimeProfile,
) -> CommandResult<AppSnapshot> {
    let Some(bound_profile) = state.1.bootstrap_profile(profile)? else {
        return Ok(RuntimeState::default().snapshot(profile));
    };
    let backend = state.0.clone();
    run_blocking(move || backend.snapshot(bound_profile).map_err(Into::into)).await
}
