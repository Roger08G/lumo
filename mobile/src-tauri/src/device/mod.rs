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

    pub fn require_bound(&self) -> LumoResult<RuntimeProfile> {
        self.profile()?.ok_or(LumoError::Unauthorized)
    }

    pub fn require_controller(&self) -> LumoResult<()> {
        self.require_profile(RuntimeProfile::Controller)
    }

    pub fn require_controlled(&self) -> LumoResult<()> {
        self.require_profile(RuntimeProfile::Controlled)
    }

    pub fn bootstrap_profile(
        &self,
        requested: RuntimeProfile,
    ) -> LumoResult<Option<RuntimeProfile>> {
        let Some(bound) = self.profile()? else {
            return Ok(None);
        };

        match bound {
            RuntimeProfile::Controlled => Ok(Some(RuntimeProfile::Controlled)),
            RuntimeProfile::Controller
                if matches!(
                    requested,
                    RuntimeProfile::Controller | RuntimeProfile::Debug
                ) =>
            {
                Ok(Some(requested))
            }
            RuntimeProfile::Controller | RuntimeProfile::Debug => Err(LumoError::Unauthorized),
        }
    }

    fn require_profile(&self, required: RuntimeProfile) -> LumoResult<()> {
        if self.require_bound()? == required {
            Ok(())
        } else {
            Err(LumoError::Unauthorized)
        }
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

    #[test]
    fn unbound_device_cannot_authorize_privileged_commands() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");

        assert_eq!(binding.require_bound(), Err(LumoError::Unauthorized));
        assert_eq!(binding.require_controller(), Err(LumoError::Unauthorized));
        assert_eq!(binding.require_controlled(), Err(LumoError::Unauthorized));
        assert_eq!(
            binding
                .bootstrap_profile(RuntimeProfile::Debug)
                .expect("unbound bootstrap"),
            None
        );
    }

    #[test]
    fn controller_binding_only_authorizes_supervisor_profiles() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");
        binding
            .bind(RuntimeProfile::Controller)
            .expect("bind controller");

        assert_eq!(binding.require_controller(), Ok(()));
        assert_eq!(binding.require_controlled(), Err(LumoError::Unauthorized));
        assert_eq!(
            binding
                .bootstrap_profile(RuntimeProfile::Controller)
                .expect("controller bootstrap"),
            Some(RuntimeProfile::Controller)
        );
        assert_eq!(
            binding
                .bootstrap_profile(RuntimeProfile::Debug)
                .expect("debug bootstrap"),
            Some(RuntimeProfile::Debug)
        );
        assert_eq!(
            binding.bootstrap_profile(RuntimeProfile::Controlled),
            Err(LumoError::Unauthorized)
        );
    }

    #[test]
    fn controlled_binding_cannot_elevate_its_bootstrap_profile() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");
        binding
            .bind(RuntimeProfile::Controlled)
            .expect("bind controlled");

        assert_eq!(binding.require_controlled(), Ok(()));
        assert_eq!(binding.require_controller(), Err(LumoError::Unauthorized));
        for requested in [
            RuntimeProfile::Controlled,
            RuntimeProfile::Controller,
            RuntimeProfile::Debug,
        ] {
            assert_eq!(
                binding
                    .bootstrap_profile(requested)
                    .expect("controlled bootstrap"),
                Some(RuntimeProfile::Controlled)
            );
        }
    }

    #[test]
    fn debug_binding_is_not_a_persisted_authority() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");
        binding.bind(RuntimeProfile::Debug).expect("bind debug");

        assert_eq!(binding.require_controller(), Err(LumoError::Unauthorized));
        assert_eq!(binding.require_controlled(), Err(LumoError::Unauthorized));
        assert_eq!(
            binding.bootstrap_profile(RuntimeProfile::Debug),
            Err(LumoError::Unauthorized)
        );
    }
}
