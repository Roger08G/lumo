use thiserror::Error;

pub type LumoResult<T> = Result<T, LumoError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LumoError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not authorized")]
    Unauthorized,
    #[error("protected actions are temporarily locked")]
    RateLimited,
    #[error("group not initialized")]
    GroupNotInitialized,
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("invitation is invalid, expired, or already used")]
    InvalidInvitation,
    #[error("message authentication failed")]
    AuthenticationFailed,
    #[error("message has expired")]
    ExpiredMessage,
    #[error("message replay detected")]
    ReplayDetected,
    #[error("remote state changed; refresh and retry")]
    RevisionConflict,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("remote transport is not available in this phase")]
    RemoteUnavailable,
}
