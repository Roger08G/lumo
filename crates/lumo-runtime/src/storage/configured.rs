use lumo_core::{
    domain::{AppSnapshot, Connectivity, RuntimeState},
    ports::StateRepository,
    LumoError, LumoResult,
};
use lumo_protocol::{
    ControlledOperation, ControlledOperationResponse, DeviceRole, DeviceSummary, InvitationResponse,
};

use crate::{
    config::{RuntimeConfig, RuntimeMode},
    ControlledOperationPort, DeviceCredential, RemoteFreshness, RemoteRepository, SqliteRepository,
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

    pub fn remote(&self) -> LumoResult<&RemoteRepository> {
        match self {
            Self::Remote(repository) => Ok(repository),
            Self::Local(_) => Err(LumoError::RemoteUnavailable),
        }
    }

    pub fn install_credential(&self, credential: DeviceCredential) -> LumoResult<()> {
        self.remote()?.install_credential(credential)
    }

    pub fn clear_credential(&self) -> LumoResult<()> {
        self.remote()?.clear_credential()
    }

    pub fn provision_group(
        &self,
        request_id: &str,
        pin: &str,
        device_name: &str,
    ) -> LumoResult<DeviceCredential> {
        self.remote()?.provision_group(request_id, pin, device_name)
    }

    pub fn consume_invitation(
        &self,
        request_id: &str,
        invitation_id: &str,
        token: &str,
        pin: &str,
        device_name: &str,
    ) -> LumoResult<DeviceCredential> {
        self.remote()?
            .consume_invitation(request_id, invitation_id, token, pin, device_name)
    }

    pub fn create_remote_invitation(&self, pin: &str) -> LumoResult<InvitationResponse> {
        self.remote()?.create_invitation(pin)
    }

    pub fn verify_remote_pin(&self, pin: &str) -> LumoResult<()> {
        self.remote()?.verify_pin(pin)
    }

    pub fn list_remote_devices(&self) -> LumoResult<Vec<DeviceSummary>> {
        self.remote()?.list_devices()
    }

    pub fn revoke_remote_device(&self, device_id: &str, pin: &str) -> LumoResult<()> {
        self.remote()?.revoke_device(device_id, pin)
    }

    pub fn leave_remote_group(&self, pin: &str) -> LumoResult<()> {
        self.remote()?.leave_group(pin)
    }

    pub fn delete_remote_group(&self, pin: &str) -> LumoResult<()> {
        self.remote()?.delete_group(pin)
    }
}

impl ControlledOperationPort for ConfiguredRepository {
    fn load_controlled_snapshot(&self) -> LumoResult<Option<AppSnapshot>> {
        match self {
            Self::Local(_) => Ok(None),
            Self::Remote(repository) => {
                let credential = repository
                    .credential()?
                    .ok_or(LumoError::AuthenticationFailed)?;
                if credential.role() == DeviceRole::Controller {
                    return Ok(None);
                }
                repository.load_member_with_freshness().map(|mut loaded| {
                    if loaded.freshness == RemoteFreshness::Stale {
                        loaded.snapshot.controlled.connectivity = Connectivity::Offline;
                    }
                    Some(loaded.snapshot)
                })
            }
        }
    }

    fn apply_controlled_operation(
        &self,
        operation: ControlledOperation,
    ) -> LumoResult<Option<ControlledOperationResponse>> {
        match self {
            Self::Local(_) => Ok(None),
            Self::Remote(repository) => repository.apply_controlled_operation(operation),
        }
    }
}

impl StateRepository for ConfiguredRepository {
    fn load(&self) -> LumoResult<RuntimeState> {
        match self {
            Self::Local(repository) => repository.load(),
            Self::Remote(repository) => repository.load_with_freshness().map(|mut loaded| {
                if loaded.freshness == RemoteFreshness::Stale {
                    loaded.state.controlled.connectivity = Connectivity::Offline;
                }
                loaded.state
            }),
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
