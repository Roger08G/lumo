use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use lumo_api::{
    build_app,
    config::{ApiConfig, ApiLimits},
    crypto::MasterKey,
};
use lumo_core::{
    application::{CreateGroupInput, ReportLocationInput, SetTrackingInput},
    domain::{AppSnapshot, Connectivity, PermissionState, RuntimeState},
    security::{ReplayGuard, SealedPayload, SessionCipher},
    LumoService,
};
use lumo_protocol::{
    group_device_path, group_devices_path, group_invitations_path, group_leave_path,
    group_member_operations_path, group_member_path, group_path, group_state_path,
    group_verify_pin_path, invitation_consume_path, ApiErrorBody, CompactPutStateRequest,
    CompactRemoteStateRecord, CompactSealedPayload, ConsumeInvitationRequest, ControlledOperation,
    ControlledOperationRequest, ControlledOperationResponse, CreateGroupRequest,
    CreateInvitationRequest, DeviceCredentialResponse, DeviceListResponse, DeviceRole,
    HealthResponse, InvitationResponse, MemberOperationEnvelopeRequest, ProtectedActionRequest,
    PutStateRequest, RemoteStateRecord, DEVICE_ID_HEADER, HEALTH_PATH, NONCE_HEADER, STATE_PATH,
    TIMESTAMP_HEADER,
};
use tempfile::tempdir;
use tower::ServiceExt;
use uuid::Uuid;
use zeroize::Zeroizing;

