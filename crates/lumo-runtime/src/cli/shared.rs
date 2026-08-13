use std::{path::PathBuf, process};

use clap::Args;
use lumo_core::{LumoError, LumoResult};
use serde::Serialize;

use crate::{ConfiguredRepository, LocalBackend, RuntimeConfig, SystemClock};

#[derive(Debug, Clone, Args)]
pub struct StorageArgs {
    #[arg(long, env = "LUMO_DATA_DIR", default_value = ".lumo-data")]
    pub data_dir: PathBuf,
}

pub fn open_backend(storage: &StorageArgs) -> LumoResult<LocalBackend<ConfiguredRepository>> {
    let config = RuntimeConfig::from_env()?.with_data_dir(&storage.data_dir);
    Ok(LocalBackend::new(
        ConfiguredRepository::open(&config)?,
        SystemClock,
    ))
}

pub fn print_json(value: &impl Serialize) -> LumoResult<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| LumoError::Serialization(error.to_string()))?;
    println!("{json}");
    Ok(())
}

pub fn print_failure_and_exit(error: LumoError) -> ! {
    eprintln!("lumo error: {error}");
    process::exit(1)
}
