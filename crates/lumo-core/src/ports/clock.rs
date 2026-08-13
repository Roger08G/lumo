pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}