const MASTER_KEY: &str = "test-only-server-master-key-with-at-least-32-bytes";
const PIN: &str = "123456";
static NONCE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
async fn health_reports_v2_and_legacy_routes_are_disabled() {
    let directory = tempdir().expect("temporary directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("application");

    let response = app
        .clone()
        .oneshot(request(Method::GET, HEALTH_PATH, Vec::new()))
        .await
        .expect("health response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let health: HealthResponse = response_json(response).await;
    assert_eq!(health.status, "ok");
    assert_eq!(health.api_version, "v2");

    let response = app
        .oneshot(request(Method::GET, STATE_PATH, Vec::new()))
        .await
        .expect("legacy response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn groups_are_isolated_reads_allow_repeated_nonces_and_mutations_reject_replay() {
    let directory = tempdir().expect("temporary directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("application");
    let controller_a = create_group(&app, "Controller A", "192.0.2.10").await;
    let controller_b = create_group(&app, "Controller B", "192.0.2.11").await;
    let path_a = group_state_path(&controller_a.group_id);
    let path_b = group_state_path(&controller_b.group_id);

    let put = PutStateRequest {
        expected_revision: None,
        record: state_record(1),
    };
    let body = serde_json::to_vec(&CompactPutStateRequest::from(&put)).expect("state body");
    let mutation = authenticated_request(Method::PUT, &path_a, body.clone(), &controller_a);
    let replay = clone_request(&mutation, body);
    let response = app.clone().oneshot(mutation).await.expect("first mutation");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.headers().contains_key("etag"));
    let response = app.clone().oneshot(replay).await.expect("mutation replay");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let read = authenticated_request(Method::GET, &path_a, Vec::new(), &controller_a);
    let repeated_read = clone_request(&read, Vec::new());
    for read in [read, repeated_read] {
        let response = app.clone().oneshot(read).await.expect("state read");
        assert_eq!(response.status(), StatusCode::OK);
        let compact: lumo_protocol::CompactRemoteStateRecord = response_json(response).await;
        assert_eq!(
            RemoteStateRecord::try_from(compact).expect("state"),
            put.record
        );
    }

    let response = app
        .clone()
        .oneshot(authenticated_request(
            Method::GET,
            &path_b,
            Vec::new(),
            &controller_a,
        ))
        .await
        .expect("cross-group request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(authenticated_request(
            Method::GET,
            &path_b,
            Vec::new(),
            &controller_b,
        ))
        .await
        .expect("empty group request");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let conflict = PutStateRequest {
        expected_revision: Some(0),
        record: state_record(2),
    };
    let response = app
        .oneshot(authenticated_json_request(
            Method::PUT,
            &path_a,
            &CompactPutStateRequest::from(&conflict),
            &controller_a,
        ))
        .await
        .expect("CAS conflict");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn invitations_are_single_use_and_devices_are_revocable() {
    let directory = tempdir().expect("temporary directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.20").await;
    let invitation = create_invitation(&app, &controller).await;
    let consume = ConsumeInvitationRequest {
        request_id: Uuid::new_v4().to_string(),
        token: invitation.token.clone(),
        pin: PIN.to_owned(),
        device_name: "Controlled".to_owned(),
    };
    let consume_path = invitation_consume_path(&invitation.invitation_id);
    let request_a = json_request(Method::POST, &consume_path, &consume);
    let request_b = json_request(Method::POST, &consume_path, &consume);
    let (response_a, response_b) = tokio::join!(
        app.clone().oneshot(request_a),
        app.clone().oneshot(request_b)
    );
    let response_a = response_a.expect("first consume");
    let response_b = response_b.expect("second consume");
    assert_eq!(response_a.status(), StatusCode::CREATED);
    assert_eq!(response_b.status(), StatusCode::CREATED);
    let controlled: DeviceCredentialResponse = response_json(response_a).await;
    let replayed: DeviceCredentialResponse = response_json(response_b).await;
    assert_eq!(controlled, replayed);
    assert_eq!(controlled.group_id, controller.group_id);
    assert_eq!(controlled.role, DeviceRole::Controlled);

    let devices_path = group_devices_path(&controller.group_id);
    let response = app
        .clone()
        .oneshot(authenticated_request(
            Method::GET,
            &devices_path,
            Vec::new(),
            &controller,
        ))
        .await
        .expect("device list");
    assert_eq!(response.status(), StatusCode::OK);
    let devices: DeviceListResponse = response_json(response).await;
    assert_eq!(
        devices
            .devices
            .iter()
            .filter(|device| device.revoked_at_ms.is_none())
            .count(),
        2
    );

    let revoke_path = group_device_path(&controller.group_id, &controlled.device_id);
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::DELETE,
            &revoke_path,
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controller,
        ))
        .await
        .expect("revoke device");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app
        .oneshot(authenticated_request(
            Method::GET,
            &group_state_path(&controller.group_id),
            Vec::new(),
            &controlled,
        ))
        .await
        .expect("revoked authentication");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invitations_allow_multiple_controllers_but_only_one_controlled_device() {
    let directory = tempdir().expect("temporary directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("application");
    let first_controller = create_group(&app, "Controller", "192.0.2.25").await;
    seed_runtime_state(&app, &first_controller).await;

    let controller_invitation =
        create_invitation_with_role(&app, &first_controller, DeviceRole::Controller).await;
    assert_eq!(controller_invitation.role, DeviceRole::Controller);
    let second_controller =
        consume_controlled(&app, &controller_invitation, Uuid::new_v4().to_string()).await;
    assert_eq!(second_controller.role, DeviceRole::Controller);
    assert_eq!(second_controller.state_key, first_controller.state_key);

    let first_controlled_invitation = create_invitation(&app, &second_controller).await;
    let second_controlled_invitation = create_invitation(&app, &first_controller).await;
    let controlled = consume_controlled(
        &app,
        &first_controlled_invitation,
        Uuid::new_v4().to_string(),
    )
    .await;
    assert_eq!(controlled.role, DeviceRole::Controlled);
    assert_ne!(controlled.state_key, first_controller.state_key);

    let response = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&second_controlled_invitation.invitation_id),
            &ConsumeInvitationRequest {
                request_id: Uuid::new_v4().to_string(),
                token: second_controlled_invitation.token,
                pin: PIN.to_owned(),
                device_name: "Second controlled".to_owned(),
            },
        ))
        .await
        .expect("reject second controlled");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .oneshot(authenticated_request(
            Method::GET,
            &group_devices_path(&first_controller.group_id),
            Vec::new(),
            &second_controller,
        ))
        .await
        .expect("list devices from invited controller");
    assert_eq!(response.status(), StatusCode::OK);
    let devices: DeviceListResponse = response_json(response).await;
    assert_eq!(
        devices
            .devices
            .iter()
            .filter(|device| {
                device.role == DeviceRole::Controller && device.revoked_at_ms.is_none()
            })
            .count(),
        2
    );
    assert_eq!(
        devices
            .devices
            .iter()
            .filter(|device| {
                device.role == DeviceRole::Controlled && device.revoked_at_ms.is_none()
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn member_endpoints_enforce_acl_key_isolation_and_least_privilege() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let app = build_app(&test_config(&database_path)).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.21").await;
    seed_runtime_state(&app, &controller).await;
    let invitation = create_invitation(&app, &controller).await;
    let controlled = consume_controlled(&app, &invitation, Uuid::new_v4().to_string()).await;
    assert_ne!(controller.state_key, controlled.state_key);

    let canonical_path = group_state_path(&controller.group_id);
    for method in [Method::GET, Method::PUT] {
        let response = app
            .clone()
            .oneshot(authenticated_request(
                method,
                &canonical_path,
                Vec::new(),
                &controlled,
            ))
            .await
            .expect("controlled canonical request");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error: ApiErrorBody = response_json(response).await;
        assert_eq!(error.code, "unauthorized");
    }

    let member_path = group_member_path(&controller.group_id);
    let response = app
        .clone()
        .oneshot(authenticated_request(
            Method::GET,
            &member_path,
            Vec::new(),
            &controller,
        ))
        .await
        .expect("controller member request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(authenticated_request(
            Method::GET,
            &member_path,
            Vec::new(),
            &controlled,
        ))
        .await
        .expect("member snapshot");
    assert_eq!(response.status(), StatusCode::OK);
    let compact: CompactRemoteStateRecord = response_json(response).await;
    let member_record = RemoteStateRecord::try_from(compact.clone()).expect("member record");
    assert!(SessionCipher::from_key(credential_key(&controller))
        .open::<AppSnapshot>(
            &member_record.envelope,
            system_now_ms(),
            &mut ReplayGuard::default(),
        )
        .is_err());
    let (snapshot, member_record): (AppSnapshot, _) =
        open_compact(compact, credential_key(&controlled));
    assert_eq!(member_record.revision, snapshot.revision);
    assert!(snapshot.session.is_some());
    assert!(snapshot.places.is_empty());
    assert!(snapshot.events.is_empty());
    assert!(snapshot.commands.is_empty());
    let snapshot_json = serde_json::to_string(&snapshot).expect("snapshot JSON");
    for private in ["pinHash", "pinGuard", "invitations", "stateKey"] {
        assert!(!snapshot_json.contains(private), "leaked {private}");
    }

    let response = app
        .clone()
        .oneshot(authenticated_request(
            Method::GET,
            &canonical_path,
            Vec::new(),
            &controller,
        ))
        .await
        .expect("canonical state");
    let canonical: CompactRemoteStateRecord = response_json(response).await;
    let canonical = RemoteStateRecord::try_from(canonical).expect("canonical record");
    assert!(SessionCipher::from_key(credential_key(&controlled))
        .open::<RuntimeState>(
            &canonical.envelope,
            system_now_ms(),
            &mut ReplayGuard::default(),
        )
        .is_err());

    let connection = rusqlite::Connection::open(database_path).expect("database");
    let (member_nonce, member_ciphertext): (Vec<u8>, Vec<u8>) = connection
        .query_row(
            "SELECT member_key_nonce, member_key_ciphertext FROM devices_v2 WHERE id = ?1",
            [&controlled.device_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("wrapped member key");
    assert_eq!(member_nonce.len(), 24);
    assert!(!member_ciphertext
        .windows(32)
        .any(|window| window == credential_key(&controlled)));
}

#[tokio::test]
async fn member_operations_are_encrypted_idempotent_and_domain_typed() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let config = test_config(&database_path);
    let app = build_app(&config).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.22").await;
    seed_runtime_state(&app, &controller).await;
    let controlled = consume_controlled(
        &app,
        &create_invitation(&app, &controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;
    let path = group_member_operations_path(&controller.group_id);

    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &path,
            &sealed_member_operation(
                &controlled,
                &Uuid::new_v4().to_string(),
                ControlledOperation::ReportLocation(ReportLocationInput {
                    latitude: 40.4,
                    longitude: -3.7,
                    accuracy_m: 10.0,
                    battery_percent: 80,
                    captured_at_ms: None,
                }),
                system_now_ms(),
                300_000,
            ),
            &controlled,
        ))
        .await
        .expect("disabled tracking response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: ApiErrorBody = response_json(response).await;
    assert_eq!(error.code, "tracking_disabled");

    let operation_id = Uuid::new_v4().to_string();
    let operation = ControlledOperation::SetTracking(SetTrackingInput {
        precise_permission: PermissionState::Granted,
        background_permission: PermissionState::Granted,
        battery_optimization_disabled: true,
        enabled: true,
    });
    let submit = |operation: ControlledOperation| {
        authenticated_json_request(
            Method::POST,
            &path,
            &sealed_member_operation(
                &controlled,
                &operation_id,
                operation,
                system_now_ms(),
                300_000,
            ),
            &controlled,
        )
    };
    let response = app
        .clone()
        .oneshot(submit(operation.clone()))
        .await
        .expect("set tracking");
    assert_eq!(response.status(), StatusCode::OK);
    let first: CompactRemoteStateRecord = response_json(response).await;
    let (first, first_record): (ControlledOperationResponse, _) =
        open_compact(first, credential_key(&controlled));
    assert!(first.snapshot.controlled.tracking_enabled);
    assert_eq!(first.processed, None);
    assert_eq!(first_record.revision, first.snapshot.revision);

    let restarted = build_app(&config).expect("restarted application");
    let response = restarted
        .clone()
        .oneshot(submit(operation))
        .await
        .expect("operation replay");
    let replay: CompactRemoteStateRecord = response_json(response).await;
    let (replay, _): (ControlledOperationResponse, _) =
        open_compact(replay, credential_key(&controlled));
    assert_eq!(replay, first);

    let response = restarted
        .clone()
        .oneshot(submit(ControlledOperation::SetConnectivity {
            connectivity: Connectivity::Offline,
        }))
        .await
        .expect("operation conflict");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let error: ApiErrorBody = response_json(response).await;
    assert_eq!(error.code, "idempotency_conflict");

    let response = restarted
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &path,
            &sealed_member_operation(
                &controlled,
                &Uuid::new_v4().to_string(),
                ControlledOperation::ProcessPending,
                system_now_ms(),
                300_000,
            ),
            &controlled,
        ))
        .await
        .expect("process pending");
    assert_eq!(response.status(), StatusCode::OK);
    let processed: CompactRemoteStateRecord = response_json(response).await;
    let (processed, processed_record): (ControlledOperationResponse, _) =
        open_compact(processed, credential_key(&controlled));
    assert!(processed.processed.is_some());
    assert_eq!(processed_record.revision, processed.snapshot.revision);

    let response = restarted
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &path,
            &sealed_member_operation(
                &controlled,
                &Uuid::new_v4().to_string(),
                ControlledOperation::ProcessPending,
                system_now_ms(),
                300_001,
            ),
            &controlled,
        ))
        .await
        .expect("oversized TTL");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pin_lockout_is_scoped_per_device_survives_restart_and_cannot_block_revoke() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let config = test_config(&database_path);
    let app = build_app(&config).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.23").await;
    seed_runtime_state(&app, &controller).await;
    let controlled = consume_controlled(
        &app,
        &create_invitation(&app, &controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;
    let path = group_verify_pin_path(&controller.group_id);

    for credential in [&controller, &controlled] {
        let response = app
            .clone()
            .oneshot(authenticated_json_request(
                Method::POST,
                &path,
                &ProtectedActionRequest {
                    pin: PIN.to_owned(),
                },
                credential,
            ))
            .await
            .expect("verify PIN");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    for attempt in 1..=5 {
        let response = app
            .clone()
            .oneshot(authenticated_json_request(
                Method::POST,
                &path,
                &ProtectedActionRequest {
                    pin: "654321".to_owned(),
                },
                &controlled,
            ))
            .await
            .expect("wrong PIN");
        assert_eq!(
            response.status(),
            if attempt == 5 {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::FORBIDDEN
            }
        );
    }
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &path,
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controlled,
        ))
        .await
        .expect("locked correct PIN");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    // A hostile controlled device must not be able to lock the controller out
    // of the very revocation action that contains it.
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &path,
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controller,
        ))
        .await
        .expect("controller PIN remains independent");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    drop(app);
    let restarted = build_app(&config).expect("restarted application");
    let response = restarted
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &path,
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controlled,
        ))
        .await
        .expect("controlled lock survives restart");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    let response = restarted
        .oneshot(authenticated_json_request(
            Method::DELETE,
            &group_device_path(&controller.group_id, &controlled.device_id),
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controller,
        ))
        .await
        .expect("controller revokes locked controlled device");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn bootstrap_and_consume_idempotency_survive_restart_and_bind_the_body() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let config = test_config(&database_path);
    let app = build_app(&config).expect("application");
    let create_request = CreateGroupRequest {
        request_id: Uuid::new_v4().to_string(),
        pin: PIN.to_owned(),
        device_name: " Controller ".to_owned(),
    };
    let response = create_group_with_request(&app, &create_request, "192.0.2.24").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let controller: DeviceCredentialResponse = response_json(response).await;

    let restarted = build_app(&config).expect("restarted application");
    let mut normalized_replay = create_request.clone();
    normalized_replay.device_name = "Controller".to_owned();
    let response = create_group_with_request(&restarted, &normalized_replay, "192.0.2.24").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let replayed_controller: DeviceCredentialResponse = response_json(response).await;
    assert_eq!(replayed_controller, controller);

    let mut conflict = normalized_replay.clone();
    conflict.device_name = "Different".to_owned();
    let response = create_group_with_request(&restarted, &conflict, "192.0.2.24").await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let invitation = create_invitation(&restarted, &controller).await;
    let consume_request = ConsumeInvitationRequest {
        request_id: Uuid::new_v4().to_string(),
        token: invitation.token.clone(),
        pin: PIN.to_owned(),
        device_name: " Controlled ".to_owned(),
    };
    let consume_path = invitation_consume_path(&invitation.invitation_id);
    let response = restarted
        .clone()
        .oneshot(json_request(Method::POST, &consume_path, &consume_request))
        .await
        .expect("consume");
    assert_eq!(response.status(), StatusCode::CREATED);
    let controlled: DeviceCredentialResponse = response_json(response).await;

    let restarted_again = build_app(&config).expect("second restart");
    let mut consume_replay = consume_request.clone();
    consume_replay.device_name = "Controlled".to_owned();
    let response = restarted_again
        .clone()
        .oneshot(json_request(Method::POST, &consume_path, &consume_replay))
        .await
        .expect("consume replay");
    assert_eq!(response.status(), StatusCode::CREATED);
    let replayed_controlled: DeviceCredentialResponse = response_json(response).await;
    assert_eq!(replayed_controlled, controlled);

    let mut changed = consume_replay.clone();
    changed.device_name = "Different".to_owned();
    let response = restarted_again
        .clone()
        .oneshot(json_request(Method::POST, &consume_path, &changed))
        .await
        .expect("consume conflict");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let mut different_id = consume_replay;
    different_id.request_id = Uuid::new_v4().to_string();
    let response = restarted_again
        .oneshot(json_request(Method::POST, &consume_path, &different_id))
        .await
        .expect("single-use invitation");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let connection = rusqlite::Connection::open(database_path).expect("database");
    let bootstrap_attempts: i64 = connection
        .query_row(
            "SELECT attempts FROM bootstrap_limits_v2 WHERE scope_key = 'global'",
            [],
            |row| row.get(0),
        )
        .expect("global bootstrap attempts");
    let bootstrap_reservations: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM bootstrap_requests_v2 WHERE request_id = ?1",
            rusqlite::params![create_request.request_id],
            |row| row.get(0),
        )
        .expect("bootstrap reservation");
    assert_eq!(bootstrap_attempts, 1);
    assert_eq!(bootstrap_reservations, 1);
    let mut statement = connection
        .prepare("SELECT response_ciphertext FROM idempotency_v2 ORDER BY kind")
        .expect("idempotency query");
    let ciphertexts = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .expect("idempotency rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("idempotency ciphertexts");
    assert_eq!(ciphertexts.len(), 2);
    let secrets = [
        controller.device_token.as_bytes(),
        controller.state_key.as_bytes(),
        controlled.device_token.as_bytes(),
        controlled.state_key.as_bytes(),
    ];
    for ciphertext in ciphertexts {
        assert!(serde_json::from_slice::<DeviceCredentialResponse>(&ciphertext).is_err());
        for secret in &secrets {
            assert!(!ciphertext
                .windows(secret.len())
                .any(|window| window == *secret));
        }
    }
}

