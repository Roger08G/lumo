use super::*;

#[tokio::test]
async fn public_enrollment_rejects_oversized_bodies_before_allocating_bootstrap_work() {
    let directory = tempdir().expect("directory");
    let database_path = directory.path().join("api.sqlite3");
    let app = build_app(&test_config(&database_path)).expect("app");
    let oversized = vec![b' '; 16 * 1_024 + 1];
    for path in [
        lumo_protocol::GROUPS_PATH.to_owned(),
        invitation_consume_path(&Uuid::new_v4().to_string()),
    ] {
        let mut request = request(Method::POST, &path, oversized.clone());
        request.extensions_mut().insert(ConnectInfo(
            "192.0.2.48:3000".parse::<SocketAddr>().expect("peer"),
        ));
        let response = app.clone().oneshot(request).await.expect("oversized body");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
    let database = rusqlite::Connection::open(database_path).expect("database");
    let reservations: u32 = database
        .query_row("SELECT COUNT(*) FROM bootstrap_requests_v2", [], |row| {
            row.get(0)
        })
        .expect("bootstrap reservations");
    assert_eq!(reservations, 0);
}

async fn replacement_invitation(
    app: &Router,
    controller: &DeviceCredentialResponse,
    target: &DeviceCredentialResponse,
) -> InvitationResponse {
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::POST,
            &group_invitations_path(&controller.group_id),
            &CreateInvitationRequest {
                pin: PIN.to_owned(),
                role: DeviceRole::Controlled,
                replace_device_id: Some(target.device_id.clone()),
            },
            controller,
        ))
        .await
        .expect("replacement invitation");
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn member_status(app: &Router, credential: &DeviceCredentialResponse) -> StatusCode {
    app.clone()
        .oneshot(authenticated_request(
            Method::GET,
            &group_member_path(&credential.group_id),
            Vec::new(),
            credential,
        ))
        .await
        .expect("member read")
        .status()
}

#[tokio::test]
async fn recovery_preserves_group_and_replaces_only_the_explicit_device_atomically() {
    let directory = tempdir().expect("directory");
    let mut config = test_config(&directory.path().join("api.sqlite3"));
    config.limits.max_devices_per_group = 2;
    let app = build_app(&config).expect("application");
    let controller = create_group(&app, "Controller", "192.0.2.40").await;
    let runtime = seed_runtime_state(&app, &controller).await;
    let original_invitation = create_invitation(&app, &controller).await;
    let original_request = ConsumeInvitationRequest {
        request_id: Uuid::new_v4().to_string(),
        token: original_invitation.token.clone(),
        pin: PIN.to_owned(),
        device_name: "Original phone".to_owned(),
    };
    let original_response = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&original_invitation.invitation_id),
            &original_request,
        ))
        .await
        .expect("original phone");
    assert_eq!(original_response.status(), StatusCode::CREATED);
    let original: DeviceCredentialResponse = response_json(original_response).await;
    let invitation = replacement_invitation(&app, &controller, &original).await;
    let stale_invitation = replacement_invitation(&app, &controller, &original).await;

    // Generating a QR cannot disconnect a working phone.
    assert_eq!(member_status(&app, &original).await, StatusCode::OK);
    let invalid_pin = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&invitation.invitation_id),
            &ConsumeInvitationRequest {
                request_id: Uuid::new_v4().to_string(),
                token: invitation.token.clone(),
                pin: "654321".to_owned(),
                device_name: "New phone".to_owned(),
            },
        ))
        .await
        .expect("wrong PIN");
    assert_eq!(invalid_pin.status(), StatusCode::BAD_REQUEST);
    assert_eq!(member_status(&app, &original).await, StatusCode::OK);

    let consumption = ConsumeInvitationRequest {
        request_id: Uuid::new_v4().to_string(),
        token: invitation.token.clone(),
        pin: PIN.to_owned(),
        device_name: "New phone".to_owned(),
    };
    let path = invitation_consume_path(&invitation.invitation_id);
    let (first, replay) = tokio::join!(
        app.clone()
            .oneshot(json_request(Method::POST, &path, &consumption)),
        app.clone()
            .oneshot(json_request(Method::POST, &path, &consumption)),
    );
    let first = first.expect("replacement");
    let replay = replay.expect("retry");
    assert_eq!(first.status(), StatusCode::CREATED);
    assert_eq!(replay.status(), StatusCode::CREATED);
    let replacement: DeviceCredentialResponse = response_json(first).await;
    let retried: DeviceCredentialResponse = response_json(replay).await;
    assert_eq!(replacement, retried);
    assert_eq!(replacement.group_id, controller.group_id);
    assert_ne!(replacement.state_key, original.state_key);
    assert_eq!(
        member_status(&app, &original).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(member_status(&app, &replacement).await, StatusCode::OK);

    let response = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&stale_invitation.invitation_id),
            &ConsumeInvitationRequest {
                request_id: Uuid::new_v4().to_string(),
                token: stale_invitation.token,
                pin: PIN.to_owned(),
                device_name: "Stale QR phone".to_owned(),
            },
        ))
        .await
        .expect("stale QR");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(member_status(&app, &replacement).await, StatusCode::OK);

    let response = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&original_invitation.invitation_id),
            &original_request,
        ))
        .await
        .expect("old credential replay");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let error: ApiErrorBody = response_json(response).await;
    assert_eq!(error.code, "credential_rejected");

    let response = app
        .oneshot(authenticated_request(
            Method::GET,
            &group_state_path(&controller.group_id),
            Vec::new(),
            &controller,
        ))
        .await
        .expect("preserved organization");
    let state: CompactRemoteStateRecord = response_json(response).await;
    let (persisted, _): (RuntimeState, _) = open_compact(state, credential_key(&controller));
    assert_eq!(persisted, runtime);
}

