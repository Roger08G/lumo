pub mod auth;
pub mod model;

pub use auth::{
    derive_state_key, RequestAuthenticator, SignedHeaders, MAX_CLOCK_SKEW_MS, MIN_API_SECRET_BYTES,
};
pub use model::{
    ApiErrorBody, HealthResponse, PutStateRequest, RemoteStateRecord, MAX_ENCRYPTED_STATE_BYTES,
};

pub const API_VERSION: &str = "v1";
pub const HEALTH_PATH: &str = "/health";
pub const STATE_PATH: &str = "/v1/state";