#[tokio::test]
async fn concurrent_bootstrap_replay_consumes_one_rate_slot_and_creates_one_group() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let mut config = test_config(&database_path);
    config.limits.bootstrap_global = 1;
    config.limits.bootstrap_per_ip = 1;
    let app = build_app(&config).expect("application");
    let create_request = CreateGroupRequest {
        request_id: Uuid::new_v4().to_string(),
        pin: PIN.to_owned(),
        device_name: "Controller".to_owned(),
    };

    let (first, second) = tokio::join!(
        create_group_with_request(&app, &create_request, "192.0.2.27"),
        create_group_with_request(&app, &create_request, "192.0.2.27")
    );
    assert_eq!(first.status(), StatusCode::CREATED);
    assert_eq!(second.status(), StatusCode::CREATED);
    let first: DeviceCredentialResponse = response_json(first).await;
    let second: DeviceCredentialResponse = response_json(second).await;
    assert_eq!(first, second);

    let connection = rusqlite::Connection::open(database_path).expect("database");
    let groups: i64 = connection
        .query_row("SELECT COUNT(*) FROM groups_v2", [], |row| row.get(0))
        .expect("group count");
    let attempts: i64 = connection
        .query_row(
            "SELECT attempts FROM bootstrap_limits_v2 WHERE scope_key = 'global'",
            [],
            |row| row.get(0),
        )
        .expect("global bootstrap attempts");
    let reservations: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM bootstrap_requests_v2 WHERE request_id = ?1",
            rusqlite::params![create_request.request_id],
            |row| row.get(0),
        )
        .expect("bootstrap reservation");
    assert_eq!(groups, 1);
    assert_eq!(attempts, 1);
    assert_eq!(reservations, 1);
}

