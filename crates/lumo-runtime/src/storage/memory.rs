use std::sync::Mutex;

use lumo_core::{domain::RuntimeState, ports::StateRepository, LumoError, LumoResult};

#[derive(Debug, Default)]
pub struct MemoryRepository {
    state: Mutex<RuntimeState>,
}

impl MemoryRepository {
    pub fn new(state: RuntimeState) -> Self {
        Self {
            state: Mutex::new(state),
        }
    }
}

impl StateRepository for MemoryRepository {
    fn load(&self) -> LumoResult<RuntimeState> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| LumoError::Storage("memory repository lock poisoned".to_owned()))
    }

    fn transact<T, F>(&self, operation: F) -> LumoResult<T>
    where
        F: FnOnce(&mut RuntimeState) -> LumoResult<T>,
    {
        let mut state = self
            .state
            .lock()
            .map_err(|_| LumoError::Storage("memory repository lock poisoned".to_owned()))?;
        operation(&mut state)
    }
}

impl super::ControlledOperationPort for MemoryRepository {}
