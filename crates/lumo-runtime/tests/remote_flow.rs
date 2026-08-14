use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use lumo_api::{
    build_app,
    config::{ApiConfig, ApiLimits},
};
use lumo_core::{
    application::{CreateGroupInput, ReportLocationInput, SetTrackingInput},
    domain::{CommandStatus, PermissionState, RuntimeProfile},
    LumoError,
};
use lumo_runtime::{FixedClock, LocalBackend, RemoteRepository};
use tempfile::tempdir;
use tokio::net::TcpListener;
use uuid::Uuid;
use zeroize::Zeroizing;

#[tokio::test(flavor = "multi_thread")]
async fn v2_device_credentials_connect_and_revoke_controller_and_controlled_clients() {
    let directory = tempdir().expect("temporary directory");
    let config = ApiConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        database_path: directory.path().join("api.sqlite3"),
        tls_cert_path: PathBuf::new(),
        tls_key_path: PathBuf::new(),
        master_key: Zeroizing::new(
            "runtime-integration-master-key-with-sufficient-entropy".to_owned(),
        ),
        enable_legacy_v1: false,
        legacy_password: None,
        trust_proxy_headers: false,
        limits: ApiLimits::default(),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("address");
    let app = build_app(&config).expect("app");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("server");
    });
    let base_url = format!("http://{address}");

    tokio::task::spawn_blocking(move || {
        let pin = "123456";
        let now_ms = system_now_ms();
        let controller_repository =
            RemoteRepository::new(&base_url, None, true).expect("controller repository");
        let create_request_id = Uuid::new_v4().to_string();
        let controller_credential = controller_repository
            .provision_group(&create_request_id, pin, "Controlador")
            .expect("provision controller");
        let replayed_controller_credential = controller_repository
            .provision_group(&create_request_id, pin, "Controlador")
            .expect("replay controller provisioning");
        assert_same_credential(&replayed_controller_credential, &controller_credential);
        let controller = LocalBackend::with_clock(
            controller_repository.clone(),
            Arc::new(FixedClock::new(now_ms)),
        );
        controller
            .create_group(
                CreateGroupInput {
                    name: "Grupo".into(),
                    supervisor_name: "Controlador".into(),
                    supervisor_phone: "+34600000001".into(),
                    tracked_person_name: "Persona".into(),
                    tracked_person_phone: "+34600000002".into(),
                    pin: pin.into(),
                },
                RuntimeProfile::Controller,
            )
            .expect("initialize group state");

        let revision_before_failed_pin = controller
            .snapshot(RuntimeProfile::Controller)
            .expect("snapshot before PIN failure")
            .revision;
        assert_eq!(
            controller.verify_pin("000000"),
            Err(LumoError::Unauthorized)
        );
        let revision_after_failed_pin = controller
            .snapshot(RuntimeProfile::Controller)
            .expect("snapshot after PIN failure")
            .revision;
        assert!(revision_after_failed_pin > revision_before_failed_pin);
        for _ in 0..4 {
            assert_eq!(
                controller.verify_pin("000000"),
                Err(LumoError::Unauthorized)
            );
        }
        assert_eq!(controller.verify_pin(pin), Err(LumoError::RateLimited));

        let invitation = controller_repository
            .create_invitation(pin)
            .expect("create invitation");
        let controlled_repository =
            RemoteRepository::new(&base_url, None, true).expect("controlled repository");
        let consume_request_id = Uuid::new_v4().to_string();
        let controlled_credential = controlled_repository
            .consume_invitation(
                &consume_request_id,
                &invitation.invitation_id,
                &invitation.token,
                pin,
                "Controlado",
            )
            .expect("consume invitation");
        let replayed_controlled_credential = controlled_repository
            .consume_invitation(
                &consume_request_id,
                &invitation.invitation_id,
                &invitation.token,
                pin,
                "Controlado",
            )
            .expect("replay invitation consumption");
        assert_same_credential(&replayed_controlled_credential, &controlled_credential);
        assert_eq!(
            controlled_repository.load_with_freshness(),
            Err(LumoError::Unauthorized)
        );
        let controlled = LocalBackend::with_clock(
            controlled_repository.clone(),
            Arc::new(FixedClock::new(now_ms.saturating_add(1_000))),
        );

        let command_id = controller.request_location().expect("locate command");
        controlled
            .set_tracking(SetTrackingInput {
                precise_permission: PermissionState::Granted,
                background_permission: PermissionState::Granted,
                battery_optimization_disabled: true,
                enabled: true,
            })
            .expect("enable tracking");
        controlled
            .report_location(ReportLocationInput {
                latitude: 40.4191,
                longitude: -3.7072,
                accuracy_m: 8.0,
                battery_percent: 70,
                captured_at_ms: Some(now_ms.saturating_add(1_000)),
            })
            .expect("report location");
        let member_snapshot = controlled
            .snapshot(RuntimeProfile::Controlled)
            .expect("member snapshot");
        assert!(member_snapshot.places.is_empty());
        assert!(member_snapshot.events.is_empty());
        assert!(member_snapshot.commands.is_empty());

        let snapshot = controller
            .snapshot(RuntimeProfile::Controller)
            .expect("controller snapshot");
        assert!(snapshot.controlled.last_location.is_some());
        assert!(snapshot.commands.iter().any(|command| {
            command.id == command_id && command.status == CommandStatus::Completed
        }));
        let devices = controller_repository.list_devices().expect("list devices");
        assert_eq!(devices.len(), 2);
        assert!(devices
            .iter()
            .any(|device| device.device_id == controlled_credential.device_id()));

        controlled_repository
            .leave_group(pin)
            .expect("controlled leaves");
        assert!(controlled_repository
            .credential()
            .expect("credential slot")
            .is_none());
        let devices = controller_repository
            .list_devices()
            .expect("list revoked device");
        assert!(devices.iter().any(|device| {
            device.device_id == controlled_credential.device_id() && device.revoked_at_ms.is_some()
        }));

        assert_eq!(
            controller_credential.device_id(),
            controller_repository
                .credential()
                .expect("controller slot")
                .expect("controller credential")
                .device_id()
        );
        controller_repository
            .delete_group(pin)
            .expect("delete group");
        assert!(controller_repository
            .credential()
            .expect("credential slot")
            .is_none());
    })
    .await
    .expect("blocking client task");
    server.abort();
}

fn system_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn assert_same_credential(
    actual: &lumo_runtime::DeviceCredential,
    expected: &lumo_runtime::DeviceCredential,
) {
    let actual = actual.to_stored();
    let expected = expected.to_stored();
    assert_eq!(&actual.api_origin, &expected.api_origin);
    assert_eq!(&actual.group_id, &expected.group_id);
    assert_eq!(&actual.device_id, &expected.device_id);
    assert_eq!(actual.role, expected.role);
    assert_eq!(&actual.device_token, &expected.device_token);
    assert_eq!(&actual.state_key, &expected.state_key);
}
