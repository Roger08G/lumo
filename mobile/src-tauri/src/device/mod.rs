use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use lumo_core::{domain::RuntimeProfile, LumoError, LumoResult};

#[derive(Debug, Clone)]
pub struct DeviceBinding {
    path: Arc<PathBuf>,
    profile: Arc<Mutex<Option<RuntimeProfile>>>,
}

impl DeviceBinding {
    pub fn open(path: impl Into<PathBuf>) -> LumoResult<Self> {
        let path = path.into();
        let profile =
            match fs::read(&path) {
                Ok(bytes) => Some(serde_json::from_slice(&bytes).map_err(|error| {
                    LumoError::Storage(format!("invalid device binding: {error}"))
                })?),
                Err(error) if error.kind() == ErrorKind::NotFound => None,
                Err(error) => return Err(storage_error(error)),
            };
        Ok(Self {
            path: Arc::new(path),
            profile: Arc::new(Mutex::new(profile)),
        })
    }

    pub fn profile(&self) -> LumoResult<Option<RuntimeProfile>> {
        self.profile
            .lock()
            .map(|profile| *profile)
            .map_err(|_| LumoError::Storage("device binding lock poisoned".to_owned()))
    }

    pub fn bind(&self, profile: RuntimeProfile) -> LumoResult<()> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(storage_error)?;
        }
        let encoded = serde_json::to_vec(&profile)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        write_private(&self.path, &encoded)?;
        *self
            .profile
            .lock()
            .map_err(|_| LumoError::Storage("device binding lock poisoned".to_owned()))? =
            Some(profile);
        Ok(())
    }

    pub fn clear(&self) -> LumoResult<()> {
        match fs::remove_file(self.path.as_ref()) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(storage_error(error)),
        }
        *self
            .profile
            .lock()
            .map_err(|_| LumoError::Storage("device binding lock poisoned".to_owned()))? = None;
        Ok(())
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> LumoResult<()> {
    fs::write(path, bytes).map_err(storage_error)
}

fn storage_error(error: impl std::fmt::Display) -> LumoError {
    LumoError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_survives_restart_and_can_be_cleared() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("device.json");
        let binding = DeviceBinding::open(&path).expect("binding");
        assert_eq!(binding.profile().expect("profile"), None);
        binding
            .bind(RuntimeProfile::Controlled)
            .expect("bind device");
        assert_eq!(
            DeviceBinding::open(&path)
                .expect("reopen")
                .profile()
                .expect("profile"),
            Some(RuntimeProfile::Controlled)
        );
        binding.clear().expect("clear");
        assert_eq!(binding.profile().expect("profile"), None);
    }
}
