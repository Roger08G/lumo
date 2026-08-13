use lumo_core::{
    application::{CreateGroupInput, InvitationView},
    domain::{AppSnapshot, RuntimeProfile},
};
use serde::Serialize;
use tauri::State;

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedView {
    verified: bool,
}

#[tauri::command]
pub async fn group_create(
    state: State<'_, BackendState>,
    input: CreateGroupInput,
) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    let binding = state.1.clone();
    run_blocking(move || {
        if binding.profile()?.is_some() {
            return Err(lumo_core::LumoError::InvalidInput(
                "this device is already paired".to_owned(),
            )
            .into());
        }
        backend
            .create_group(input, RuntimeProfile::Controller)
            .and_then(|snapshot| {
                binding.bind(RuntimeProfile::Controller)?;
                Ok(snapshot)
            })
            .map_err(Into::into)
    })
    .await
}

#[tauri::command]
pub async fn group_verify_pin(
    state: State<'_, BackendState>,
    pin: String,
) -> CommandResult<VerifiedView> {
    let backend = state.0.clone();
    run_blocking(move || {
        backend.verify_pin(&pin)?;
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
    run_blocking(move || backend.create_invitation(&pin).map_err(Into::into)).await
}

#[tauri::command]
pub async fn group_consume_invitation(
    state: State<'_, BackendState>,
    token: String,
    pin: String,
) -> CommandResult<VerifiedView> {
    let backend = state.0.clone();
    let binding = state.1.clone();
    run_blocking(move || {
        if binding.profile()?.is_some() {
            return Err(lumo_core::LumoError::InvalidInput(
                "this device is already paired".to_owned(),
            )
            .into());
        }
        backend.consume_invitation(&token, &pin)?;
        binding.bind(RuntimeProfile::Controlled)?;
        Ok(VerifiedView { verified: true })
    })
    .await
}

#[tauri::command]
pub async fn group_leave(
    state: State<'_, BackendState>,
    pin: String,
) -> CommandResult<VerifiedView> {
    let backend = state.0.clone();
    let binding = state.1.clone();
    let mode = state.2;
    run_blocking(move || {
        if mode == lumo_runtime::RuntimeMode::Local {
            backend.leave_group(&pin)?;
        } else {
            backend.verify_pin(&pin)?;
        }
        binding.clear()?;
        Ok(VerifiedView { verified: true })
    })
    .await
}