#[tokio::test]
async fn revoke_and_member_write_linearize_without_post_revoke_mutation() {
    let directory = tempdir().expect("temporary directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.25").await;
    seed_runtime_state(&app, &controller).await;
    let controlled = consume_controlled(
        &app,
        &create_invitation(&app, &controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;
    let operation_path = group_member_operations_path(&controller.group_id);
    let operation = authenticated_json_request(
        Method::POST,
        &operation_path,
        &sealed_member_operation(
            &controlled,
            &Uuid::new_v4().to_string(),
            ControlledOperation::SetConnectivity {
                connectivity: Connectivity::Offline,
            },
            system_now_ms(),
            300_000,
        ),
        &controlled,
    );
    let revoke = authenticated_json_request(
        Method::DELETE,
        &group_device_path(&controller.group_id, &controlled.device_id),
        &ProtectedActionRequest {
            pin: PIN.to_owned(),
        },
        &controller,
    );
    let (operation_response, revoke_response) =
        tokio::join!(app.clone().oneshot(operation), app.clone().oneshot(revoke));
    let operation_status = operation_response.expect("operation response").status();
    assert!(matches!(
        operation_status,
        StatusCode::OK | StatusCode::UNAUTHORIZED
    ));
    assert_eq!(
        revoke_response.expect("revoke response").status(),
        StatusCode::NO_CONTENT
    );

    let response = app
        .oneshot(authenticated_json_request(
            Method::POST,
            &operation_path,
            &sealed_member_operation(
                &controlled,
                &Uuid::new_v4().to_string(),
                ControlledOperation::SendHelp,
                system_now_ms(),
                300_000,
            ),
            &controlled,
        ))
        .await
        .expect("post-revoke operation");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invitation_pin_failures_lock_and_expired_invitations_fail_closed() {
    let directory = tempdir().expect("temporary directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.30").await;
    let invitation = create_invitation(&app, &controller).await;
    let path = invitation_consume_path(&invitation.invitation_id);
    let wrong = ConsumeInvitationRequest {
        request_id: Uuid::new_v4().to_string(),
        token: invitation.token.clone(),
        pin: "654321".to_owned(),
        device_name: "Controlled".to_owned(),
    };
    for attempt in 1..=5 {
        let response = app
            .clone()
            .oneshot(json_request(Method::POST, &path, &wrong))
            .await
            .expect("wrong PIN response");
        let expected = if attempt == 5 {
            StatusCode::TOO_MANY_REQUESTS
        } else {
            StatusCode::BAD_REQUEST
        };
        assert_eq!(response.status(), expected);
    }
    let correct = ConsumeInvitationRequest {
        request_id: Uuid::new_v4().to_string(),
        token: invitation.token,
        pin: PIN.to_owned(),
        device_name: "Controlled".to_owned(),
    };
    let response = app
        .oneshot(json_request(Method::POST, &path, &correct))
        .await
        .expect("locked invitation response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    let expired_directory = tempdir().expect("temporary directory");
    let mut expired_config = test_config(&expired_directory.path().join("api.sqlite3"));
    expired_config.limits.invite_ttl_ms = 1;
    let expired_app = build_app(&expired_config).expect("application");
    let expired_controller = create_group(&expired_app, "Controller", "192.0.2.31").await;
    let expired_invitation = create_invitation(&expired_app, &expired_controller).await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let response = expired_app
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&expired_invitation.invitation_id),
            &ConsumeInvitationRequest {
                request_id: Uuid::new_v4().to_string(),
                token: expired_invitation.token,
                pin: PIN.to_owned(),
                device_name: "Controlled".to_owned(),
            },
        ))
        .await
        .expect("expired invitation response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn bootstrap_rate_limits_and_group_quota_are_enforced() {
    let directory = tempdir().expect("temporary directory");
    let mut config = test_config(&directory.path().join("api.sqlite3"));
    config.limits.bootstrap_per_ip = 1;
    config.limits.bootstrap_global = 2;
    let app = build_app(&config).expect("application");

    assert_eq!(
        create_group_status(&app, "192.0.2.40").await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_group_status(&app, "192.0.2.40").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        create_group_status(&app, "192.0.2.41").await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_group_status(&app, "192.0.2.42").await,
        StatusCode::TOO_MANY_REQUESTS
    );

    let quota_directory = tempdir().expect("temporary directory");
    let mut quota_config = test_config(&quota_directory.path().join("api.sqlite3"));
    quota_config.limits.max_groups = 1;
    let quota_app = build_app(&quota_config).expect("application");
    assert_eq!(
        create_group_status(&quota_app, "192.0.2.50").await,
        StatusCode::CREATED
    );
    assert_eq!(
        create_group_status(&quota_app, "192.0.2.51").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn controlled_can_leave_and_controller_can_delete_with_the_pin() {
    let directory = tempdir().expect("temporary directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.60").await;
    let invitation = create_invitation(&app, &controller).await;
    let controlled: DeviceCredentialResponse = response_json(
        app.clone()
            .oneshot(json_request(
                Method::POST,
                &invitation_consume_path(&invitation.invitation_id),
                &ConsumeInvitationRequest {
                    request_id: Uuid::new_v4().to_string(),
                    token: invitation.token,
                    pin: PIN.to_owned(),
                    device_name: "Controlled".to_owned(),
                },
            ))
            .await
            .expect("consume invitation"),
    )
    .await;

    let leave_path = group_leave_path(&controller.group_id);
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &leave_path,
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controlled,
        ))
        .await
        .expect("leave group");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let delete_path = group_path(&controller.group_id);
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::DELETE,
            &delete_path,
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controller,
        ))
        .await
        .expect("delete group");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app
        .oneshot(authenticated_request(
            Method::GET,
            &group_state_path(&controller.group_id),
            Vec::new(),
            &controller,
        ))
        .await
        .expect("deleted group authentication");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sqlite_persists_only_keyed_hashes_and_wrapped_group_keys() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let app = build_app(&test_config(&database_path)).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.70").await;
    let invitation = create_invitation(&app, &controller).await;

    let connection = rusqlite::Connection::open(&database_path).expect("database");
    let (stored_pin_hash, wrapped_key): (String, Vec<u8>) = connection
        .query_row(
            "SELECT pin_hash, state_key_ciphertext FROM groups_v2 WHERE id = ?1",
            [&controller.group_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("group secrets");
    let device_token_hash: Vec<u8> = connection
        .query_row(
            "SELECT token_hash FROM devices_v2 WHERE id = ?1",
            [&controller.device_id],
            |row| row.get(0),
        )
        .expect("device token hash");
    let invitation_token_hash: Vec<u8> = connection
        .query_row(
            "SELECT token_hash FROM invitations_v2 WHERE id = ?1",
            [&invitation.invitation_id],
            |row| row.get(0),
        )
        .expect("invitation token hash");

    let raw_state_key = URL_SAFE_NO_PAD
        .decode(&controller.state_key)
        .expect("state key");
    let raw_device_token = URL_SAFE_NO_PAD
        .decode(&controller.device_token)
        .expect("device token");
    let raw_invitation_token = URL_SAFE_NO_PAD
        .decode(&invitation.token)
        .expect("invitation token");
    assert_ne!(wrapped_key, raw_state_key);
    assert_ne!(device_token_hash, raw_device_token);
    assert_ne!(invitation_token_hash, raw_invitation_token);
    assert!(!stored_pin_hash.contains(PIN));
    assert!(!lumo_core::security::verify_pin(PIN, &stored_pin_hash));

    let master = MasterKey::new(MASTER_KEY).expect("master");
    let other =
        MasterKey::new("different-server-master-key-with-32-byte-minimum").expect("other master");
    assert!(master.verify_group_pin(&controller.group_id, PIN, &stored_pin_hash));
    assert!(!other.verify_group_pin(&controller.group_id, PIN, &stored_pin_hash));
    assert!(master.verify_token_hash(&controller.device_token, &device_token_hash));
    assert!(!other.verify_token_hash(&controller.device_token, &device_token_hash));
    assert!(master.verify_token_hash(&invitation.token, &invitation_token_hash));
    assert!(!other.verify_token_hash(&invitation.token, &invitation_token_hash));
}

#[test]
fn v2_migration_preserves_legacy_storage_without_exposing_it() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).expect("legacy database");
    connection
        .execute_batch(
            "CREATE TABLE remote_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                revision INTEGER NOT NULL,
                payload BLOB NOT NULL
             );
             INSERT INTO remote_state(singleton, revision, payload) VALUES(1, 1, x'7b7d');
             PRAGMA user_version = 1;",
        )
        .expect("legacy schema");
    drop(connection);

    let config = test_config(&database_path);
    let _app = build_app(&config).expect("v2 migration");
    let connection = rusqlite::Connection::open(database_path).expect("migrated database");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema version");
    let legacy_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM remote_state", [], |row| row.get(0))
        .expect("legacy rows");
    let v2_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='table' AND name IN (
                'groups_v2', 'bootstrap_requests_v2', 'device_pin_guards_v2'
             )",
            [],
            |row| row.get(0),
        )
        .expect("v2 table");
    assert_eq!(version, 5);
    assert_eq!(legacy_rows, 1);
    assert_eq!(v2_tables, 3);
}

