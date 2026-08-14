mod credential_vault;
mod onboarding;

pub use credential_vault::DeviceCredentialVault;
pub use onboarding::PendingOnboardingStore;

use std::{
    fs,
    io::{ErrorKind, Write},
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
        let profile = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(profile) => Some(profile),
                Err(_) => {
                    fs::remove_file(&path).map_err(storage_error)?;
                    None
                }
            },
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
        if let Some(current) = self.profile()? {
            return if current == profile {
                Ok(())
            } else {
                Err(LumoError::Unauthorized)
            };
        }
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(storage_error)?;
        }
        let encoded = serde_json::to_vec(&profile)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        write_binding_atomically(&self.path, &encoded)?;
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

fn write_binding_atomically(path: &Path, bytes: &[u8]) -> LumoResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| LumoError::Storage("device binding path has no parent".to_owned()))?;
    let temporary = parent.join(format!(".device-binding-{}.tmp", uuid::Uuid::new_v4()));
    write_private(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(storage_error(error));
    }
    // A file fsync already makes the binding contents durable. Opening/fsyncing an app-private
    // directory is denied by some Android OEM kernels, so keep the extra directory durability
    // barrier on other Unix targets only.
    #[cfg(all(unix, not(target_os = "android")))]
    {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(storage_error)?;
    }
    Ok(())
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
    fn binding_is_idempotent_but_cannot_change_authority() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let binding = DeviceBinding::open(directory.path().join("device.json")).expect("binding");
        binding
            .bind(RuntimeProfile::Controller)
            .expect("initial binding");
        binding
            .bind(RuntimeProfile::Controller)
            .expect("idempotent binding");
        assert_eq!(
            binding.bind(RuntimeProfile::Controlled),
            Err(LumoError::Unauthorized)
        );
        assert_eq!(
            binding.profile().expect("profile"),
            Some(RuntimeProfile::Controller)
        );
    }

    #[test]
    fn corrupt_binding_is_removed_and_returns_to_onboarding() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("device.json");
        fs::write(&path, b"not-json").expect("corrupt binding");
        let binding = DeviceBinding::open(&path).expect("binding");
        assert_eq!(binding.profile().expect("profile"), None);
        assert!(!path.exists());
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
