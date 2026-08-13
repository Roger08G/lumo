use std::sync::Arc;

use clap::{Parser, Subcommand};
use lumo_core::{
    domain::{CommandStatus, RuntimeProfile, RuntimeState},
    LumoError, LumoResult,
};
use serde_json::json;

use crate::{FixedClock, LocalBackend, MemoryRepository};

use super::shared::{open_backend, print_json, StorageArgs};

#[derive(Debug, Parser)]
#[command(
    name = "lumo-controller",
    version,
    about = "Lumo local controller runtime"
)]
struct ControllerCli {
    #[command(flatten)]
    storage: StorageArgs,
    #[command(subcommand)]
    command: ControllerCommand,
}

#[derive(Debug, Subcommand)]
enum ControllerCommand {
    Snapshot,
    Locate,
    Invite {
        #[arg(long)]
        pin: String,
    },
    DeletePlace {
        #[arg(long)]
        id: String,
        #[arg(long)]
        pin: String,
    },
    SelfTest,
}

pub fn run() -> LumoResult<()> {
    let cli = ControllerCli::parse();
    if matches!(&cli.command, ControllerCommand::SelfTest) {
        return self_test();
    }
    let backend = open_backend(&cli.storage)?;
    match cli.command {
        ControllerCommand::Snapshot => print_json(&backend.snapshot(RuntimeProfile::Controller)?),
        ControllerCommand::Locate => {
            let command_id = backend.request_location()?;
            print_json(&json!({ "status": "queued", "commandId": command_id }))
        }
        ControllerCommand::Invite { pin } => print_json(&backend.create_invitation(&pin)?),
        ControllerCommand::DeletePlace { id, pin } => print_json(&backend.delete_place(&id, &pin)?),
        ControllerCommand::SelfTest => unreachable!(),
    }
}

fn self_test() -> LumoResult<()> {
    let clock = Arc::new(FixedClock::new(1_000));
    let backend = LocalBackend::with_clock(MemoryRepository::new(RuntimeState::default()), clock);
    backend.debug_seed("123456")?;
    if backend.verify_pin("000000") != Err(LumoError::Unauthorized) {
        return Err(LumoError::Storage(
            "controller self-test expected wrong PIN rejection".to_owned(),
        ));
    }
    backend.create_invitation("123456")?;
    let command_id = backend.request_location()?;
    let snapshot = backend.snapshot(RuntimeProfile::Controller)?;
    let queued = snapshot
        .commands
        .iter()
        .any(|command| command.id == command_id && command.status == CommandStatus::Queued);
    if !queued || snapshot.places.len() != 3 {
        return Err(LumoError::Storage(
            "controller self-test state mismatch".to_owned(),
        ));
    }
    print_json(&json!({
        "binary": "lumo-controller",
        "status": "ok",
        "checks": ["pin-rejection", "single-use-invite", "locate-queue", "snapshot"]
    }))
}
