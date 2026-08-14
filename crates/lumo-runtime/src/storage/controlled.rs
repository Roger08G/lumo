use lumo_core::{domain::AppSnapshot, LumoResult};
use lumo_protocol::{ControlledOperation, ControlledOperationResponse};

/// Optional least-privilege transport used by a controlled device.
///
/// Local repositories return `None` and keep using the in-process domain service. A remote
/// repository returns `Some` and must route the request through the member-only API instead of
/// exposing the canonical group state.
pub trait ControlledOperationPort {
    fn load_controlled_snapshot(&self) -> LumoResult<Option<AppSnapshot>> {
        Ok(None)
    }

    fn apply_controlled_operation(
        &self,
        _operation: ControlledOperation,
    ) -> LumoResult<Option<ControlledOperationResponse>> {
        Ok(None)
    }
}
