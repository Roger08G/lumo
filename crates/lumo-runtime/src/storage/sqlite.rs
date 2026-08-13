use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use lumo_core::{
    domain::RuntimeState,
    ports::StateRepository,
    security::{ReplayGuard, SealedPayload, SessionCipher},
    LumoError, LumoResult,
};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use zeroize::Zeroizing;

const DATABASE_FILE: &str = "lumo-state.sqlite3";
const KEY_FILE: &str = "lumo-state.key";

#[derive(Clone)]
pub struct SqliteRepository {
    database_path: PathBuf,
    key: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for SqliteRepository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqliteRepository")
            .field("database_path", &self.database_path)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl SqliteRepository {
    pub fn open(data_dir: impl AsRef<Path>) -> LumoResult<Self> {
        let data_dir = data_dir.as_ref();
        fs::create_dir_all(data_dir).map_err(storage_error)?;
        let key = load_or_create_key(&data_dir.join(KEY_FILE))?;
        let repository = Self {
            database_path: data_dir.join(DATABASE_FILE),
            key: Zeroizing::new(key.to_vec()),
        };
        repository.connect()?;
        Ok(repository)
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    fn cipher(&self) -> LumoResult<SessionCipher> {
        let key: [u8; 32] = self
            .key
            .as_slice()
            .try_into()
            .map_err(|_| LumoError::Storage("invalid local key length".to_owned()))?;
        Ok(SessionCipher::from_key(key))
    }

    fn connect(&self) -> LumoResult<Connection> {
        let connection = Connection::open(&self.database_path).map_err(storage_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(storage_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(storage_error)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS runtime_state (
                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                    revision INTEGER NOT NULL,
                    payload BLOB NOT NULL
                );",
            )
            .map_err(storage_error)?;
        Ok(connection)
    }

    fn load_from_connection(&self, connection: &Connection) -> LumoResult<RuntimeState> {
        let encoded: Option<Vec<u8>> = connection
            .query_row(
                "SELECT payload FROM runtime_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let Some(encoded) = encoded else {
            return Ok(RuntimeState::default());
        };
        let envelope: SealedPayload = serde_json::from_slice(&encoded)
            .map_err(|error| LumoError::Storage(format!("invalid encrypted state: {error}")))?;
        self.cipher()?
            .open(&envelope, 0, &mut ReplayGuard::default())
    }

    fn save_to_connection(&self, connection: &Connection, state: &RuntimeState) -> LumoResult<()> {
        let envelope = self.cipher()?.seal(state, 0, i64::MAX)?;
        let encoded = serde_json::to_vec(&envelope)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        connection
            .execute(
                "INSERT INTO runtime_state(singleton, revision, payload)
                 VALUES(1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET revision = excluded.revision, payload = excluded.payload",
                params![state.revision, encoded],
            )
            .map_err(storage_error)?;
        Ok(())
    }
}

impl StateRepository for SqliteRepository {
    fn load(&self) -> LumoResult<RuntimeState> {
        let connection = self.connect()?;
        self.load_from_connection(&connection)
    }

    fn transact<T, F>(&self, operation: F) -> LumoResult<T>
    where
        F: FnOnce(&mut RuntimeState) -> LumoResult<T>,
    {
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let mut state = self.load_from_connection(&transaction)?;
        let original = state.clone();
        let outcome = operation(&mut state);
        if state != original {
            self.save_to_connection(&transaction, &state)?;
        }
        transaction.commit().map_err(storage_error)?;
        outcome
    }
}

fn load_or_create_key(path: &Path) -> LumoResult<[u8; 32]> {
    let mut generated = [0_u8; 32];
    OsRng.fill_bytes(&mut generated);
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(&generated).map_err(storage_error)?;
            file.sync_all().map_err(storage_error)?;
            Ok(generated)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => read_key(path),
        Err(error) => Err(storage_error(error)),
    }
}

fn read_key(path: &Path) -> LumoResult<[u8; 32]> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(storage_error)?;
    bytes
        .try_into()
        .map_err(|_| LumoError::Storage("local key file must contain exactly 32 bytes".to_owned()))
}

fn storage_error(error: impl std::fmt::Display) -> LumoError {
    LumoError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use lumo_core::ports::StateRepository;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn state_persists_without_plaintext_json() {
        let directory = tempdir().expect("tempdir");
        let repository = SqliteRepository::open(directory.path()).expect("repository");
        repository
            .transact(|state| {
                state.revision = 42;
                Ok(())
            })
            .expect("save");
        assert_eq!(repository.load().expect("load").revision, 42);

        let database = fs::read(repository.database_path()).expect("database bytes");
        assert!(!String::from_utf8_lossy(&database).contains("\"revision\":42"));
    }

    #[test]
    fn a_different_key_cannot_open_existing_state() {
        let directory = tempdir().expect("tempdir");
        let repository = SqliteRepository::open(directory.path()).expect("repository");
        repository
            .transact(|state| {
                state.revision = 7;
                Ok(())
            })
            .expect("save");
        fs::remove_file(directory.path().join(KEY_FILE)).expect("remove key");
        let wrong_key_repository = SqliteRepository::open(directory.path()).expect("repository");
        assert!(matches!(
            wrong_key_repository.load(),
            Err(LumoError::AuthenticationFailed)
        ));
    }
}
