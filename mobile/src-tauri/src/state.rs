use std::sync::{atomic::AtomicBool, Arc, Mutex};

use lumo_runtime::{ConfiguredRepository, LocalBackend, RuntimeMode};

use crate::device::{DeviceBinding, DeviceCredentialVault, PendingOnboardingStore};

pub struct BackendState {
    pub backend: LocalBackend<ConfiguredRepository>,
    pub binding: DeviceBinding,
    pub mode: RuntimeMode,
    pub repository: ConfiguredRepository,
    pub vault: DeviceCredentialVault,
    pub onboarding: PendingOnboardingStore,
    pub lifecycle: Arc<Mutex<()>>,
    pub restore_failed: Arc<AtomicBool>,
}
