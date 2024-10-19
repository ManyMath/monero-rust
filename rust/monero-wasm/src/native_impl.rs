//! Native platform implementations of abstractions

#![cfg(not(target_arch = "wasm32"))]

use crate::abstractions::TimeProvider;
use std::time::{SystemTime, UNIX_EPOCH};
pub struct SystemTimeProvider;

impl SystemTimeProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemTimeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| {
                // If system time is before UNIX epoch, return 0
                // This is safer than panicking and allows the system to continue
                std::time::Duration::from_secs(0)
            })
            .as_secs()
    }

    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| {
                // If system time is before UNIX epoch, return 0
                // This is safer than panicking and allows the system to continue
                std::time::Duration::from_secs(0)
            })
            .as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_time_provider() {
        let provider = SystemTimeProvider::new();
        let now = provider.now();
        let now_ms = provider.now_ms();

        assert!(now > 1_600_000_000);
        assert!(now < 2_000_000_000);
        assert!(now_ms > now * 1000);
        assert!(now_ms < (now + 1) * 1000);
    }
}
