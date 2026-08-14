use lumo_core::domain::AppSnapshot;
use lumo_runtime::simulation::SimulationScenario;
use tauri::State;

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn debug_apply_scenario(
    state: State<'_, BackendState>,
    scenario: SimulationScenario,
) -> CommandResult<AppSnapshot> {
    if state.2 == lumo_runtime::RuntimeMode::Remote {
        return Err(lumo_core::LumoError::Unauthorized.into());
    }
    let backend = state.0.clone();
    state.1.require_controller()?;
    run_blocking(move || backend.debug_scenario(scenario).map_err(Into::into)).await
}
