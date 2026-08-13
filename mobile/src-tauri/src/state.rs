use lumo_runtime::{ConfiguredRepository, LocalBackend, RuntimeMode};

use crate::device::DeviceBinding;

pub struct BackendState(
    pub LocalBackend<ConfiguredRepository>,
    pub DeviceBinding,
    pub RuntimeMode,
);
