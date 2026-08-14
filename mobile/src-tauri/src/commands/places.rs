use lumo_core::{
    application::CreatePlaceInput,
    domain::{AppSnapshot, Place},
};
use tauri::State;

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn place_create(
    state: State<'_, BackendState>,
    input: CreatePlaceInput,
) -> CommandResult<Place> {
    let backend = state.0.clone();
    state.1.require_controller()?;
    run_blocking(move || backend.create_place(input).map_err(Into::into)).await
}

#[tauri::command]
pub async fn place_update(
    state: State<'_, BackendState>,
    id: String,
    input: CreatePlaceInput,
) -> CommandResult<Place> {
    let backend = state.0.clone();
    state.1.require_controller()?;
    run_blocking(move || backend.update_place(&id, input).map_err(Into::into)).await
}

#[tauri::command]
pub async fn place_delete(
    state: State<'_, BackendState>,
    id: String,
    pin: String,
) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    state.1.require_controller()?;
    run_blocking(move || backend.delete_place(&id, &pin).map_err(Into::into)).await
}
