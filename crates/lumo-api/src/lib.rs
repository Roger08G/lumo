pub mod auth;
pub mod config;
pub mod routes;
pub mod storage;

use std::sync::Arc;

use axum::{routing::get, Router};

use auth::ReplayProtection;
use config::ApiConfig;
use lumo_core::LumoResult;
use lumo_protocol::RequestAuthenticator;
use routes::{get_state, health, put_state};
use storage::ApiStore;

#[derive(Clone)]
pub struct ApiState {
    pub store: ApiStore,
    pub authenticator: RequestAuthenticator,
    pub replay: Arc<ReplayProtection>,
}

pub fn build_app(config: &ApiConfig) -> LumoResult<Router> {
    let state = ApiState {
        store: ApiStore::open(&config.database_path)?,
        authenticator: RequestAuthenticator::new(config.password.as_str().to_owned())?,
        replay: Arc::new(ReplayProtection::default()),
    };
    Ok(Router::new()
        .route(lumo_protocol::HEALTH_PATH, get(health))
        .route(lumo_protocol::STATE_PATH, get(get_state).put(put_state))
        .with_state(state))
}
