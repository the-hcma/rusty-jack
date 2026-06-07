//! User activity detection for daemon wake triggers.

use crate::RustyJackError;
use std::time::Duration;

/// Source of host idle time for the daemon.
pub trait ActivityMonitor {
    fn idle_duration(&self) -> Result<Duration, RustyJackError>;
}

/// Platform idle-time monitor.
#[derive(Debug, Default)]
pub struct PlatformActivityMonitor;

impl ActivityMonitor for PlatformActivityMonitor {
    fn idle_duration(&self) -> Result<Duration, RustyJackError> {
        platform_idle_duration()
    }
}

#[cfg(target_os = "macos")]
fn platform_idle_duration() -> Result<Duration, RustyJackError> {
    let seconds = macos_idle_seconds();
    if seconds.is_finite() && seconds >= 0.0 {
        Ok(Duration::from_secs_f64(seconds))
    } else {
        Err(RustyJackError::AppLaunch(
            "invalid idle time from CGEventSource".into(),
        ))
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn macos_idle_seconds() -> f64 {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
    }

    const COMBINED_SESSION_STATE: i32 = 0;
    const ANY_INPUT_EVENT_TYPE: u32 = 0xFFFF_FFFF;

    // SAFETY: CoreGraphics documents this call as safe from any thread.
    unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT_TYPE) }
}

#[cfg(not(target_os = "macos"))]
fn platform_idle_duration() -> Result<Duration, RustyJackError> {
    Ok(Duration::ZERO)
}

#[must_use]
pub fn parse_hid_idle_duration(output: &str) -> Option<Duration> {
    let nanos = output
        .lines()
        .find_map(|line| line.split_once("HIDIdleTime"))?
        .1
        .split_once('=')?
        .1
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_nanos(nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hid_idle_duration() {
        let output = r#"
    | |   "HIDIdleTime" = 1234567890
    | |   "Other" = 1
"#;

        assert_eq!(
            parse_hid_idle_duration(output),
            Some(Duration::from_nanos(1_234_567_890))
        );
    }

    #[test]
    fn test_parse_hid_idle_duration_missing() {
        assert_eq!(parse_hid_idle_duration("\"Other\" = 1"), None);
    }
}
