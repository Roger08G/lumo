use std::sync::{Arc, Mutex};

use lumo_runtime::{ConfiguredRepository, LocalBackend, RuntimeMode};

use crate::device::{DeviceBinding, DeviceCredentialVault, PendingOnboardingStore};

pub struct BackendState(
    pub LocalBackend<ConfiguredRepository>,
    pub DeviceBinding,
    pub RuntimeMode,
    pub ConfiguredRepository,
    pub DeviceCredentialVault,
    pub PendingOnboardingStore,
    pub Arc<Mutex<()>>,
);
