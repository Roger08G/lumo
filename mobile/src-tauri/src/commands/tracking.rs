use lumo_core::{
    application::{ReportLocationInput, SetTrackingInput},
    domain::AppSnapshot,
};
use serde::Serialize;
use tauri::State;

use crate::state::BackendState;

use super::error::{run_blocking, CommandResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandAccepted {
    pub command_id: String,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessedView {
    pub processed: usize,
}

#[tauri::command]
pub async fn tracker_set_tracking(
    state: State<'_, BackendState>,
    input: SetTrackingInput,
) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    run_blocking(move || backend.set_tracking(input).map_err(Into::into)).await
}

#[tauri::command]
pub async fn tracker_report_location(
    state: State<'_, BackendState>,
    input: ReportLocationInput,
) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    run_blocking(move || backend.report_location(input).map_err(Into::into)).await
}

#[tauri::command]
pub async fn tracker_process_pending(
    state: State<'_, BackendState>,
) -> CommandResult<ProcessedView> {
    let backend = state.0.clone();
    run_blocking(move || {
        Ok(ProcessedView {
            processed: backend.process_pending()?,
        })
    })
    .await
}

#[tauri::command]
pub async fn tracker_send_help(state: State<'_, BackendState>) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    run_blocking(move || backend.send_help().map_err(Into::into)).await
}

#[tauri::command]
pub async fn controller_request_location(
    state: State<'_, BackendState>,
) -> CommandResult<CommandAccepted> {
    let backend = state.0.clone();
    run_blocking(move || {
        Ok(CommandAccepted {
            command_id: backend.request_location()?,
            status: "queued",
        })
    })
    .await
}

#[tauri::command]
pub async fn events_mark_read(state: State<'_, BackendState>) -> CommandResult<AppSnapshot> {
    let backend = state.0.clone();
    run_blocking(move || backend.mark_events_read().map_err(Into::into)).await
}
