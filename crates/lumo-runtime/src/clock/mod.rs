use std::{
    sync::atomic::{AtomicI64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use lumo_core::ports::Clock;

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct FixedClock {
    now_ms: AtomicI64,
}

impl FixedClock {
    pub fn new(now_ms: i64) -> Self {
        Self {
            now_ms: AtomicI64::new(now_ms),
        }
    }

    pub fn set(&self, now_ms: i64) {
        self.now_ms.store(now_ms, Ordering::SeqCst);
    }

    pub fn advance(&self, milliseconds: i64) {
        self.now_ms.fetch_add(milliseconds, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms.load(Ordering::SeqCst)
    }
}
