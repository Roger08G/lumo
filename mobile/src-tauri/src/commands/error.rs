use lumo_core::LumoError;
use serde::Serialize;

pub type CommandResult<T> = Result<T, CommandError>;

pub async fn run_blocking<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| CommandError {
            code: "runtime_error",
            message: format!("backend task failed: {error}"),
        })?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl From<LumoError> for CommandError {
    fn from(error: LumoError) -> Self {
        let code = match error {
            LumoError::InvalidInput(_) => "invalid_input",
            LumoError::Unauthorized => "unauthorized",
            LumoError::RateLimited => "rate_limited",
            LumoError::GroupNotInitialized => "group_not_initialized",
            LumoError::NotFound(_) => "not_found",
            LumoError::InvalidInvitation => "invalid_invitation",
            LumoError::AuthenticationFailed => "authentication_failed",
            LumoError::ExpiredMessage => "expired_message",
            LumoError::ReplayDetected => "replay_detected",
            LumoError::RevisionConflict => "revision_conflict",
            LumoError::Storage(_) => "storage_error",
            LumoError::Configuration(_) => "configuration_error",
            LumoError::Serialization(_) => "serialization_error",
            LumoError::RemoteUnavailable => "remote_unavailable",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}