#[tokio::test]
async fn schema_v3_migrates_pin_guards_and_bootstrap_reservations_without_data_loss() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let config = test_config(&database_path);
    let app = build_app(&config).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.28").await;
    drop(app);

    let connection = rusqlite::Connection::open(&database_path).expect("database");
    connection
        .execute_batch(
            "DROP TABLE device_pin_guards_v2;
             DROP TABLE bootstrap_requests_v2;
             PRAGMA user_version = 3;",
        )
        .expect("simulate v3 schema");
    drop(connection);

    let migrated = build_app(&config).expect("v3 migration");
    let response = migrated
        .oneshot(authenticated_json_request(
            Method::POST,
            &group_verify_pin_path(&controller.group_id),
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controller,
        ))
        .await
        .expect("existing controller after migration");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let connection = rusqlite::Connection::open(database_path).expect("migrated database");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema version");
    let new_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='table' AND name IN (
                'bootstrap_requests_v2', 'device_pin_guards_v2'
             )",
            [],
            |row| row.get(0),
        )
        .expect("v4 tables");
    let existing_group: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM groups_v2 WHERE id = ?1",
            [&controller.group_id],
            |row| row.get(0),
        )
        .expect("existing group");
    assert_eq!(version, 5);
    assert_eq!(new_tables, 2);
    assert_eq!(existing_group, 1);
}

