//! Background supervisor loop for `rusty-jack daemon`.

use crate::activity::ActivityMonitor;
use crate::apply::{preferred_uid, switch_output, volume_for_target, ApplyResult, SwitchOptions};
use crate::config::{load_config, Config};
use crate::coreaudio::AudioHal;
use crate::eqmac::{
    ensure_eqmac_for_target, format_ensure_messages, EqMacEnsureAction, EqMacEnsureResult,
};
use crate::output_device::OutputDevice;
use crate::policy::{select_fallback_target, select_routing_target, RoutingTarget};
use crate::sony::SonyWakeResult;
use crate::system_default::DeviceList;
use crate::volume_memory::remember_active_non_preferred;
use crate::RustyJackError;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const MIN_SONY_STARTUP_FALLBACK_GRACE: Duration = Duration::from_secs(30);

/// Why the daemon is evaluating policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonTickReason {
    Startup,
    StartupRetry,
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

    pub fn allow_sony_wake(&mut self, now: Instant, cooldown: Duration) -> bool {
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
    daemon_tick_with_eqmac(hal, config, reason, &ensure_eqmac_for_target)
}

type EqMacEnsureFn<'a> =
    dyn Fn(&[OutputDevice], &str) -> Result<EqMacEnsureResult, RustyJackError> + 'a;

fn daemon_tick_with_eqmac(
    hal: &dyn AudioHal,
    config: &Config,
    reason: DaemonTickReason,
    ensure_eqmac: &EqMacEnsureFn<'_>,
) -> Result<(DaemonTickResult, DeviceList), RustyJackError> {
    daemon_tick_with_hooks(
        hal,
        config,
        reason,
        ensure_eqmac,
        &crate::sony::wake_on_output_selected,
        &crate::sony::wake_on_activity,
    )
}

type SonyWakeFn<'a> =
    dyn Fn(&Config, &[OutputDevice], &str) -> Result<Option<SonyWakeResult>, RustyJackError> + 'a;

fn daemon_tick_with_hooks(
    hal: &dyn AudioHal,
    config: &Config,
    reason: DaemonTickReason,
    ensure_eqmac: &EqMacEnsureFn<'_>,
    wake_on_output_selected: &SonyWakeFn<'_>,
    wake_on_activity: &SonyWakeFn<'_>,
) -> Result<(DaemonTickResult, DeviceList), RustyJackError> {
    let list = hal.list_outputs()?;
    if !config.auto_switch {
        return Ok((DaemonTickResult::AutoSwitchDisabled, list));
    }

    let target = select_routing_target(config, &list.devices)
        .map_err(|err| RustyJackError::Config(err.to_string()))?;
    let preferred_uid = preferred_uid(config, &list.devices);
    let default_uid = hal.default_output_uid()?;
    let current_uid = current_routed_output_uid(&list, default_uid.as_deref());

    if current_uid.as_deref() == Some(target.uid.as_str()) {
        if reason == DaemonTickReason::UserActivity {
            if let Some(fallback) =
                sony_activity_fallback_target(config, &list.devices, &target.uid, wake_on_activity)
            {
                let result = switch_daemon_target(
                    hal,
                    config,
                    &list,
                    &fallback,
                    preferred_uid.as_deref(),
                    ensure_eqmac,
                )?;
                return Ok((DaemonTickResult::Switched(result), list));
            }
        } else if matches!(
            reason,
            DaemonTickReason::Startup
                | DaemonTickReason::StartupRetry
                | DaemonTickReason::Scheduled
        ) {
            let checked_target = sony_checked_current_target_or_fallback(
                config,
                &list.devices,
                target.clone(),
                reason,
                wake_on_output_selected,
            );
            if checked_target.uid != target.uid {
                let result = switch_daemon_target(
                    hal,
                    config,
                    &list,
                    &checked_target,
                    preferred_uid.as_deref(),
                    ensure_eqmac,
                )?;
                return Ok((DaemonTickResult::Switched(result), list));
            }
        }
        ensure_eqmac_for_daemon_target(&list.devices, &target.uid, reason, ensure_eqmac)?;
        ensure_startup_volume(hal, config, reason, &target, &preferred_uid)?;
        let result = no_change_result(&target);
        return Ok((DaemonTickResult::NoChange(result), list));
    }

    let target = if matches!(
        reason,
        DaemonTickReason::Startup | DaemonTickReason::StartupRetry | DaemonTickReason::Scheduled
    ) {
        sony_checked_target_or_fallback(
            config,
            &list.devices,
            target,
            reason,
            wake_on_output_selected,
        )
    } else {
        target
    };

    if current_uid.as_deref() == Some(target.uid.as_str()) {
        ensure_eqmac_for_daemon_target(&list.devices, &target.uid, reason, ensure_eqmac)?;
        ensure_startup_volume(hal, config, reason, &target, &preferred_uid)?;
        let result = no_change_result(&target);
        return Ok((DaemonTickResult::NoChange(result), list));
    }

    let result = switch_daemon_target(
        hal,
        config,
        &list,
        &target,
        preferred_uid.as_deref(),
        ensure_eqmac,
    )?;

    Ok((DaemonTickResult::Switched(result), list))
}

