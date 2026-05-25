//! Background supervisor loop for `rusty-jack daemon`.

use crate::activity::ActivityMonitor;
use crate::apply::{switch_output, ApplyResult, SwitchOptions};
use crate::config::{load_config, Config};
use crate::coreaudio::AudioHal;
use crate::eqmac::{ensure_eqmac_for_target, format_ensure_messages};
use crate::policy::select_routing_target;
use crate::system_default::DeviceList;
use crate::RustyJackError;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

/// Why the daemon is evaluating policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonTickReason {
    Scheduled,
    UserActivity,
}

/// Outcome of one daemon policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonTickResult {
    AutoSwitchDisabled,
    Switched(ApplyResult),
    NoChange(ApplyResult),
}

/// Mutable state carried between daemon polls.
#[derive(Debug, Default)]
pub struct DaemonState {
    was_idle: Option<bool>,
    last_activity_wake: Option<Instant>,
}

impl DaemonState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return true only on an idle -> active transition.
    pub fn observe_idle_duration(
        &mut self,
        idle_duration: Duration,
        idle_threshold: Duration,
    ) -> bool {
        let is_idle = idle_duration >= idle_threshold;
        let became_active = matches!(self.was_idle, Some(true)) && !is_idle;
        self.was_idle = Some(is_idle);
        became_active
    }

    pub fn allow_activity_wake(&mut self, now: Instant, cooldown: Duration) -> bool {
        if self
            .last_activity_wake
            .is_some_and(|last| now.duration_since(last) < cooldown)
        {
            return false;
        }
        self.last_activity_wake = Some(now);
        true
    }
}

/// Evaluate policy once for the daemon.
pub fn daemon_tick(
    hal: &dyn AudioHal,
    config: &Config,
    reason: DaemonTickReason,
) -> Result<(DaemonTickResult, DeviceList), RustyJackError> {
    let list = hal.list_outputs()?;
    if !config.auto_switch {
        return Ok((DaemonTickResult::AutoSwitchDisabled, list));
    }

    let target = select_routing_target(config, &list.devices)
        .map_err(|err| RustyJackError::Config(err.to_string()))?;
    let current = hal.default_output_uid()?;

    if current.as_deref() == Some(target.uid.as_str()) {
        if reason == DaemonTickReason::UserActivity {
            crate::sony::warn_on_activity(config, &list.devices, &target.uid);
        }
        let result = ApplyResult::NoChange {
            uid: target.uid,
            device_name: target.name,
            monitor_name: target.monitor_name,
            reason: "default output already on target".into(),
        };
        return Ok((DaemonTickResult::NoChange(result), list));
    }

    let eqmac = ensure_eqmac_for_target(&list.devices, &target.uid)?;
    for line in format_ensure_messages(eqmac) {
        eprintln!("{line}");
    }

    let result = switch_output(
        hal,
        &target,
        &SwitchOptions {
            also_set_system_output: config.also_set_system_output,
            volume: config.volume,
        },
    )?;

    if matches!(result, ApplyResult::Switched { .. }) {
        sleep_switch_delay(config);
        crate::sony::warn_on_output_selected(config, &list.devices, &target.uid);
    }

    Ok((DaemonTickResult::Switched(result), list))
}

/// Run the daemon forever, reloading config before each scheduled poll.
pub fn run_forever(
    hal: &dyn AudioHal,
    config_path: &Path,
    activity: &dyn ActivityMonitor,
) -> Result<(), RustyJackError> {
    let mut config = load_config(config_path)?;
    let mut state = DaemonState::new();
    seed_activity_state(activity, &mut state, &config);
    run_startup_wake_check(hal, &config, &mut state);

    loop {
        run_tick_logged(hal, &config, DaemonTickReason::Scheduled);
        let poll_interval = Duration::from_millis(config.poll_interval_ms);
        let activity_interval = Duration::from_millis(config.activity_poll_interval_ms);
        let started = Instant::now();

        while started.elapsed() < poll_interval {
            let remaining = poll_interval.saturating_sub(started.elapsed());
            thread::sleep(activity_interval.min(remaining));

            let idle_threshold = Duration::from_millis(config.activity_idle_threshold_ms);
            match activity.idle_duration() {
                Ok(idle_duration) => {
                    if state.observe_idle_duration(idle_duration, idle_threshold) {
                        match load_config(config_path) {
                            Ok(updated) => config = updated,
                            Err(err) => eprintln!("warning: could not reload config: {err}"),
                        }
                        let cooldown = sony_wake_cooldown(&config);
                        if state.allow_activity_wake(Instant::now(), cooldown) {
                            run_tick_logged(hal, &config, DaemonTickReason::UserActivity);
                        }
                    }
                }
                Err(err) => eprintln!("warning: could not read user activity state: {err}"),
            }
        }

        config = load_config(config_path)?;
    }
}