#[tokio::test]
async fn database_master_key_check_fails_closed_across_restart_and_v2_migration() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("api.sqlite3");
    let config = test_config(&database_path);
    let app = build_app(&config).expect("application");
    create_group(&app, "Controller", "192.0.2.26").await;
    drop(app);

    // Simulate the migration case that has no verifier yet: an existing v2 database.
    let connection = rusqlite::Connection::open(&database_path).expect("database");
    connection
        .execute("DELETE FROM server_meta_v2", [])
        .expect("remove verifier");
    connection
        .pragma_update(None, "user_version", 2_i64)
        .expect("v2 marker");
    drop(connection);

    let mut wrong = config.clone();
    wrong.master_key = Zeroizing::new(
        "different-test-master-key-with-at-least-32-bytes-and-high-entropy".to_owned(),
    );
    assert!(matches!(
        build_app(&wrong),
        Err(lumo_core::LumoError::Configuration(_))
    ));
    let _app = build_app(&config).expect("correct master recovers migration");

    let connection = rusqlite::Connection::open(database_path).expect("database");
    let check: Vec<u8> = connection
        .query_row(
            "SELECT value FROM server_meta_v2 WHERE key = 'master_key_check_v1'",
            [],
            |row| row.get(0),
        )
        .expect("master key verifier");
    assert_eq!(check.len(), 32);
    assert!(!check
        .windows(MASTER_KEY.len())
        .any(|window| window == MASTER_KEY.as_bytes()));
}

