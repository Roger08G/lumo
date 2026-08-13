use crate::{domain::RuntimeState, LumoResult};

pub trait StateRepository: Send + Sync {
    fn load(&self) -> LumoResult<RuntimeState>;

    fn transact<T, F>(&self, operation: F) -> LumoResult<T>
    where
        F: FnOnce(&mut RuntimeState) -> LumoResult<T>;
}