#[tokio::test]
async fn recovery_requires_controller_role_and_same_group_controlled_target() {
    let directory = tempdir().expect("directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("app");
    let controller = create_group(&app, "Controller", "192.0.2.41").await;
    let other = create_group(&app, "Other group", "192.0.2.42").await;
    let controlled = consume_controlled(
        &app,
        &create_invitation(&app, &controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;

    for (actor, role, target, expected) in [
        (
            &controlled,
            DeviceRole::Controlled,
            &controlled.device_id,
            StatusCode::FORBIDDEN,
        ),
        (
            &controller,
            DeviceRole::Controller,
            &controlled.device_id,
            StatusCode::BAD_REQUEST,
        ),
        (
            &controller,
            DeviceRole::Controlled,
            &controller.device_id,
            StatusCode::CONFLICT,
        ),
        (
            &controller,
            DeviceRole::Controlled,
            &other.device_id,
            StatusCode::CONFLICT,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(authenticated_json_request(
                Method::POST,
                &group_invitations_path(&controller.group_id),
                &CreateInvitationRequest {
                    pin: PIN.to_owned(),
                    role,
                    replace_device_id: Some(target.clone()),
                },
                actor,
            ))
            .await
            .expect("unauthorized replacement");
        assert_eq!(response.status(), expected);
    }
    assert_eq!(
        member_status(&app, &controlled).await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn failed_replacement_rolls_back_revocation_and_invitation_consumption() {
    let directory = tempdir().expect("directory");
    let database_path = directory.path().join("api.sqlite3");
    let app = build_app(&test_config(&database_path)).expect("app");
    let controller = create_group(&app, "Controller", "192.0.2.43").await;
    let controlled = consume_controlled(
        &app,
        &create_invitation(&app, &controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;
    let invitation = replacement_invitation(&app, &controller, &controlled).await;
    let database = rusqlite::Connection::open(&database_path).expect("database");
    database
        .execute_batch(
            "CREATE TRIGGER fail_replacement BEFORE INSERT ON devices_v2
             BEGIN SELECT RAISE(ABORT, 'injected test storage failure'); END;",
        )
        .expect("inject transaction failure");
    let consumption = ConsumeInvitationRequest {
        request_id: Uuid::new_v4().to_string(),
        token: invitation.token,
        pin: PIN.to_owned(),
        device_name: "Replacement".to_owned(),
    };
    let path = invitation_consume_path(&invitation.invitation_id);
    let response = app
        .clone()
        .oneshot(json_request(Method::POST, &path, &consumption))
        .await
        .expect("failed replacement");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        member_status(&app, &controlled).await,
        StatusCode::NO_CONTENT
    );
    database
        .execute_batch("DROP TRIGGER fail_replacement;")
        .expect("remove failure");
    let response = app
        .clone()
        .oneshot(json_request(Method::POST, &path, &consumption))
        .await
        .expect("retry after failure");
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        member_status(&app, &controlled).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn revoked_controller_cannot_admit_members_or_write_after_prior_authentication() {
    let directory = tempdir().expect("directory");
    let database_path = directory.path().join("api.sqlite3");
    let app = build_app(&test_config(&database_path)).expect("app");
    let owner = create_group(&app, "Owner", "192.0.2.44").await;
    let controller = consume_controlled(
        &app,
        &create_invitation_with_role(&app, &owner, DeviceRole::Controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;
    let invitation = create_invitation(&app, &controller).await;
    let master = MasterKey::new(MASTER_KEY).expect("master key");
    let store = lumo_api::storage::ApiStore::open(&database_path, &master).expect("store");
    let actor = store
        .authenticate_device_mutation_v2(
            &master,
            &owner.group_id,
            &controller.device_id,
            &controller.device_token,
            &next_nonce(),
            system_now_ms(),
        )
        .expect("authentication before revocation");
    let response = app
        .clone()
        .oneshot(authenticated_json_request(
            Method::DELETE,
            &group_device_path(&owner.group_id, &controller.device_id),
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &owner,
        ))
        .await
        .expect("revoke controller");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(matches!(
        store.compare_and_swap_v2(
            &owner.group_id,
            &actor.device_id,
            None,
            &state_record(1),
            system_now_ms()
        ),
        Err(lumo_core::LumoError::CredentialRejected)
    ));
    assert!(matches!(
        store.load_state_v2(&owner.group_id, &actor.device_id),
        Err(lumo_core::LumoError::CredentialRejected)
    ));
    let response = app
        .oneshot(json_request(
            Method::POST,
            &invitation_consume_path(&invitation.invitation_id),
            &ConsumeInvitationRequest {
                request_id: Uuid::new_v4().to_string(),
                token: invitation.token,
                pin: PIN.to_owned(),
                device_name: "Unauthorized member".to_owned(),
            },
        ))
        .await
        .expect("revoked issuer invitation");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn corrupt_persisted_state_does_not_report_revoked_device_credentials() {
    let directory = tempdir().expect("directory");
    let database_path = directory.path().join("api.sqlite3");
    let app = build_app(&test_config(&database_path)).expect("app");
    let controller = create_group(&app, "Controller", "192.0.2.45").await;
    seed_runtime_state(&app, &controller).await;
    let controlled = consume_controlled(
        &app,
        &create_invitation(&app, &controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;
    let database = rusqlite::Connection::open(&database_path).expect("database");
    let payload: Vec<u8> = database
        .query_row(
            "SELECT payload FROM group_state_v2 WHERE group_id = ?1",
            [&controller.group_id],
            |row| row.get(0),
        )
        .expect("state");
    let mut record: RemoteStateRecord = serde_json::from_slice(&payload).expect("record");
    record.envelope.ciphertext[0] ^= 1;
    database
        .execute(
            "UPDATE group_state_v2 SET payload = ?2 WHERE group_id = ?1",
            rusqlite::params![
                controller.group_id,
                serde_json::to_vec(&record).expect("payload")
            ],
        )
        .expect("corrupt encrypted state");
    let response = app
        .clone()
        .oneshot(authenticated_request(
            Method::GET,
            &group_member_path(&controller.group_id),
            Vec::new(),
            &controlled,
        ))
        .await
        .expect("member failure");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let error: ApiErrorBody = response_json(response).await;
    assert_eq!(error.code, "internal_error");
    let response = app
        .oneshot(authenticated_json_request(
            Method::POST,
            &group_verify_pin_path(&controller.group_id),
            &ProtectedActionRequest {
                pin: PIN.to_owned(),
            },
            &controlled,
        ))
        .await
        .expect("valid credential retained");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn distinct_replacement_invitations_racing_cannot_replace_each_other() {
    let directory = tempdir().expect("directory");
    let app = build_app(&test_config(&directory.path().join("api.sqlite3"))).expect("app");
    let controller = create_group(&app, "Controller", "192.0.2.46").await;
    let controlled = consume_controlled(
        &app,
        &create_invitation(&app, &controller).await,
        Uuid::new_v4().to_string(),
    )
    .await;
    let first = replacement_invitation(&app, &controller, &controlled).await;
    let second = replacement_invitation(&app, &controller, &controlled).await;
    let make_request = |invitation: InvitationResponse| {
        json_request(
            Method::POST,
            &invitation_consume_path(&invitation.invitation_id),
            &ConsumeInvitationRequest {
                request_id: Uuid::new_v4().to_string(),
                token: invitation.token,
                pin: PIN.to_owned(),
                device_name: "Concurrent replacement".to_owned(),
            },
        )
    };
    let (first, second) = tokio::join!(
        app.clone().oneshot(make_request(first)),
        app.clone().oneshot(make_request(second)),
    );
    let first = first.expect("first contender");
    let second = second.expect("second contender");
    let (success, rejected) = if first.status() == StatusCode::CREATED {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(success.status(), StatusCode::CREATED);
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let winner: DeviceCredentialResponse = response_json(success).await;
    assert_eq!(member_status(&app, &winner).await, StatusCode::NO_CONTENT);
    assert_eq!(
        member_status(&app, &controlled).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn schema_v5_migration_preserves_invitation_and_invalid_tokens_cannot_lock_its_pin() {
    let directory = tempdir().expect("directory");
    let database_path = directory.path().join("api.sqlite3");
    let config = test_config(&database_path);
    let app = build_app(&config).expect("app");
    let controller = create_group(&app, "Controller", "192.0.2.47").await;
    let invitation = create_invitation(&app, &controller).await;
    drop(app);
    let database = rusqlite::Connection::open(&database_path).expect("database");
    database
        .execute_batch(
            "ALTER TABLE invitations_v2 DROP COLUMN replace_device_id; PRAGMA user_version = 5;",
        )
        .expect("simulate deployed v5 schema");
    drop(database);
    let migrated = build_app(&config).expect("migrate v5");
    for _ in 0..6 {
        let response = migrated
            .clone()
            .oneshot(json_request(
                Method::POST,
                &invitation_consume_path(&invitation.invitation_id),
                &ConsumeInvitationRequest {
                    request_id: Uuid::new_v4().to_string(),
                    token: URL_SAFE_NO_PAD.encode([0_u8; 32]),
                    pin: PIN.to_owned(),
                    device_name: "Invalid token".to_owned(),
                },
            ))
            .await
            .expect("invalid token");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let controlled = consume_controlled(&migrated, &invitation, Uuid::new_v4().to_string()).await;
    assert_eq!(controlled.group_id, controller.group_id);
    assert_eq!(
        member_status(&migrated, &controlled).await,
        StatusCode::NO_CONTENT
    );
}
