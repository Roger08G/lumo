use std::{collections::VecDeque, sync::Mutex};

use lumo_core::{security::SealedPayload, LumoError, LumoResult};

pub trait RemoteTransport: Send + Sync {
    fn send(&self, message: SealedPayload) -> LumoResult<()>;
    fn receive(&self) -> LumoResult<Option<SealedPayload>>;
}

#[derive(Debug)]
pub struct MemoryTransport {
    queue: Mutex<VecDeque<SealedPayload>>,
    capacity: usize,
}

impl MemoryTransport {
    pub fn new(capacity: usize) -> LumoResult<Self> {
        if capacity == 0 {
            return Err(LumoError::InvalidInput(
                "transport capacity must be positive".to_owned(),
            ));
        }
        Ok(Self {
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        })
    }
}

impl RemoteTransport for MemoryTransport {
    fn send(&self, message: SealedPayload) -> LumoResult<()> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| LumoError::Storage("transport lock poisoned".to_owned()))?;
        if queue.len() >= self.capacity {
            return Err(LumoError::Storage("transport queue is full".to_owned()));
        }
        queue.push_back(message);
        Ok(())
    }

    fn receive(&self) -> LumoResult<Option<SealedPayload>> {
        self.queue
            .lock()
            .map(|mut queue| queue.pop_front())
            .map_err(|_| LumoError::Storage("transport lock poisoned".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use lumo_core::security::SessionCipher;

    use super::*;

    #[test]
    fn preserves_order_and_applies_backpressure() {
        let transport = MemoryTransport::new(1).expect("transport");
        let cipher = SessionCipher::generate();
        let first = cipher.seal(&1_u8, 0, 100).expect("first");
        let second = cipher.seal(&2_u8, 0, 100).expect("second");
        transport.send(first.clone()).expect("send");
        assert!(transport.send(second).is_err());
        assert_eq!(transport.receive().expect("receive"), Some(first));
    }
}
