use std::{net::SocketAddr, path::PathBuf};

use axum::{
    body::{to_bytes, Body},
    http::{header::ETAG, header::IF_NONE_MATCH, Method, Request, StatusCode},
};
use lumo_api::{
    auth::{NONCE_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER},
    build_app,
    config::ApiConfig,
};
use lumo_core::security::SealedPayload;
use lumo_protocol::{
    CompactPutStateRequest, CompactRemoteStateRecord, HealthResponse, PutStateRequest,
    RemoteStateRecord, RequestAuthenticator, COMPACT_STATE_PATH, HEALTH_PATH, STATE_PATH,
};
use tempfile::tempdir;
use tower::ServiceExt;
use zeroize::Zeroizing;

const PASSWORD: &str = "test-password-with-enough-entropy";

#[tokio::test]
async fn health_endpoint_checks_the_database_without_authentication() {
    let directory = tempdir().expect("temporary directory");
    let config = ApiConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        database_path: directory.path().join("api.sqlite3"),
        tls_cert_path: PathBuf::new(),
        tls_key_path: PathBuf::new(),
        password: Zeroizing::new(PASSWORD.to_owned()),
    };
    let response = build_app(&config)
        .expect("app")
        .oneshot(
            Request::builder()
                .uri(HEALTH_PATH)
                .body(Body::empty())
                .expect("health request"),
        )
        .await
        .expect("health response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("health body");
    let health: HealthResponse = serde_json::from_slice(&body).expect("health json");
    assert_eq!(health.status, "ok");
    assert_eq!(health.api_version, "v1");
}

#[tokio::test]
async fn signed_encrypted_state_round_trip_rejects_replay_and_conflicts() {
    let directory = tempdir().expect("temporary directory");
    let config = ApiConfig {
        bind: "127.0.0.1:0".parse::<SocketAddr>().expect("bind"),
        database_path: directory.path().join("api.sqlite3"),
        tls_cert_path: PathBuf::from("unused-cert"),
        tls_key_path: PathBuf::from("unused-key"),
        password: Zeroizing::new(PASSWORD.to_owned()),
    };
    let app = build_app(&config).expect("app");
    let request = PutStateRequest {
        expected_revision: None,
        record: RemoteStateRecord {
            revision: 1,
            envelope: SealedPayload {
                version: 1,
                message_id: "f47ac10b-58cc-4372-a567-0e02b2c3d479".into(),
                issued_at_ms: 1,
                expires_at_ms: i64::MAX,
                nonce: vec![7; 24],
                ciphertext: vec![1; 16],
            },
        },
    };
    let body = serde_json::to_vec(&request).expect("body");
    let signed_request = signed(Method::PUT, STATE_PATH, body.clone());
    let replay_request = clone_request(&signed_request, body.clone());
    let response = app.clone().oneshot(signed_request).await.expect("put");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app.clone().oneshot(replay_request).await.expect("replay");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = app
        .clone()
        .oneshot(signed(Method::GET, STATE_PATH, Vec::new()))
        .await
        .expect("get");
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response.headers().get(ETAG).expect("etag").clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let loaded: RemoteStateRecord = serde_json::from_slice(&bytes).expect("record");
    assert_eq!(loaded, request.record);

    let mut conditional = signed(Method::GET, STATE_PATH, Vec::new());
    conditional.headers_mut().insert(IF_NONE_MATCH, etag);
    let response = app
        .clone()
        .oneshot(conditional)
        .await
        .expect("conditional get");
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        response.headers()["cache-control"],
        "no-store",
        "conditional responses must not be stored by intermediaries"
    );

    let stale_revision = PutStateRequest {
        expected_revision: Some(1),
        record: request.record.clone(),
    };
    let body = serde_json::to_vec(&stale_revision).expect("stale revision body");
    let response = app
        .clone()
        .oneshot(signed(Method::PUT, STATE_PATH, body))
        .await
        .expect("stale revision");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let conflicting = PutStateRequest {
        expected_revision: Some(0),
        record: RemoteStateRecord {
            revision: 2,
            ..request.record
        },
    };
    let body = serde_json::to_vec(&conflicting).expect("conflicting body");
    let response = app
        .oneshot(signed(Method::PUT, STATE_PATH, body))
        .await
        .expect("conflict");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn compact_v1_round_trip_uses_base64_and_etags() {
    let directory = tempdir().expect("temporary directory");
    let config = ApiConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        database_path: directory.path().join("api.sqlite3"),
        tls_cert_path: PathBuf::new(),
        tls_key_path: PathBuf::new(),
        password: Zeroizing::new(PASSWORD.to_owned()),
    };
    let app = build_app(&config).expect("app");
    let legacy = PutStateRequest {
        expected_revision: None,
        record: RemoteStateRecord {
            revision: 1,
            envelope: SealedPayload {
                version: 1,
                message_id: "f47ac10b-58cc-4372-a567-0e02b2c3d479".into(),
                issued_at_ms: 1,
                expires_at_ms: i64::MAX,
                nonce: vec![7; 24],
                ciphertext: vec![42; 4_096],
            },
        },
    };
    let compact = CompactPutStateRequest::from(&legacy);
    let body = serde_json::to_vec(&compact).expect("compact body");
    let legacy_body = serde_json::to_vec(&legacy).expect("legacy body");
    assert!(body.len() * 2 < legacy_body.len());

    let response = app
        .clone()
        .oneshot(signed(Method::PUT, COMPACT_STATE_PATH, body))
        .await
        .expect("compact put");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.headers().contains_key(ETAG));

    let response = app
        .oneshot(signed(Method::GET, COMPACT_STATE_PATH, Vec::new()))
        .await
        .expect("compact get");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("compact response");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert!(json["envelope"]["ciphertext"].is_string());
    let compact: CompactRemoteStateRecord = serde_json::from_slice(&bytes).expect("record");
    assert_eq!(
        RemoteStateRecord::try_from(compact).expect("legacy"),
        legacy.record
    );
}

#[tokio::test]
async fn unsigned_requests_are_rejected() {
    let directory = tempdir().expect("temporary directory");
    let config = ApiConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        database_path: directory.path().join("api.sqlite3"),
        tls_cert_path: PathBuf::new(),
        tls_key_path: PathBuf::new(),
        password: Zeroizing::new(PASSWORD.to_owned()),
    };
    let response = build_app(&config)
        .expect("app")
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(STATE_PATH)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

fn signed(method: Method, path: &str, body: Vec<u8>) -> Request<Body> {
    let now_ms = system_now_ms();
    let signed = RequestAuthenticator::new(PASSWORD.to_owned())
        .expect("authenticator")
        .sign(method.as_str(), path, &body, now_ms);
    Request::builder()
        .method(method)
        .uri(path)
        .header(TIMESTAMP_HEADER, signed.timestamp_ms)
        .header(NONCE_HEADER, signed.nonce)
        .header(SIGNATURE_HEADER, signed.signature)
        .body(Body::from(body))
        .expect("request")
}

fn clone_request(request: &Request<Body>, body: Vec<u8>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(request.method().clone())
        .uri(request.uri().clone());
    for (name, value) in request.headers() {
        builder = builder.header(name, value);
    }
    builder.body(Body::from(body)).expect("cloned request")
}

fn system_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