fn no_change_result(target: &RoutingTarget) -> ApplyResult {
    ApplyResult::NoChange {
        uid: target.uid.clone(),
        device_name: target.name.clone(),
        monitor_name: target.monitor_name.clone(),
        reason: "active output already on target".into(),
    }
}

fn current_routed_output_uid(list: &DeviceList, default_uid: Option<&str>) -> Option<String> {
    list.devices
        .iter()
        .find(|device| device.is_active)
        .map(|device| device.uid.clone())
        .or_else(|| default_uid.map(str::to_string))
}

fn ensure_startup_volume(
    hal: &dyn AudioHal,
    config: &Config,
    reason: DaemonTickReason,
    target: &RoutingTarget,
    preferred_uid: &Option<String>,
) -> Result<(), RustyJackError> {
    if reason != DaemonTickReason::Startup {
        return Ok(());
    }
    if let Some(volume) = volume_for_target(config, target, preferred_uid) {
        let _ = hal.set_output_volume(&target.uid, volume)?;
    }
    Ok(())
}

fn switch_daemon_target(
    hal: &dyn AudioHal,
    config: &Config,
    list: &DeviceList,
    target: &RoutingTarget,
    preferred_uid: Option<&str>,
    ensure_eqmac: &EqMacEnsureFn<'_>,
) -> Result<ApplyResult, RustyJackError> {
    let eqmac = ensure_eqmac(&list.devices, &target.uid)?;
    for line in format_ensure_messages(eqmac) {
        eprintln!("{line}");
    }

    remember_active_non_preferred(hal, &list.devices, preferred_uid, &target.uid)?;
    let preferred_uid = preferred_uid.map(str::to_string);
    let volume = volume_for_target(config, target, &preferred_uid);

    let result = switch_output(
        hal,
        target,
        &SwitchOptions {
            also_set_system_output: config.also_set_system_output,
            volume,
        },
    )?;

    if matches!(result, ApplyResult::Switched { .. }) {
        sleep_switch_delay(config);
    }

    Ok(result)
}

fn ensure_eqmac_for_daemon_target(
    devices: &[OutputDevice],
    target_uid: &str,
    reason: DaemonTickReason,
    ensure_eqmac: &EqMacEnsureFn<'_>,
) -> Result<(), RustyJackError> {
    let eqmac = ensure_eqmac(devices, target_uid)?;
    let should_log = matches!(eqmac.action, EqMacEnsureAction::Launched)
        || (reason == DaemonTickReason::Startup
            && matches!(eqmac.action, EqMacEnsureAction::NotInstalled));
    if should_log {
        for line in format_ensure_messages(eqmac) {
            eprintln!("{line}");
        }
    }
    Ok(())
}

