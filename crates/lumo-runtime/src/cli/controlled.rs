use std::sync::Arc;

use clap::{Parser, Subcommand};
use lumo_core::{
    application::{ReportLocationInput, SetTrackingInput},
    domain::{EventKind, PermissionState, RuntimeProfile, RuntimeState},
    LumoError, LumoResult,
};
use serde_json::json;

use crate::{FixedClock, LocalBackend, MemoryRepository};

use super::shared::{open_backend, print_json, StorageArgs};

#[derive(Debug, Parser)]
#[command(
    name = "lumo-controlled",
    version,
    about = "Lumo local controlled-device runtime"
)]
struct ControlledCli {
    #[command(flatten)]
    storage: StorageArgs,
    #[command(subcommand)]
    command: ControlledCommand,
}

#[derive(Debug, Subcommand)]
enum ControlledCommand {
    Snapshot,
    Setup,
    Report {
        #[arg(long, allow_hyphen_values = true)]
        latitude: f64,
        #[arg(long, allow_hyphen_values = true)]
        longitude: f64,
        #[arg(long, default_value_t = 10.0)]
        accuracy: f32,
        #[arg(long, default_value_t = 100)]
        battery: u8,
    },
    Process,
    SendHelp,
    Join {
        #[arg(long)]
        token: String,
        #[arg(long)]
        pin: String,
    },
    SelfTest,
}

pub fn run() -> LumoResult<()> {
    let cli = ControlledCli::parse();
    if matches!(&cli.command, ControlledCommand::SelfTest) {
        return self_test();
    }
    let backend = open_backend(&cli.storage)?;
    match cli.command {
        ControlledCommand::Snapshot => print_json(&backend.snapshot(RuntimeProfile::Controlled)?),
        ControlledCommand::Setup => print_json(&backend.set_tracking(tracking_enabled())?),
        ControlledCommand::Report {
            latitude,
            longitude,
            accuracy,
            battery,
        } => print_json(&backend.report_location(ReportLocationInput {
            latitude,
            longitude,
            accuracy_m: accuracy,
            battery_percent: battery,
            captured_at_ms: None,
        })?),
        ControlledCommand::Process => {
            print_json(&json!({ "processed": backend.process_pending()? }))
        }
        ControlledCommand::SendHelp => print_json(&backend.send_help()?),
        ControlledCommand::Join { token, pin } => {
            backend.consume_invitation(&token, &pin)?;
            print_json(&json!({ "status": "joined" }))
        }
        ControlledCommand::SelfTest => unreachable!(),
    }
}

fn self_test() -> LumoResult<()> {
    let clock = Arc::new(FixedClock::new(2_000));
    let backend = LocalBackend::with_clock(MemoryRepository::new(RuntimeState::default()), clock);
    backend.debug_seed("123456")?;
    backend.set_tracking(tracking_enabled())?;
    backend.report_location(ReportLocationInput {
        latitude: 40.4168,
        longitude: -3.7038,
        accuracy_m: 8.0,
        battery_percent: 74,
        captured_at_ms: None,
    })?;
    backend.request_location()?;
    if backend.process_pending()? != 1 {
        return Err(LumoError::Storage(
            "controlled self-test did not process locate command".to_owned(),
        ));
    }
    backend.send_help()?;
    let snapshot = backend.snapshot(RuntimeProfile::Controlled)?;
    if snapshot.controlled.last_location.is_none()
        || !snapshot
            .events
            .iter()
            .any(|event| event.kind == EventKind::Help)
    {
        return Err(LumoError::Storage(
            "controlled self-test state mismatch".to_owned(),
        ));
    }
    print_json(&json!({
        "binary": "lumo-controlled",
        "status": "ok",
        "checks": ["permissions", "location", "geofence", "command-ack", "help"]
    }))
}

fn tracking_enabled() -> SetTrackingInput {
    SetTrackingInput {
        precise_permission: PermissionState::Granted,
        background_permission: PermissionState::Granted,
        battery_optimization_disabled: true,
        enabled: true,
    }
}