fn test_config(database_path: &Path) -> ApiConfig {
    ApiConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        database_path: database_path.to_owned(),
        tls_cert_path: PathBuf::new(),
        tls_key_path: PathBuf::new(),
        master_key: Zeroizing::new(MASTER_KEY.to_owned()),
        enable_legacy_v1: false,
        legacy_password: None,
        trust_proxy_headers: false,
        limits: ApiLimits::default(),
    }
}

async fn create_group(app: &Router, device_name: &str, peer_ip: &str) -> DeviceCredentialResponse {
    let response = create_group_response(app, device_name, peer_ip).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let credential: DeviceCredentialResponse = response_json(response).await;
    assert_eq!(credential.role, DeviceRole::Controller);
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(&credential.device_token)
            .expect("token")
            .len(),
        32
    );
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(&credential.state_key)
            .expect("key")
            .len(),
        32
    );
    credential
}

async fn create_group_status(app: &Router, peer_ip: &str) -> StatusCode {
    create_group_response(app, "Controller", peer_ip)
        .await
        .status()
}

async fn create_group_response(
    app: &Router,
    device_name: &str,
    peer_ip: &str,
) -> axum::response::Response {
    create_group_with_request(
        app,
        &CreateGroupRequest {
            request_id: Uuid::new_v4().to_string(),
            pin: PIN.to_owned(),
            device_name: device_name.to_owned(),
        },
        peer_ip,
    )
    .await
}

async fn create_group_with_request(
    app: &Router,
    value: &CreateGroupRequest,
    peer_ip: &str,
) -> axum::response::Response {
    let mut request = json_request(Method::POST, lumo_protocol::GROUPS_PATH, value);
    let peer: SocketAddr = format!("{peer_ip}:32123").parse().expect("peer address");
    request.extensions_mut().insert(ConnectInfo(peer));
    app.clone().oneshot(request).await.expect("create group")
}

async fn create_invitation(
    app: &Router,
    controller: &DeviceCredentialResponse,
) -> InvitationResponse {
    create_invitation_with_role(app, controller, DeviceRole::Controlled).await
}