fn sony_checked_target_or_fallback(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
    target: RoutingTarget,
    reason: DaemonTickReason,
    wake_on_output_selected: &SonyWakeFn<'_>,
) -> RoutingTarget {
    match wake_on_output_selected(config, devices, &target.uid) {
        Ok(Some(result)) => eprintln!("{}", crate::sony::format_wake_message(&result)),
        Ok(None) => {}
        Err(err) => {
            eprintln!("warning: {err}");
            if allow_sony_fallback(reason) {
                if let Some(fallback) = fallback_excluding(config, devices, &target.uid) {
                    eprintln!(
                        "warning: using fallback output {} ({}) because the selected Sony speaker is unreachable",
                        fallback.name, fallback.uid
                    );
                    return fallback;
                }
            }
        }
    }
    target
}

fn sony_checked_current_target_or_fallback(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
    target: RoutingTarget,
    reason: DaemonTickReason,
    wake_on_output_selected: &SonyWakeFn<'_>,
) -> RoutingTarget {
    match wake_on_output_selected(config, devices, &target.uid) {
        Ok(Some(result)) => eprintln!("{}", crate::sony::format_wake_message(&result)),
        Ok(None) => {}
        Err(err) => {
            eprintln!("warning: {err}");
            if allow_sony_fallback(reason) {
                if let Some(fallback) = fallback_excluding(config, devices, &target.uid) {
                    eprintln!(
                        "warning: using fallback output {} ({}) because the selected Sony speaker is unreachable",
                        fallback.name, fallback.uid
                    );
                    return fallback;
                }
            }
        }
    }
    target
}

fn allow_sony_fallback(reason: DaemonTickReason) -> bool {
    reason == DaemonTickReason::Scheduled
}

fn sony_activity_fallback_target(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
    target_uid: &str,
    wake_on_activity: &SonyWakeFn<'_>,
) -> Option<RoutingTarget> {
    match wake_on_activity(config, devices, target_uid) {
        Ok(Some(result)) => {
            eprintln!("{}", crate::sony::format_wake_message(&result));
            None
        }
        Ok(None) => None,
        Err(err) => {
            eprintln!("warning: {err}");
            fallback_excluding(config, devices, target_uid)
        }
    }
}

fn fallback_excluding(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
    excluded_uid: &str,
) -> Option<RoutingTarget> {
    select_fallback_target(config, devices).filter(|target| target.uid != excluded_uid)
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
    run_tick_logged(hal, &config, DaemonTickReason::Startup);
    let startup_grace_started = Instant::now();

    loop {
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
                        if state.allow_sony_wake(Instant::now(), cooldown) {
                            run_tick_logged(hal, &config, DaemonTickReason::UserActivity);
                        }
                    }
                }
                Err(err) => eprintln!("warning: could not read user activity state: {err}"),
            }
        }

        config = load_config(config_path)?;
        let reason = if startup_grace_started.elapsed() < sony_startup_fallback_grace(&config) {
            DaemonTickReason::StartupRetry
        } else {
            DaemonTickReason::Scheduled
        };
        run_tick_logged(hal, &config, reason);
    }
}

