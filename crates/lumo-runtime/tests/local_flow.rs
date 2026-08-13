use std::sync::Arc;

use lumo_core::{
    application::ReportLocationInput,
    domain::{CommandStatus, EventKind, RuntimeProfile},
};
use lumo_runtime::{FixedClock, LocalBackend, SqliteRepository};
use tempfile::tempdir;

#[test]
fn controller_controlled_and_debug_share_an_encrypted_local_state() {
    let directory = tempdir().expect("temporary data directory");
    let debug_clock = Arc::new(FixedClock::new(1_000));
    let controller_clock = Arc::new(FixedClock::new(2_000));
    let controlled_clock = Arc::new(FixedClock::new(3_000));

    let debug = LocalBackend::with_clock(
        SqliteRepository::open(directory.path()).expect("debug repository"),
        debug_clock,
    );
    debug.debug_seed("123456").expect("seed");

    let controller = LocalBackend::with_clock(
        SqliteRepository::open(directory.path()).expect("controller repository"),
        controller_clock,
    );
    let command_id = controller.request_location().expect("locate command");

    let controlled = LocalBackend::with_clock(
        SqliteRepository::open(directory.path()).expect("controlled repository"),
        controlled_clock,
    );
    controlled
        .report_location(ReportLocationInput {
            latitude: 40.4191,
            longitude: -3.7072,
            accuracy_m: 7.0,
            battery_percent: 71,
            captured_at_ms: None,
        })
        .expect("location report");
    controlled.send_help().expect("help event");

    let snapshot = controller
        .snapshot(RuntimeProfile::Controller)
        .expect("controller snapshot");
    assert!(snapshot.controlled.current_place_id.is_some());
    assert!(snapshot
        .commands
        .iter()
        .any(|command| { command.id == command_id && command.status == CommandStatus::Completed }));
    assert!(snapshot
        .events
        .iter()
        .any(|event| event.kind == EventKind::Arrival));
    assert!(snapshot
        .events
        .iter()
        .any(|event| event.kind == EventKind::Help));

    drop(controller);
    let restarted = LocalBackend::with_clock(
        SqliteRepository::open(directory.path()).expect("restarted repository"),
        Arc::new(FixedClock::new(4_000)),
    );
    assert!(restarted
        .snapshot(RuntimeProfile::Controller)
        .expect("persisted snapshot")
        .session
        .is_some());
}
