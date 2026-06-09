//! User activity detection for daemon wake triggers.

use crate::config::Config;
use crate::scalar_webapi_device::{KEYBOARD_TRIGGER, MOUSE_TRIGGER};
use crate::state::{save_activity_snapshot, ActivitySnapshot};
use crate::RustyJackError;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Active GUI session user, when detectable.
#[must_use]
pub fn console_user_name() -> Option<String> {
    platform_console_user_name()
}

/// User account running the daemon process.
#[must_use]
pub fn daemon_user_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

/// ScalarWebAPI wake triggers that fire on idle→active transitions.
#[must_use]
pub fn activity_wake_triggers(config: &Config) -> Vec<String> {
    config
        .scalar_webapi_device
        .as_ref()
        .filter(|api| api.enabled)
        .map(|api| {
            api.triggers
                .iter()
                .filter(|trigger| {
                    matches!(
                        trigger.to_ascii_lowercase().as_str(),
                        KEYBOARD_TRIGGER | MOUSE_TRIGGER
                    )
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Record one activity poll sample, log it, and persist for `status`.
///
/// # Errors
///
/// Returns an error when the snapshot cannot be written to disk.
pub fn record_activity_poll(
    idle_duration: Duration,
    idle_threshold: Duration,
    config: &Config,
    became_active: bool,
) -> Result<ActivitySnapshot, RustyJackError> {
    let idle_seconds = idle_duration.as_secs_f64();
    let threshold_seconds = idle_threshold.as_secs_f64();
    let console_user = console_user_name();
    let daemon_user = daemon_user_name();
    let triggers = activity_wake_triggers(config);
    let sampled_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut snapshot = ActivitySnapshot {
        sampled_at_unix_seconds,
        idle_seconds,
        threshold_seconds,
        is_idle: idle_duration >= idle_threshold,
        console_user: console_user.clone(),
        daemon_user: daemon_user.clone(),
        triggers,
        last_became_active_at_unix_seconds: None,
        last_became_active_console_user: None,
        last_became_active_daemon_user: None,
    };

    if became_active {
        snapshot.last_became_active_at_unix_seconds = Some(sampled_at_unix_seconds);
        snapshot.last_became_active_console_user = console_user.clone();
        snapshot.last_became_active_daemon_user = Some(daemon_user.clone());
        tracing::info!(target: "daemon", "{}", format_activity_log_line(&snapshot));
    } else {
        if let Ok(Some(previous)) = crate::state::load_activity_snapshot() {
            snapshot.last_became_active_at_unix_seconds =
                previous.last_became_active_at_unix_seconds;
            snapshot.last_became_active_console_user = previous.last_became_active_console_user;
            snapshot.last_became_active_daemon_user = previous.last_became_active_daemon_user;
        }
        tracing::debug!(target: "daemon", "{}", format_activity_log_line(&snapshot));
    }

    save_activity_snapshot(&snapshot)?;
    Ok(snapshot)
}

#[must_use]
pub fn format_activity_log_line(snapshot: &ActivitySnapshot) -> String {
    let console = snapshot.console_user.as_deref().unwrap_or("(none)");
    let triggers = if snapshot.triggers.is_empty() {
        "(none)".into()
    } else {
        snapshot.triggers.join(",")
    };
    let became_active = snapshot
        .last_became_active_at_unix_seconds
        .is_some_and(|at| at == snapshot.sampled_at_unix_seconds);
    format!(
        "[activity] idle={:.1}s threshold={:.1}s console_user={console} daemon_user={} became_active={became_active} triggers={triggers}",
        snapshot.idle_seconds, snapshot.threshold_seconds, snapshot.daemon_user
    )
}

#[cfg(target_os = "macos")]
fn platform_console_user_name() -> Option<String> {
    let output = std::process::Command::new("stat")
        .args(["-f", "%Su", "/dev/console"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8(output.stdout).ok()?;
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_console_user_name() -> Option<String> {
    None
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

    #[test]
    fn test_activity_wake_triggers_filters_keyboard_and_mouse() {
        use crate::config::{Config, ScalarWebApiDeviceConfig};
        let config = Config {
            scalar_webapi_device: Some(ScalarWebApiDeviceConfig {
                enabled: true,
                model: "test".into(),
                host: Some("device.local".into()),
                port: 10_000,
                path: "/scalar".into(),
                mac_output: Default::default(),
                triggers: vec![
                    KEYBOARD_TRIGGER.into(),
                    MOUSE_TRIGGER.into(),
                    crate::scalar_webapi_device::OUTPUT_SELECTED_TRIGGER.into(),
                ],
                wake_debounce_ms: 30_000,
                request_timeout_ms: 3_000,
                require_quick_start: true,
                wake_on_lan: false,
                mac_address: None,
            }),
            ..Default::default()
        };
        assert_eq!(
            activity_wake_triggers(&config),
            vec![String::from("keyboard"), String::from("mouse")]
        );
    }

    #[test]
    fn test_format_activity_log_line() {
        let snapshot = ActivitySnapshot {
            sampled_at_unix_seconds: 1,
            idle_seconds: 12.4,
            threshold_seconds: 60.0,
            is_idle: false,
            console_user: Some("hcma".into()),
            daemon_user: "hcma".into(),
            triggers: vec!["keyboard".into(), "mouse".into()],
            last_became_active_at_unix_seconds: Some(1),
            last_became_active_console_user: Some("hcma".into()),
            last_became_active_daemon_user: Some("hcma".into()),
        };
        assert_eq!(
            format_activity_log_line(&snapshot),
            "[activity] idle=12.4s threshold=60.0s console_user=hcma daemon_user=hcma became_active=true triggers=keyboard,mouse"
        );
    }
}
