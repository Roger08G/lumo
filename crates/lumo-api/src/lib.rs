pub mod auth;
pub mod config;
pub mod crypto;
mod routes;
pub mod storage;

use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, MatchedPath},
    http::{HeaderValue, Request},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
    Router,
};

use auth::ReplayProtection;
use config::ApiConfig;
use crypto::MasterKey;
use lumo_core::LumoResult;
use lumo_protocol::RequestAuthenticator;
use routes::{
    apply_group_member_operation, consume_invitation, create_group, create_invitation,
    delete_group, get_compact_state, get_group_member, get_group_state, get_state, health,
    leave_group, list_devices, put_compact_state, put_group_state, put_state, revoke_device,
    verify_group_pin,
};
use storage::ApiStore;
use uuid::Uuid;

const MAX_API_REQUEST_BYTES: usize = 2_200_000;

#[derive(Clone)]
pub struct ApiState {
    pub store: ApiStore,
    pub master: MasterKey,
    pub legacy_authenticator: Option<RequestAuthenticator>,
    pub legacy_replay: Arc<ReplayProtection>,
    pub limits: config::ApiLimits,
    pub trust_proxy_headers: bool,
    /// Serializes the memory-hard public PIN bootstrap on the single-CPU,
    /// 256 MiB production container. Rate limits are reserved while this
    /// permit is held and before Argon2 is invoked.
    pub bootstrap_hash_gate: Arc<tokio::sync::Semaphore>,
}

pub fn build_app(config: &ApiConfig) -> LumoResult<Router> {
    let master = MasterKey::new(&config.master_key)?;
    let store = ApiStore::open(&config.database_path, &master)?;
    let state = ApiState {
        store,
        master,
        legacy_authenticator: config
            .legacy_password
            .as_ref()
            .map(|password| RequestAuthenticator::new(password.as_str().to_owned()))
            .transpose()?,
        legacy_replay: Arc::new(ReplayProtection::default()),
        limits: config.limits.clone(),
        trust_proxy_headers: config.trust_proxy_headers,
        bootstrap_hash_gate: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let mut router = Router::new()
        .route(lumo_protocol::HEALTH_PATH, get(health))
        .route(lumo_protocol::GROUPS_PATH, post(create_group))
        .route("/v2/groups/{group_id}", delete(delete_group))
        .route(
            "/v2/groups/{group_id}/state/compact",
            get(get_group_state).put(put_group_state),
        )
        .route("/v2/groups/{group_id}/member", get(get_group_member))
        .route(
            "/v2/groups/{group_id}/member/operations",
            post(apply_group_member_operation),
        )
        .route("/v2/groups/{group_id}/verify-pin", post(verify_group_pin))
        .route("/v2/groups/{group_id}/invitations", post(create_invitation))
        .route(
            "/v2/invitations/{invitation_id}/consume",
            post(consume_invitation),
        )
        .route("/v2/groups/{group_id}/devices", get(list_devices))
        .route(
            "/v2/groups/{group_id}/devices/{device_id}",
            delete(revoke_device),
        )
        .route("/v2/groups/{group_id}/leave", post(leave_group));
    if config.enable_legacy_v1 {
        router = router
            .route(lumo_protocol::STATE_PATH, get(get_state).put(put_state))
            .route(
                lumo_protocol::COMPACT_STATE_PATH,
                get(get_compact_state).put(put_compact_state),
            );
    }
    Ok(router
        .layer(DefaultBodyLimit::max(MAX_API_REQUEST_BYTES))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(request_observability))
        .with_state(state))
}

async fn request_observability(request: Request<Body>, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    // Never log concrete path parameters: group, invitation and device IDs are
    // identifiers for sensitive family/location data. Unknown routes get a
    // fixed label rather than reflecting attacker-controlled path content.
    let route = request_route_template(&request).to_owned();
    let started = Instant::now();
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    if route != lumo_protocol::HEALTH_PATH || !response.status().is_success() {
        eprintln!(
            "request_id={request_id} method={method} route={route} status={} duration_ms={}",
            response.status().as_u16(),
            started.elapsed().as_millis()
        );
    }
    response
}

fn request_route_template(request: &Request<Body>) -> &str {
    request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("<unmatched>")
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

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn observability_uses_route_templates_without_identifiers() {
        let group_id = "d20790a4-ae4f-4805-94bf-4028c51ccbd5";
        let app = Router::new().route(
            "/v2/groups/{group_id}",
            get(
                |request: Request<Body>| async move { request_route_template(&request).to_owned() },
            ),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v2/groups/{group_id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(&body[..], b"/v2/groups/{group_id}");
        assert!(!body
            .windows(group_id.len())
            .any(|window| window == group_id.as_bytes()));

        let unmatched = Request::builder()
            .uri(format!("/not-found/{group_id}"))
            .body(Body::empty())
            .expect("unmatched request");
        assert_eq!(request_route_template(&unmatched), "<unmatched>");
    }
}
