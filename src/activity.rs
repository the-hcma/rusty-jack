//! User activity detection for daemon wake triggers.

use crate::RustyJackError;
#[cfg(target_os = "macos")]
use std::process::Command;
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
    let output = Command::new("ioreg")
        .args(["-c", "IOHIDSystem"])
        .output()
        .map_err(RustyJackError::Io)?;
    if !output.status.success() {
        return Err(RustyJackError::AppLaunch(
            "failed to read macOS HID idle time with ioreg".into(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hid_idle_duration(&stdout)
        .ok_or_else(|| RustyJackError::AppLaunch("ioreg output did not include HIDIdleTime".into()))
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
