//! Volume set/verify outcome (platform-independent).

use serde::Serialize;
use std::time::Duration;

/// Outcome of setting volume with verification/retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VolumeEnsureResult {
    pub target: u8,
    pub actual: Option<u8>,
    pub verified: bool,
    pub attempts: u32,
}

/// Retry timing when verifying volume (eqMac may reset level briefly after route changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeEnsureOptions {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub retry_delay: Duration,
    pub tolerance: u8,
}

impl Default for VolumeEnsureOptions {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            initial_delay: Duration::from_millis(100),
            retry_delay: Duration::from_millis(200),
            tolerance: 1,
        }
    }
}

impl VolumeEnsureOptions {
    #[must_use]
    pub const fn fast() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(0),
            retry_delay: Duration::from_millis(0),
            tolerance: 1,
        }
    }
}

#[must_use]
pub fn volume_within_tolerance(actual: u8, target: u8, tolerance: u8) -> bool {
    u16::from(actual.abs_diff(target)) <= u16::from(tolerance)
}

#[cfg(test)]
mod tests {
    use super::volume_within_tolerance;

    #[test]
    fn test_volume_within_tolerance() {
        assert!(volume_within_tolerance(13, 13, 0));
        assert!(volume_within_tolerance(12, 13, 1));
        assert!(!volume_within_tolerance(10, 13, 1));
    }
}
