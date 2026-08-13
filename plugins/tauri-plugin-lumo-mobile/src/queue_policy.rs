pub const RETENTION_MS: i64 = 24 * 60 * 60 * 1_000;
pub const MAX_ENTRIES: usize = 100;

pub fn is_recent(timestamp_ms: i64, now_ms: i64) -> bool {
    timestamp_ms > 0 && (0..=RETENTION_MS).contains(&now_ms.saturating_sub(timestamp_ms))
}

pub fn newest<T>(entries: &mut Vec<T>) {
    let overflow = entries.len().saturating_sub(MAX_ENTRIES);
    if overflow > 0 {
        entries.drain(..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_expired_and_future_entries() {
        let now = 100_000_000;
        assert!(is_recent(now, now));
        assert!(!is_recent(0, now));
        assert!(!is_recent(now - RETENTION_MS - 1, now));
        assert!(!is_recent(now + 1, now));
    }

    #[test]
    fn keeps_only_the_newest_hundred_entries() {
        let mut entries = (0..120).collect::<Vec<_>>();
        newest(&mut entries);
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries, (20..120).collect::<Vec<_>>());
    }
}
