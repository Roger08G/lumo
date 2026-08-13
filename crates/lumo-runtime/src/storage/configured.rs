use lumo_core::{domain::RuntimeState, ports::StateRepository, LumoResult};

use crate::{
    config::{RuntimeConfig, RuntimeMode},
    RemoteRepository, SqliteRepository,
};

#[derive(Debug, Clone)]
pub enum ConfiguredRepository {
    Local(SqliteRepository),
    Remote(RemoteRepository),
}

impl ConfiguredRepository {
    pub fn open(config: &RuntimeConfig) -> LumoResult<Self> {
        match config.mode {
            RuntimeMode::Local => Ok(Self::Local(SqliteRepository::open(&config.data_dir)?)),
            RuntimeMode::Remote => Ok(Self::Remote(RemoteRepository::from_config(config)?)),
        }
    }
}

impl StateRepository for ConfiguredRepository {
    fn load(&self) -> LumoResult<RuntimeState> {
        match self {
            Self::Local(repository) => repository.load(),
            Self::Remote(repository) => repository.load(),
        }
    }

    fn transact<T, F>(&self, operation: F) -> LumoResult<T>
    where
        F: FnOnce(&mut RuntimeState) -> LumoResult<T>,
    {
        match self {
            Self::Local(repository) => repository.transact(operation),
            Self::Remote(repository) => repository.transact(operation),
        }
    }
}