async fn create_invitation_with_role(
    app: &Router,
    controller: &DeviceCredentialResponse,
    role: DeviceRole,
) -> InvitationResponse {
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &group_invitations_path(&controller.group_id),
            &CreateInvitationRequest {
                pin: PIN.to_owned(),
                role,
            },
            controller,
        ))
        .await
        .expect("create invitation");
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn consume_controlled(
    app: &Router,
    invitation: &InvitationResponse,
    request_id: String,
) -> DeviceCredentialResponse {
    let response = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&invitation.invitation_id),
            &ConsumeInvitationRequest {
                request_id,
                token: invitation.token.clone(),
                pin: PIN.to_owned(),
                device_name: "Controlled".to_owned(),
            },
        ))
        .await
        .expect("consume invitation");
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn seed_runtime_state(app: &Router, controller: &DeviceCredentialResponse) -> RuntimeState {
    let mut state = RuntimeState::default();
    LumoService
        .create_group(
            &mut state,
            CreateGroupInput {
                name: "Family".to_owned(),
                supervisor_name: "Controller".to_owned(),
                supervisor_phone: "+34600000001".to_owned(),
                tracked_person_name: "Member".to_owned(),
                tracked_person_phone: "+34600000002".to_owned(),
                pin: PIN.to_owned(),
            },
            system_now_ms(),
        )
        .expect("runtime group");
    let now_ms = system_now_ms();
    let record = RemoteStateRecord {
        revision: state.revision,
        envelope: SessionCipher::from_key(credential_key(controller))
            .seal(&state, now_ms, i64::MAX.saturating_sub(now_ms))
            .expect("seal canonical state"),
    };
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::PUT,
            &group_state_path(&controller.group_id),
            &CompactPutStateRequest::from(&PutStateRequest {
                expected_revision: None,
                record,
            }),
            controller,
        ))
        .await
        .expect("seed state");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    state
}

fn credential_key(credential: &DeviceCredentialResponse) -> [u8; 32] {
    URL_SAFE_NO_PAD
        .decode(&credential.state_key)
        .expect("credential key")
        .try_into()
        .expect("32-byte credential key")
}

fn open_compact<T: serde::de::DeserializeOwned>(
    compact: CompactRemoteStateRecord,
    key: [u8; 32],
) -> (T, RemoteStateRecord) {
    let record = RemoteStateRecord::try_from(compact).expect("compact record");
    let value = SessionCipher::from_key(key)
        .open(
            &record.envelope,
            system_now_ms(),
            &mut ReplayGuard::default(),
        )
        .expect("open member record");
    (value, record)
}

fn sealed_member_operation(
    credential: &DeviceCredentialResponse,
    operation_id: &str,
    operation: ControlledOperation,
    issued_at_ms: i64,
    ttl_ms: i64,
) -> MemberOperationEnvelopeRequest {
    let envelope = SessionCipher::from_key(credential_key(credential))
        .seal(
            &ControlledOperationRequest {
                operation_id: operation_id.to_owned(),
                operation,
            },
            issued_at_ms,
            ttl_ms,
        )
        .expect("seal member operation");
    MemberOperationEnvelopeRequest {
        envelope: CompactSealedPayload::from(&envelope),
    }
}

fn authenticated_json_request<T: serde::Serialize>(
    method: Method,
    path: &str,
    value: &T,
    credential: &DeviceCredentialResponse,
) -> Request<Body> {
    authenticated_request(
        method,
        path,
        serde_json::to_vec(value).expect("JSON request"),
        credential,
    )
}

fn authenticated_request(
    method: Method,
    path: &str,
    body: Vec<u8>,
    credential: &DeviceCredentialResponse,
) -> Request<Body> {
    let mut request = request(method, path, body);
    request.headers_mut().insert(
        AUTHORIZATION,
        format!("Bearer {}", credential.device_token)
            .parse()
            .expect("authorization"),
    );
    request.headers_mut().insert(
        DEVICE_ID_HEADER,
        credential.device_id.parse().expect("device ID"),
    );
    request.headers_mut().insert(
        TIMESTAMP_HEADER,
        system_now_ms().to_string().parse().expect("timestamp"),
    );
    request
        .headers_mut()
        .insert(NONCE_HEADER, next_nonce().parse().expect("nonce"));
    request
}

fn json_request<T: serde::Serialize>(method: Method, path: &str, value: &T) -> Request<Body> {
    request(
        method,
        path,
        serde_json::to_vec(value).expect("JSON request"),
    )
}

fn request(method: Method, path: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
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

async fn response_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).unwrap_or_else(|error| {
        panic!(
            "invalid JSON response ({status}): {error}; body={}",
            String::from_utf8_lossy(&body)
        )
    })
}

fn state_record(revision: u64) -> RemoteStateRecord {
    RemoteStateRecord {
        revision,
        envelope: SealedPayload {
            version: 1,
            message_id: Uuid::new_v4().to_string(),
            issued_at_ms: 1,
            expires_at_ms: i64::MAX,
            nonce: vec![7; 24],
            ciphertext: vec![42; 128],
        },
    }
}

fn next_nonce() -> String {
    let counter = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0_u8; 24];
    bytes[16..].copy_from_slice(&counter.to_be_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
