pub mod auth;
pub mod config;
pub mod routes;
pub mod storage;

use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Request},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};

use auth::ReplayProtection;
use config::ApiConfig;
use lumo_core::LumoResult;
use lumo_protocol::RequestAuthenticator;
use routes::{get_compact_state, get_state, health, put_compact_state, put_state};
use storage::ApiStore;
use uuid::Uuid;

const MAX_API_REQUEST_BYTES: usize = 2_200_000;

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
        .route(
            lumo_protocol::COMPACT_STATE_PATH,
            get(get_compact_state).put(put_compact_state),
        )
        .layer(DefaultBodyLimit::max(MAX_API_REQUEST_BYTES))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(request_observability))
        .with_state(state))
}

async fn request_observability(request: Request<Body>, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    if path != lumo_protocol::HEALTH_PATH || !response.status().is_success() {
        eprintln!(
            "request_id={request_id} method={method} path={path} status={} duration_ms={}",
            response.status().as_u16(),
            started.elapsed().as_millis()
        );
    }
    response
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
}
