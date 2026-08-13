use lumo_core::domain::{AppSnapshot, ControlledDevice, RuntimeProfile};
use tauri::State;

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn app_bootstrap(
    state: State<'_, BackendState>,
    profile: RuntimeProfile,
) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    let bound = state.1.profile()?;
    run_blocking(move || {
        let mut snapshot = backend.snapshot(profile)?;
        if bound.is_none() {
            snapshot.session = None;
            snapshot.controlled = ControlledDevice::default();
            snapshot.places.clear();
            snapshot.events.clear();
            snapshot.commands.clear();
        }
        Ok(snapshot)
    })
    .await
}
