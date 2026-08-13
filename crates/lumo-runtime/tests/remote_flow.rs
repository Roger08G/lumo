use std::{path::PathBuf, sync::Arc};

use lumo_api::{build_app, config::ApiConfig};
use lumo_core::{
    application::ReportLocationInput,
    domain::{CommandStatus, RuntimeProfile},
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
