use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use lumo_core::{LumoError, LumoResult};
use lumo_protocol::RemoteStateRecord;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::crypto::MasterKey;

mod v2;

pub use v2::{
    AuthenticatedDevice, ConsumeInvitation, ConsumedInvitation, Idempotent, MemberOperationResult,
    MemberSnapshotResult, NewDevice, NewGroup, NewInvitation,
};

#[derive(Clone)]
pub struct ApiStore {
    connection: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for ApiStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ApiStore").finish_non_exhaustive()
    }
}

impl ApiStore {
    pub fn open(path: impl AsRef<Path>, master: &MasterKey) -> LumoResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(storage_error)?;
        }
        let connection = Connection::open(path).map_err(storage_error)?;
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(storage_error)?;
        connection.set_prepared_statement_cache_capacity(16);
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA journal_size_limit = 16777216;
                 PRAGMA wal_autocheckpoint = 256;
                 PRAGMA temp_store = MEMORY;
                 CREATE TABLE IF NOT EXISTS remote_state (
                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                    revision INTEGER NOT NULL,
                    payload BLOB NOT NULL
                 );",
            )
            .map_err(storage_error)?;
        v2::migrate(&connection)?;
        v2::verify_or_initialize_master(&connection, master)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn load(&self) -> LumoResult<Option<RemoteStateRecord>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| LumoError::Storage("API database lock poisoned".to_owned()))?;
        let encoded: Option<Vec<u8>> = connection
            .query_row(
                "SELECT payload FROM remote_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let record: Option<RemoteStateRecord> = encoded
            .map(|bytes| {
                serde_json::from_slice(&bytes)
                    .map_err(|error| LumoError::Storage(format!("invalid state record: {error}")))
            })
            .transpose()?;
        if let Some(record) = &record {
            record.validate().map_err(|_| {
                LumoError::Storage("persisted state record failed validation".to_owned())
            })?;
        }
        Ok(record)
    }

    pub fn healthcheck(&self) -> LumoResult<()> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| LumoError::Storage("API database lock poisoned".to_owned()))?;
        connection
            .query_row(
                "SELECT COUNT(*) FROM remote_state WHERE singleton = 1",
                [],
                |_row| Ok(()),
            )
            .map_err(storage_error)
    }

    pub fn compare_and_swap(
        &self,
        expected_revision: Option<u64>,
        record: &RemoteStateRecord,
    ) -> LumoResult<bool> {
        record.validate()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| LumoError::Storage("API database lock poisoned".to_owned()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let current: Option<u64> = transaction
            .query_row(
                "SELECT revision FROM remote_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if current != expected_revision {
            return Ok(false);
        }
        if record.revision <= current.unwrap_or_default() {
            return Err(LumoError::InvalidInput(
                "state revision must advance monotonically".to_owned(),
            ));
        }
        let encoded = serde_json::to_vec(record)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO remote_state(singleton, revision, payload)
                 VALUES(1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET revision = excluded.revision, payload = excluded.payload",
                params![record.revision, encoded],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(true)
    }
}

fn storage_error(error: impl std::fmt::Display) -> LumoError {
    LumoError::Storage(error.to_string())
}