fn run_startup_wake_check(hal: &dyn AudioHal, config: &Config, state: &mut DaemonState) {
    let cooldown = sony_wake_cooldown(config);
    if state.allow_activity_wake(Instant::now(), cooldown) {
        run_tick_logged(hal, config, DaemonTickReason::UserActivity);
    }
}

fn run_tick_logged(hal: &dyn AudioHal, config: &Config, reason: DaemonTickReason) {
    match daemon_tick(hal, config, reason) {
        Ok((DaemonTickResult::Switched(result), list)) => {
            print_daemon_switch(&result, &list);
        }
        Ok((DaemonTickResult::NoChange(_), _)) | Ok((DaemonTickResult::AutoSwitchDisabled, _)) => {}
        Err(err) => {
            eprintln!("warning: daemon tick failed: {err}");
        }
    }
}

fn print_daemon_switch(result: &ApplyResult, list: &DeviceList) {
    if let ApplyResult::Switched {
        to_uid,
        device_name,
        monitor_name,
        ..
    } = result
    {
        let label = if let Some(monitor) = monitor_name {
            format!("{device_name} ({monitor})")
        } else {
            device_name.clone()
        };
        let from = crate::apply::label_for_uid(
            list,
            match result {
                ApplyResult::Switched { from_uid, .. } => from_uid.as_deref().unwrap_or("(none)"),
                ApplyResult::NoChange { .. } => "(none)",
            },
        );
        println!("daemon: switched default output from {from} to {label} ({to_uid})");
    }
}

fn seed_activity_state(activity: &dyn ActivityMonitor, state: &mut DaemonState, config: &Config) {
    if let Ok(idle_duration) = activity.idle_duration() {
        let threshold = Duration::from_millis(config.activity_idle_threshold_ms);
        let _ = state.observe_idle_duration(idle_duration, threshold);
    }
}

fn sleep_switch_delay(config: &Config) {
    let delay = Duration::from_millis(config.switch_delay_ms);
    if !delay.is_zero() {
        thread::sleep(delay);
    }
}

fn sony_wake_cooldown(config: &Config) -> Duration {
    config
        .sony_speaker
        .as_ref()
        .map(|sony| Duration::from_millis(sony.wake_debounce_ms))
        .unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig};
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn hdmi_device(uid: &str, monitor: &str) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: Some(monitor.into()),
        }
    }

    fn test_config(uid: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 0,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            preferred_device: DeviceSelectorConfig {
                uid: Some(uid.into()),
                monitor_name: None,
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            sony_speaker: None,
        }
    }

    #[test]
    fn test_daemon_tick_switches_when_needed() {
        let hal = MockHal::new(vec![
            hdmi_device("builtin", "Built-in"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("builtin");

        let (result, _list) =
            daemon_tick(&hal, &test_config("hdmi-1"), DaemonTickReason::Scheduled).unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_daemon_tick_no_change_does_not_switch() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");

        let (result, _list) =
            daemon_tick(&hal, &test_config("hdmi-1"), DaemonTickReason::Scheduled).unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_tick_respects_auto_switch_false() {
        let hal = MockHal::new(vec![
            hdmi_device("builtin", "Built-in"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("builtin");
        let mut config = test_config("hdmi-1");
        config.auto_switch = false;

        let (result, _list) = daemon_tick(&hal, &config, DaemonTickReason::Scheduled).unwrap();

        assert_eq!(result, DaemonTickResult::AutoSwitchDisabled);
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_activity_state_only_fires_on_idle_to_active() {
        let mut state = DaemonState::new();
        let threshold = Duration::from_secs(60);

        assert!(!state.observe_idle_duration(Duration::from_secs(1), threshold));
        assert!(!state.observe_idle_duration(Duration::from_secs(90), threshold));
        assert!(state.observe_idle_duration(Duration::from_secs(1), threshold));
        assert!(!state.observe_idle_duration(Duration::from_secs(1), threshold));
    }

    #[test]
    fn test_activity_wake_cooldown() {
        let mut state = DaemonState::new();
        let now = Instant::now();

        assert!(state.allow_activity_wake(now, Duration::from_secs(30)));
        assert!(!state.allow_activity_wake(now + Duration::from_secs(1), Duration::from_secs(30)));
        assert!(state.allow_activity_wake(now + Duration::from_secs(31), Duration::from_secs(30)));
    }
}