fn run_tick_logged(hal: &dyn AudioHal, config: &Config, reason: DaemonTickReason) {
    match daemon_tick(hal, config, reason) {
        Ok((DaemonTickResult::Switched(result), list)) => {
            print_daemon_switch(&result, &list);
        }
        Ok((DaemonTickResult::NoChange(_), _)) => {}
        Ok((DaemonTickResult::AutoSwitchDisabled, _)) => {}
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

fn sony_startup_fallback_grace(config: &Config) -> Duration {
    let cooldown = sony_wake_cooldown(config);
    if cooldown > MIN_SONY_STARTUP_FALLBACK_GRACE {
        cooldown
    } else {
        MIN_SONY_STARTUP_FALLBACK_GRACE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig, SonySpeakerConfig};
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;
    use std::sync::Mutex;

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

    fn builtin_speakers(uid: &str) -> OutputDevice {
        OutputDevice {
            id: 2,
            uid: uid.into(),
            name: "Mac mini Speakers".into(),
            transport: TransportKind::BuiltIn,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: None,
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

    fn no_op_eqmac(
        _devices: &[OutputDevice],
        _target_uid: &str,
    ) -> Result<EqMacEnsureResult, RustyJackError> {
        Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotNeeded,
        })
    }

    fn daemon_tick_no_eqmac(
        hal: &dyn AudioHal,
        config: &Config,
        reason: DaemonTickReason,
    ) -> Result<(DaemonTickResult, DeviceList), RustyJackError> {
        daemon_tick_with_eqmac(hal, config, reason, &no_op_eqmac)
    }

    fn sony_config(uid: &str) -> Config {
        let mut config = test_config(uid);
        config.sony_speaker = Some(SonySpeakerConfig {
            enabled: true,
            model: "SRS-ZR5".into(),
            host: Some("sony-speaker.local".into()),
            port: 10000,
            path: "sony".into(),
            mac_output: DeviceSelectorConfig {
                uid: Some(uid.into()),
                monitor_name: None,
            },
            triggers: vec!["output_selected".into(), "keyboard".into(), "mouse".into()],
            wake_debounce_ms: 30_000,
            request_timeout_ms: 3_000,
            require_quick_start: true,
        });
        config
    }

    fn fake_sony_wake_result() -> SonyWakeResult {
        SonyWakeResult {
            endpoint: "http://sony-speaker.local:10000/sony/system".into(),
            status_code: 200,
            previous_status: Some("standby".into()),
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
            daemon_tick_no_eqmac(&hal, &test_config("hdmi-1"), DaemonTickReason::Scheduled)
                .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_daemon_tick_no_change_does_not_switch() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &test_config("hdmi-1"), DaemonTickReason::Scheduled)
                .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_preserves_current_preferred_output() {
        let hal = MockHal::new(vec![
            hdmi_device("hdmi-1", "DELL U3219Q"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("hdmi-1");

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &test_config("hdmi-1"), DaemonTickReason::Startup).unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_tick_no_change_when_virtual_default_routes_to_target() {
        let mut hdmi = hdmi_device("hdmi-1", "DELL U3219Q");
        hdmi.is_active = true;
        let hal = MockHal::new(vec![hdmi]).with_default("EQMOutputCapture");

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &test_config("hdmi-1"), DaemonTickReason::Scheduled)
                .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_no_change_wakes_selected_sony_output() {
        let hal = MockHal::new(vec![builtin_speakers("BuiltInHeadphoneOutputDevice")])
            .with_default("BuiltInHeadphoneOutputDevice");
        let config = sony_config("BuiltInHeadphoneOutputDevice");
        let wake_calls = Mutex::new(Vec::<String>::new());
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], uid: &str| {
            wake_calls.lock().unwrap().push(uid.to_string());
            Ok(Some(fake_sony_wake_result()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Startup,
            &no_op_eqmac,
            &wake_on_output_selected,
            &wake_on_activity,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert_eq!(
            wake_calls.lock().unwrap().as_slice(),
            ["BuiltInHeadphoneOutputDevice"]
        );
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_no_change_keeps_selected_sony_when_wake_fails() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInHeadphoneOutputDevice");
        let mut config = sony_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Startup,
            &no_op_eqmac,
            &wake_on_output_selected,
            &wake_on_activity,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInHeadphoneOutputDevice")
        );
    }

    #[test]
    fn test_daemon_startup_retry_no_change_keeps_selected_sony_when_wake_fails() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInHeadphoneOutputDevice");
        let mut config = sony_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::StartupRetry,
            &no_op_eqmac,
            &wake_on_output_selected,
            &wake_on_activity,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInHeadphoneOutputDevice")
        );
    }

    #[test]
    fn test_daemon_startup_switches_to_sony_instead_of_fallback_when_wake_fails() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInSpeakerDevice");
        let mut config = sony_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Startup,
            &no_op_eqmac,
            &wake_on_output_selected,
            &wake_on_activity,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInHeadphoneOutputDevice")
        );
    }

    #[test]
    fn test_daemon_scheduled_no_change_falls_back_when_selected_sony_unreachable() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInHeadphoneOutputDevice");
        let mut config = sony_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Scheduled,
            &no_op_eqmac,
            &wake_on_output_selected,
            &wake_on_activity,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInSpeakerDevice")
        );
    }

    #[test]
    fn test_daemon_no_change_checks_eqmac_health_for_hdmi_target() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let calls = std::sync::Mutex::new(Vec::<String>::new());
        let ensure_eqmac = |devices: &[OutputDevice], target_uid: &str| {
            assert!(devices.iter().any(|device| device.uid == target_uid));
            calls.lock().unwrap().push(target_uid.to_string());
            Ok(EqMacEnsureResult {
                action: EqMacEnsureAction::AlreadyRunning,
            })
        };

        let (result, _list) = daemon_tick_with_eqmac(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Scheduled,
            &ensure_eqmac,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert_eq!(calls.lock().unwrap().as_slice(), ["hdmi-1"]);
        assert!(hal.set_calls().is_empty());
        assert!(hal.volume_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_applies_volume_when_already_on_target() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let mut config = test_config("hdmi-1");
        config.volume = Some(25);

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &config, DaemonTickReason::Startup).unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
        assert_eq!(
            hal.volume_calls(),
            vec![crate::coreaudio::mock::SetVolumeCall {
                uid: "hdmi-1".into(),
                percent: 25,
            }]
        );
    }

    #[test]
    fn test_daemon_scheduled_no_change_leaves_volume_alone() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let mut config = test_config("hdmi-1");
        config.volume = Some(25);

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &config, DaemonTickReason::Scheduled).unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.volume_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_no_change_on_fallback_does_not_use_preferred_volume() {
        let hal = MockHal::new(vec![builtin_speakers("BuiltInSpeakerDevice")])
            .with_default("BuiltInSpeakerDevice");
        let mut config = test_config("missing-hdmi");
        config.volume = Some(25);

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &config, DaemonTickReason::Startup).unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
        assert!(hal.volume_calls().is_empty());
    }

    #[test]
    fn test_daemon_switch_sets_config_volume_before_and_after_route_change() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInSpeakerDevice"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("BuiltInSpeakerDevice");
        let mut config = test_config("hdmi-1");
        config.volume = Some(25);

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &config, DaemonTickReason::Startup).unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(
            hal.volume_calls(),
            vec![
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi-1".into(),
                    percent: 25,
                },
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi-1".into(),
                    percent: 25,
                }
            ]
        );
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

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &config, DaemonTickReason::Scheduled).unwrap();

        assert_eq!(result, DaemonTickResult::AutoSwitchDisabled);
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_tick_switches_to_builtin_fallback_when_preferred_missing() {
        let hal = MockHal::new(vec![builtin_speakers("BuiltInSpeakerDevice")])
            .with_default("disconnected-hdmi");

        let (result, _list) =
            daemon_tick_no_eqmac(&hal, &test_config("hdmi-1"), DaemonTickReason::Scheduled)
                .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInSpeakerDevice")
        );
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

        assert!(state.allow_sony_wake(now, Duration::from_secs(30)));
        assert!(!state.allow_sony_wake(now + Duration::from_secs(1), Duration::from_secs(30)));
        assert!(state.allow_sony_wake(now + Duration::from_secs(31), Duration::from_secs(30)));
    }

    #[test]
    fn test_sony_startup_fallback_grace_has_floor() {
        let mut config = sony_config("BuiltInHeadphoneOutputDevice");
        config.sony_speaker.as_mut().unwrap().wake_debounce_ms = 2_000;

        assert_eq!(
            sony_startup_fallback_grace(&config),
            MIN_SONY_STARTUP_FALLBACK_GRACE
        );
    }

    #[test]
    fn test_sony_startup_fallback_grace_allows_longer_debounce() {
        let mut config = sony_config("BuiltInHeadphoneOutputDevice");
        config.sony_speaker.as_mut().unwrap().wake_debounce_ms = 45_000;

        assert_eq!(
            sony_startup_fallback_grace(&config),
            Duration::from_secs(45)
        );
    }
}
