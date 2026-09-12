use std::{
    fmt, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use lumo_core::{LumoError, LumoResult};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{DeviceCredential, StoredDeviceCredential};

#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    path: Arc<PathBuf>,
    lock: Arc<Mutex<()>>,
}

impl FileCredentialStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Arc::new(path.into()),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn load(&self) -> LumoResult<Option<StoredDeviceCredential>> {
        let _guard = self.guard()?;
        let bytes = match fs::read(self.path.as_ref()) {
            Ok(bytes) => Zeroizing::new(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        serde_json::from_slice(bytes.as_slice())
            .map(Some)
            .map_err(|_| LumoError::Storage("invalid device credential".to_owned()))
    }

    pub fn store(&self, credential: &DeviceCredential) -> LumoResult<()> {
        let _guard = self.guard()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| LumoError::Storage("device credential path has no parent".to_owned()))?;
        fs::create_dir_all(parent).map_err(storage_error)?;
        let stored = credential.to_stored();
        let encoded = Zeroizing::new(
            serde_json::to_vec(&stored)
                .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        let temporary = parent.join(format!(".device-credential-{}.tmp", Uuid::new_v4()));
        write_private(&temporary, encoded.as_slice())?;
        replace_file(&temporary, self.path.as_ref())
    }

    pub fn clear(&self) -> LumoResult<()> {
        let _guard = self.guard()?;
        match fs::remove_file(self.path.as_ref()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn guard(&self) -> LumoResult<MutexGuard<'_, ()>> {
        self.lock
            .lock()
            .map_err(|_| LumoError::Storage("device credential lock poisoned".to_owned()))
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> LumoResult<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(storage_error)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result.map_err(storage_error)
}

fn replace_file(temporary: &Path, destination: &Path) -> LumoResult<()> {
    // Same-directory rename replaces an existing file atomically on Windows and Unix.
    // Never unlink the only durable credential before installing its replacement.
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::remove_file(temporary);
        return Err(storage_error(error));
    }
    Ok(())
}

fn storage_error(error: impl fmt::Display) -> LumoError {
    LumoError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use lumo_protocol::DeviceRole;

    use super::*;

    #[test]
    fn private_file_store_round_trips_and_clears() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = FileCredentialStore::new(directory.path().join("device-credential.json"));
        let credential = DeviceCredential::from_parts(
            "https://api.example.test",
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
            DeviceRole::Controller,
            URL_SAFE_NO_PAD.encode([7_u8; 32]),
            URL_SAFE_NO_PAD.encode([9_u8; 32]),
            false,
        )
        .expect("credential");

        assert!(store.load().expect("empty load").is_none());
        store.store(&credential).expect("store credential");
        let restored = store.load().expect("load").expect("stored credential");
        assert_eq!(restored.group_id, credential.group_id());
        assert_eq!(restored.device_id, credential.device_id());
        assert_eq!(restored.role, credential.role());
        store.store(&credential).expect("atomic overwrite");
        assert!(store.load().expect("replacement load").is_some());
        store.clear().expect("clear credential");
        assert!(store.load().expect("empty after clear").is_none());
    }

    #[test]
    fn failed_atomic_replacement_preserves_the_existing_credential() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let destination = directory.path().join("credential.json");
        fs::write(&destination, b"original credential").expect("existing credential");
        assert!(replace_file(&directory.path().join("missing.tmp"), &destination).is_err());
        assert_eq!(
            fs::read(destination).expect("preserved credential"),
            b"original credential"
        );
    }
}
