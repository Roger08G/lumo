use std::sync::Arc;

use clap::{Parser, Subcommand};
use lumo_core::{
    domain::{RuntimeProfile, RuntimeState},
    LumoError, LumoResult,
};
use serde_json::json;

use crate::{simulation::SimulationScenario, FixedClock, LocalBackend, MemoryRepository};

use super::shared::{open_backend, print_json, StorageArgs};

#[derive(Debug, Parser)]
#[command(
    name = "lumo-debug",
    version,
    about = "Lumo deterministic local debug runtime"
)]
struct DebugCli {
    #[command(flatten)]
    storage: StorageArgs,
    #[command(subcommand)]
    command: DebugCommand,
}

#[derive(Debug, Subcommand)]
enum DebugCommand {
    Seed {
        #[arg(long)]
        pin: String,
    },
    Scenario {
        scenario: SimulationScenario,
    },
    Snapshot,
    Reset,
    SelfTest,
}

pub fn run() -> LumoResult<()> {
    let cli = DebugCli::parse();
    if matches!(&cli.command, DebugCommand::SelfTest) {
        return self_test();
    }
    let backend = open_backend(&cli.storage)?;
    match cli.command {
        DebugCommand::Seed { pin } => print_json(&backend.debug_seed(&pin)?),
        DebugCommand::Scenario { scenario } => print_json(&backend.debug_scenario(scenario)?),
        DebugCommand::Snapshot => print_json(&backend.snapshot(RuntimeProfile::Debug)?),
        DebugCommand::Reset => {
            backend.reset()?;
            print_json(&json!({ "status": "reset" }))
        }
        DebugCommand::SelfTest => unreachable!(),
    }
}

fn self_test() -> LumoResult<()> {
    let clock = Arc::new(FixedClock::new(3_000));
    let backend = LocalBackend::with_clock(MemoryRepository::new(RuntimeState::default()), clock);
    backend.debug_seed("123456")?;
    for scenario in [
        SimulationScenario::Home,
        SimulationScenario::Supermarket,
        SimulationScenario::Medical,
        SimulationScenario::Away,
        SimulationScenario::Offline,
        SimulationScenario::Permission,
        SimulationScenario::Battery,
        SimulationScenario::Help,
    ] {
        backend.debug_scenario(scenario)?;
    }
    let snapshot = backend.snapshot(RuntimeProfile::Debug)?;
    if snapshot.session.is_none() || snapshot.places.len() != 3 || snapshot.events.is_empty() {
        return Err(LumoError::Storage(
            "debug self-test state mismatch".to_owned(),
        ));
    }
    print_json(&json!({
        "binary": "lumo-debug",
        "status": "ok",
        "checks": ["seed", "home", "supermarket", "medical", "away", "offline", "permission", "battery", "help"]
    }))
}
