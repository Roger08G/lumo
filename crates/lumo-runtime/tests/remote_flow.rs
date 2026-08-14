use std::{path::PathBuf, sync::Arc};

use lumo_api::{build_app, config::ApiConfig};
use lumo_core::{
    application::{CreateGroupInput, CreatePlaceInput, ReportLocationInput},
    domain::{CommandStatus, PlaceIcon, PlaceKind, PlaceTone, RuntimeProfile},
};
use lumo_runtime::{FixedClock, LocalBackend, RemoteRepository};
use tempfile::tempdir;
use tokio::net::TcpListener;
use zeroize::Zeroizing;

const PASSWORD: &str = "remote-test-password-with-entropy";

#[tokio::test(flavor = "multi_thread")]
async fn remote_repository_connects_controller_and_controlled_clients() {
    let directory = tempdir().expect("temporary directory");
    let config = ApiConfig {
        bind: "127.0.0.1:0".parse().expect("bind"),
        database_path: directory.path().join("api.sqlite3"),
        tls_cert_path: PathBuf::new(),
        tls_key_path: PathBuf::new(),
        password: Zeroizing::new(PASSWORD.to_owned()),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("address");
    let app = build_app(&config).expect("app");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });
    let base_url = format!("http://{address}");

    tokio::task::spawn_blocking(move || {
        let controller = LocalBackend::with_clock(
            RemoteRepository::new(&base_url, PASSWORD, true).expect("controller repository"),
            Arc::new(FixedClock::new(system_now_ms())),
        );
        controller.debug_seed("123456").expect("seed remote state");
        let command_id = controller.request_location().expect("locate");

        let controlled = LocalBackend::with_clock(
            RemoteRepository::new(&base_url, PASSWORD, true).expect("controlled repository"),
            Arc::new(FixedClock::new(system_now_ms())),
        );
        controlled
            .report_location(ReportLocationInput {
                latitude: 40.4191,
                longitude: -3.7072,
                accuracy_m: 8.0,
                battery_percent: 70,
                captured_at_ms: None,
            })
            .expect("report location");

        let snapshot = controller
            .snapshot(RuntimeProfile::Controller)
            .expect("remote snapshot");
        assert!(snapshot.controlled.last_location.is_some());
        assert!(snapshot.commands.iter().any(|command| {
            command.id == command_id && command.status == CommandStatus::Completed
        }));

        let previous_revision = snapshot.revision;
        controller.reset().expect("reset remote state");
        let empty = controller
            .snapshot(RuntimeProfile::Controller)
            .expect("empty remote snapshot");
        assert!(empty.session.is_none());
        assert!(empty.revision > previous_revision);

        let recreated = controller
            .create_group(
                CreateGroupInput {
                    name: "Familia".into(),
                    supervisor_name: "Supervisor".into(),
                    supervisor_phone: "+34600000001".into(),
                    tracked_person_name: "Persona".into(),
                    tracked_person_phone: "+34600000002".into(),
                    pin: "123456".into(),
                },
                RuntimeProfile::Controller,
            )
            .expect("recreate remote group");
        assert!(recreated.session.is_some());
        assert!(recreated.revision > empty.revision);

        let created_place = controller
            .create_place(CreatePlaceInput {
                name: "Biblioteca".into(),
                address: "Calle Nueva, 8".into(),
                latitude: 40.416_775,
                longitude: -3.703_79,
                radius_m: 50,
                kind: PlaceKind::Place,
                color: PlaceTone::Blue,
                icon: PlaceIcon::School,
            })
            .expect("create remote place");
        assert_eq!(created_place.name, "Biblioteca");

        let updated_place = controller
            .update_place(
                &created_place.id,
                CreatePlaceInput {
                    name: "Biblioteca central".into(),
                    address: created_place.address.clone(),
                    latitude: created_place.latitude,
                    longitude: created_place.longitude,
                    radius_m: 50,
                    kind: created_place.kind,
                    color: created_place.color,
                    icon: created_place.icon,
                },
            )
            .expect("update remote place");
        assert_eq!(updated_place.name, "Biblioteca central");
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
