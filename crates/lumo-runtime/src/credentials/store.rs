use std::{
    fmt, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use lumo_core::{LumoError, LumoResult};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{DeviceCredential, StoredDeviceCredential};

#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    path: Arc<PathBuf>,
}

impl FileCredentialStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Arc::new(path.into()),
        }
    }

    pub fn load(&self) -> LumoResult<Option<StoredDeviceCredential>> {
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
        match fs::remove_file(self.path.as_ref()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
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
    file.write_all(bytes).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)
}

fn replace_file(temporary: &Path, destination: &Path) -> LumoResult<()> {
    if destination.exists() {
        fs::remove_file(destination).map_err(storage_error)?;
    }
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
        store.clear().expect("clear credential");
        assert!(store.load().expect("empty after clear").is_none());
    }
}
