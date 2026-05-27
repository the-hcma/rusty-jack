//! Background supervisor loop for `rusty-jack daemon`.

use crate::activity::ActivityMonitor;
use crate::apply::{preferred_uid, switch_output, volume_for_target, ApplyResult, SwitchOptions};
use crate::config::{load_config, Config};
use crate::coreaudio::AudioHal;
use crate::hdmi_displayport_volume_control::{
    ensure_hdmi_displayport_volume_control_for_target, format_ensure_messages,
    recover_hdmi_displayport_volume_control_for_target, HdmiDisplayPortVolumeControlEnsureAction,
    HdmiDisplayPortVolumeControlEnsureResult,
};
use crate::network::{current_network_access_snapshot, NetworkAccessSnapshot};
use crate::output_device::OutputDevice;
use crate::passthrough::PassthroughController;
use crate::policy::{select_fallback_target, select_routing_target, RoutingTarget};
use crate::scalar_webapi_device::ScalarWebApiDeviceWakeResult;
use crate::system_default::DeviceList;
use crate::volume_memory::remember_active_non_preferred;
use crate::RustyJackError;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const MIN_SCALAR_WEBAPI_DEVICE_STARTUP_FALLBACK_GRACE: Duration = Duration::from_secs(30);

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
    last_scalar_webapi_device_activity_wake: Option<Instant>,
    network_access_observed: bool,
    network_access: Option<NetworkAccessSnapshot>,
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

    pub fn allow_scalar_webapi_device_wake(&mut self, now: Instant, cooldown: Duration) -> bool {
        if self
            .last_scalar_webapi_device_activity_wake
            .is_some_and(|last| now.duration_since(last) < cooldown)
        {
            return false;
        }
        self.last_scalar_webapi_device_activity_wake = Some(now);
        true
    }

    /// Allow an immediate ScalarWebAPI activity wake after network recovery.
    pub fn reset_scalar_webapi_device_wake_cooldown(&mut self) {
        self.last_scalar_webapi_device_activity_wake = None;
    }

    pub fn observe_network_access(
        &mut self,
        snapshot: Option<NetworkAccessSnapshot>,
    ) -> NetworkAccessChange {
        let changed = self.network_access_observed
            && match (&self.network_access, &snapshot) {
                (Some(previous), Some(current)) => previous != current,
                (Some(_), None) | (None, Some(_)) => true,
                (None, None) => false,
            };
        self.network_access_observed = true;
        self.network_access = snapshot;

        if changed {
            NetworkAccessChange::Changed
        } else {
            NetworkAccessChange::Unchanged
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkAccessChange {
    Unchanged,
    Changed,
}

/// Evaluate policy once for the daemon.
pub fn daemon_tick(
    hal: &dyn AudioHal,
    config: &Config,
    reason: DaemonTickReason,
) -> Result<(DaemonTickResult, DeviceList), RustyJackError> {
    daemon_tick_with_hooks(hal, config, reason, &daemon_hooks())
}

type HdmiDisplayPortVolumeControlEnsureFn<'a> = dyn Fn(&[OutputDevice], &str) -> Result<HdmiDisplayPortVolumeControlEnsureResult, RustyJackError>
    + 'a;
type HdmiDisplayPortVolumeControlRecoverFn<'a> = dyn Fn(&[OutputDevice], &str) -> Result<HdmiDisplayPortVolumeControlEnsureResult, RustyJackError>
    + 'a;

struct HdmiDisplayPortVolumeControlHooks<'a> {
    ensure: &'a HdmiDisplayPortVolumeControlEnsureFn<'a>,
    recover: &'a HdmiDisplayPortVolumeControlRecoverFn<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarWebApiDeviceFallbackPermission {
    Allowed,
    Suppressed,
}

type ScalarWebApiDeviceWakeFn<'a> = dyn Fn(
        &Config,
        &[OutputDevice],
        &str,
    ) -> Result<Option<ScalarWebApiDeviceWakeResult>, RustyJackError>
    + 'a;

struct ScalarWebApiDeviceHooks<'a> {
    fallback: ScalarWebApiDeviceFallbackPermission,
    wake_on_output_selected: &'a ScalarWebApiDeviceWakeFn<'a>,
    wake_on_activity: &'a ScalarWebApiDeviceWakeFn<'a>,
}

struct DaemonHooks<'a> {
    /// HDMI/DisplayPort volume control is port-specific and no-ops for other transports.
    hdmi_displayport_volume_control: HdmiDisplayPortVolumeControlHooks<'a>,
    /// ScalarWebAPI is device-specific: these hooks wake the configured external device
    /// attached to the selected Mac output, regardless of whether that output needs port volume control.
    scalar_webapi_device: ScalarWebApiDeviceHooks<'a>,
}

fn daemon_hooks<'a>() -> DaemonHooks<'a> {
    DaemonHooks {
        hdmi_displayport_volume_control: HdmiDisplayPortVolumeControlHooks {
            ensure: &ensure_hdmi_displayport_volume_control_for_target,
            recover: &recover_hdmi_displayport_volume_control_for_target,
        },
        scalar_webapi_device: ScalarWebApiDeviceHooks {
            fallback: ScalarWebApiDeviceFallbackPermission::Allowed,
            wake_on_output_selected: &crate::scalar_webapi_device::wake_on_output_selected,
            wake_on_activity: &crate::scalar_webapi_device::wake_on_activity,
        },
    }
}

fn daemon_tick_with_hooks(
    hal: &dyn AudioHal,
    config: &Config,
    reason: DaemonTickReason,
    hooks: &DaemonHooks<'_>,
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
            if let Some(fallback) = scalar_webapi_device_activity_fallback_target(
                config,
                &list.devices,
                &target.uid,
                hooks.scalar_webapi_device.fallback,
                hooks.scalar_webapi_device.wake_on_activity,
            ) {
                let result = switch_daemon_target(
                    hal,
                    config,
                    &list,
                    &fallback,
                    preferred_uid.as_deref(),
                    hooks.hdmi_displayport_volume_control.ensure,
                )?;
                return Ok((DaemonTickResult::Switched(result), list));
            }
        } else if matches!(
            reason,
            DaemonTickReason::Startup
                | DaemonTickReason::StartupRetry
                | DaemonTickReason::Scheduled
        ) {
            let checked_target = scalar_webapi_device_checked_current_target_or_fallback(
                config,
                &list.devices,
                target.clone(),
                reason,
                hooks.scalar_webapi_device.fallback,
                hooks.scalar_webapi_device.wake_on_output_selected,
            );
            if checked_target.uid != target.uid {
                let result = switch_daemon_target(
                    hal,
                    config,
                    &list,
                    &checked_target,
                    preferred_uid.as_deref(),
                    hooks.hdmi_displayport_volume_control.ensure,
                )?;
                return Ok((DaemonTickResult::Switched(result), list));
            }
        }
        if let Some(result) = recover_hdmi_displayport_volume_control_for_daemon_target(
            hal,
            config,
            &list,
            &target,
            preferred_uid.as_deref(),
            reason,
            &hooks.hdmi_displayport_volume_control,
        )? {
            return Ok((DaemonTickResult::Switched(result), list));
        }
        ensure_startup_volume(hal, config, reason, &target, &preferred_uid)?;
        let result = no_change_result(&target);
        return Ok((DaemonTickResult::NoChange(result), list));
    }

    let target = if matches!(
        reason,
        DaemonTickReason::Startup | DaemonTickReason::StartupRetry | DaemonTickReason::Scheduled
    ) {
        scalar_webapi_device_checked_target_or_fallback(
            config,
            &list.devices,
            target,
            reason,
            hooks.scalar_webapi_device.fallback,
            hooks.scalar_webapi_device.wake_on_output_selected,
        )
    } else {
        target
    };

    if current_uid.as_deref() == Some(target.uid.as_str()) {
        if let Some(result) = recover_hdmi_displayport_volume_control_for_daemon_target(
            hal,
            config,
            &list,
            &target,
            preferred_uid.as_deref(),
            reason,
            &hooks.hdmi_displayport_volume_control,
        )? {
            return Ok((DaemonTickResult::Switched(result), list));
        }
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
        hooks.hdmi_displayport_volume_control.ensure,
    )?;

    Ok((DaemonTickResult::Switched(result), list))
}

fn no_change_result(target: &RoutingTarget) -> ApplyResult {
    ApplyResult::NoChange {
        uid: target.uid.clone(),
        device_name: target.name.clone(),
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
    ensure_volume_control: &HdmiDisplayPortVolumeControlEnsureFn<'_>,
) -> Result<ApplyResult, RustyJackError> {
    let volume_control = ensure_volume_control(&list.devices, &target.uid)?;
    for line in format_ensure_messages(volume_control) {
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

fn ensure_hdmi_displayport_volume_control_for_daemon_target(
    devices: &[OutputDevice],
    target_uid: &str,
    reason: DaemonTickReason,
    ensure_volume_control: &HdmiDisplayPortVolumeControlEnsureFn<'_>,
) -> Result<(), RustyJackError> {
    let volume_control = ensure_volume_control(devices, target_uid)?;
    let should_log = matches!(
        volume_control.action,
        HdmiDisplayPortVolumeControlEnsureAction::EqMacLaunched
    ) || (reason == DaemonTickReason::Startup
        && matches!(
            volume_control.action,
            HdmiDisplayPortVolumeControlEnsureAction::NativeDriverRecommended
        ));
    if should_log {
        for line in format_ensure_messages(volume_control) {
            eprintln!("{line}");
        }
    }
    Ok(())
}

fn recover_hdmi_displayport_volume_control_for_daemon_target(
    hal: &dyn AudioHal,
    config: &Config,
    list: &DeviceList,
    target: &RoutingTarget,
    preferred_uid: Option<&str>,
    reason: DaemonTickReason,
    volume_control_hooks: &HdmiDisplayPortVolumeControlHooks<'_>,
) -> Result<Option<ApplyResult>, RustyJackError> {
    if reason != DaemonTickReason::Startup {
        ensure_hdmi_displayport_volume_control_for_daemon_target(
            &list.devices,
            &target.uid,
            reason,
            volume_control_hooks.ensure,
        )?;
        return Ok(None);
    }

    let volume_control = (volume_control_hooks.recover)(&list.devices, &target.uid)?;
    let recovered = matches!(
        volume_control.action,
        HdmiDisplayPortVolumeControlEnsureAction::EqMacRestarted
    );
    let should_log = recovered
        || (reason == DaemonTickReason::Startup
            && matches!(
                volume_control.action,
                HdmiDisplayPortVolumeControlEnsureAction::NativeDriverRecommended
            ));
    if should_log {
        for line in format_ensure_messages(volume_control) {
            eprintln!("{line}");
        }
    }

    if recovered {
        let result = switch_daemon_target(
            hal,
            config,
            list,
            target,
            preferred_uid,
            volume_control_hooks.ensure,
        )?;
        let preferred_uid = preferred_uid.map(str::to_string);
        ensure_startup_volume(hal, config, reason, target, &preferred_uid)?;
        return Ok(Some(result));
    }

    Ok(None)
}

fn scalar_webapi_device_checked_target_or_fallback(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
    target: RoutingTarget,
    reason: DaemonTickReason,
    scalar_webapi_device_fallback: ScalarWebApiDeviceFallbackPermission,
    wake_on_output_selected: &ScalarWebApiDeviceWakeFn<'_>,
) -> RoutingTarget {
    match wake_on_output_selected(config, devices, &target.uid) {
        Ok(Some(result)) => eprintln!(
            "{}",
            crate::scalar_webapi_device::format_wake_message(&result)
        ),
        Ok(None) => {}
        Err(err) => {
            eprintln!("warning: {err}");
            if allow_scalar_webapi_device_fallback(reason, scalar_webapi_device_fallback) {
                if let Some(fallback) = fallback_excluding(config, devices, &target.uid) {
                    eprintln!(
                        "warning: using fallback output {} ({}) because the selected ScalarWebAPI device is unreachable",
                        fallback.name, fallback.uid
                    );
                    return fallback;
                }
            }
        }
    }
    target
}

fn scalar_webapi_device_checked_current_target_or_fallback(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
    target: RoutingTarget,
    reason: DaemonTickReason,
    scalar_webapi_device_fallback: ScalarWebApiDeviceFallbackPermission,
    wake_on_output_selected: &ScalarWebApiDeviceWakeFn<'_>,
) -> RoutingTarget {
    match wake_on_output_selected(config, devices, &target.uid) {
        Ok(Some(result)) => eprintln!(
            "{}",
            crate::scalar_webapi_device::format_wake_message(&result)
        ),
        Ok(None) => {}
        Err(err) => {
            eprintln!("warning: {err}");
            if allow_scalar_webapi_device_fallback(reason, scalar_webapi_device_fallback) {
                if let Some(fallback) = fallback_excluding(config, devices, &target.uid) {
                    eprintln!(
                        "warning: using fallback output {} ({}) because the selected ScalarWebAPI device is unreachable",
                        fallback.name, fallback.uid
                    );
                    return fallback;
                }
            }
        }
    }
    target
}

fn allow_scalar_webapi_device_fallback(
    reason: DaemonTickReason,
    scalar_webapi_device_fallback: ScalarWebApiDeviceFallbackPermission,
) -> bool {
    scalar_webapi_device_fallback == ScalarWebApiDeviceFallbackPermission::Allowed
        && matches!(
            reason,
            DaemonTickReason::Scheduled | DaemonTickReason::UserActivity
        )
}

fn scalar_webapi_device_activity_fallback_target(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
    target_uid: &str,
    scalar_webapi_device_fallback: ScalarWebApiDeviceFallbackPermission,
    wake_on_activity: &ScalarWebApiDeviceWakeFn<'_>,
) -> Option<RoutingTarget> {
    match wake_on_activity(config, devices, target_uid) {
        Ok(Some(result)) => {
            eprintln!(
                "{}",
                crate::scalar_webapi_device::format_wake_message(&result)
            );
            None
        }
        Ok(None) => None,
        Err(err) => {
            eprintln!("warning: {err}");
            if allow_scalar_webapi_device_fallback(
                DaemonTickReason::UserActivity,
                scalar_webapi_device_fallback,
            ) {
                fallback_excluding(config, devices, target_uid)
            } else {
                None
            }
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
    let mut passthrough = PassthroughController::default();
    seed_activity_state(activity, &mut state, &config);
    seed_network_state(&mut state);
    run_tick_logged(
        hal,
        &config,
        DaemonTickReason::Startup,
        ScalarWebApiDeviceFallbackPermission::Suppressed,
    );
    sync_passthrough_logged(hal, &mut passthrough, &config);
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
                        let network_change = observe_current_network_access(&mut state);
                        if network_change == NetworkAccessChange::Changed {
                            state.reset_scalar_webapi_device_wake_cooldown();
                        }
                        let cooldown = scalar_webapi_device_wake_cooldown(&config);
                        if state.allow_scalar_webapi_device_wake(Instant::now(), cooldown) {
                            let scalar_webapi_device_fallback =
                                scalar_webapi_device_fallback_permission(network_change);
                            run_tick_logged(
                                hal,
                                &config,
                                DaemonTickReason::UserActivity,
                                scalar_webapi_device_fallback,
                            );
                            sync_passthrough_logged(hal, &mut passthrough, &config);
                        }
                    }
                }
                Err(err) => eprintln!("warning: could not read user activity state: {err}"),
            }
        }

        config = load_config(config_path)?;
        let network_change = observe_current_network_access(&mut state);
        if network_change == NetworkAccessChange::Changed {
            state.reset_scalar_webapi_device_wake_cooldown();
        }
        let reason = if startup_grace_started.elapsed()
            < scalar_webapi_device_startup_fallback_grace(&config)
        {
            DaemonTickReason::StartupRetry
        } else {
            DaemonTickReason::Scheduled
        };
        let scalar_webapi_device_fallback = if reason == DaemonTickReason::Scheduled {
            scalar_webapi_device_fallback_permission(network_change)
        } else {
            ScalarWebApiDeviceFallbackPermission::Suppressed
        };
        run_tick_logged(hal, &config, reason, scalar_webapi_device_fallback);
        sync_passthrough_logged(hal, &mut passthrough, &config);
    }
}

fn sync_passthrough_logged(
    hal: &dyn AudioHal,
    passthrough: &mut PassthroughController,
    config: &Config,
) {
    if let Err(err) = passthrough.sync(hal, config) {
        eprintln!("warning: passthrough sync failed: {err}");
    }
}

fn run_tick_logged(
    hal: &dyn AudioHal,
    config: &Config,
    reason: DaemonTickReason,
    scalar_webapi_device_fallback: ScalarWebApiDeviceFallbackPermission,
) {
    let hooks = DaemonHooks {
        hdmi_displayport_volume_control: HdmiDisplayPortVolumeControlHooks {
            ensure: &ensure_hdmi_displayport_volume_control_for_target,
            recover: &recover_hdmi_displayport_volume_control_for_target,
        },
        scalar_webapi_device: ScalarWebApiDeviceHooks {
            fallback: scalar_webapi_device_fallback,
            wake_on_output_selected: &crate::scalar_webapi_device::wake_on_output_selected,
            wake_on_activity: &crate::scalar_webapi_device::wake_on_activity,
        },
    };
    match daemon_tick_with_hooks(hal, config, reason, &hooks) {
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
        ..
    } = result
    {
        let from = crate::apply::label_for_uid(
            list,
            match result {
                ApplyResult::Switched { from_uid, .. } => from_uid.as_deref().unwrap_or("(none)"),
                ApplyResult::NoChange { .. } => "(none)",
            },
        );
        println!("daemon: switched default output from {from} to {device_name} ({to_uid})");
    }
}

fn seed_activity_state(activity: &dyn ActivityMonitor, state: &mut DaemonState, config: &Config) {
    if let Ok(idle_duration) = activity.idle_duration() {
        let threshold = Duration::from_millis(config.activity_idle_threshold_ms);
        let _ = state.observe_idle_duration(idle_duration, threshold);
    }
}

fn seed_network_state(state: &mut DaemonState) {
    let _ = state.observe_network_access(current_network_access_snapshot().ok().flatten());
}

fn observe_current_network_access(state: &mut DaemonState) -> NetworkAccessChange {
    state.observe_network_access(current_network_access_snapshot().ok().flatten())
}

fn scalar_webapi_device_fallback_permission(
    change: NetworkAccessChange,
) -> ScalarWebApiDeviceFallbackPermission {
    match change {
        NetworkAccessChange::Changed => ScalarWebApiDeviceFallbackPermission::Allowed,
        NetworkAccessChange::Unchanged => ScalarWebApiDeviceFallbackPermission::Suppressed,
    }
}

fn sleep_switch_delay(config: &Config) {
    let delay = Duration::from_millis(config.switch_delay_ms);
    if !delay.is_zero() {
        thread::sleep(delay);
    }
}

fn scalar_webapi_device_wake_cooldown(config: &Config) -> Duration {
    config
        .scalar_webapi_device
        .as_ref()
        .map(|api| Duration::from_millis(api.wake_debounce_ms))
        .unwrap_or(Duration::ZERO)
}

fn scalar_webapi_device_startup_fallback_grace(config: &Config) -> Duration {
    let cooldown = scalar_webapi_device_wake_cooldown(config);
    if cooldown > MIN_SCALAR_WEBAPI_DEVICE_STARTUP_FALLBACK_GRACE {
        cooldown
    } else {
        MIN_SCALAR_WEBAPI_DEVICE_STARTUP_FALLBACK_GRACE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig, ScalarWebApiDeviceConfig};
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;
    use std::sync::Mutex;

    fn hdmi_device(uid: &str, _monitor: &str) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: false,
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
                name: None,
                uid: Some(uid.into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            scalar_webapi_device: None,
        }
    }

    fn no_op_hdmi_displayport_volume_control(
        _devices: &[OutputDevice],
        _target_uid: &str,
    ) -> Result<HdmiDisplayPortVolumeControlEnsureResult, RustyJackError> {
        Ok(HdmiDisplayPortVolumeControlEnsureResult {
            action: HdmiDisplayPortVolumeControlEnsureAction::NotNeeded,
        })
    }

    fn no_op_hdmi_displayport_volume_control_recovery(
        _devices: &[OutputDevice],
        _target_uid: &str,
    ) -> Result<HdmiDisplayPortVolumeControlEnsureResult, RustyJackError> {
        Ok(HdmiDisplayPortVolumeControlEnsureResult {
            action: HdmiDisplayPortVolumeControlEnsureAction::NotNeeded,
        })
    }

    fn daemon_tick_no_hdmi_displayport_volume_control(
        hal: &dyn AudioHal,
        config: &Config,
        reason: DaemonTickReason,
    ) -> Result<(DaemonTickResult, DeviceList), RustyJackError> {
        let hooks = no_op_daemon_hooks(ScalarWebApiDeviceFallbackPermission::Allowed);
        daemon_tick_with_hooks(hal, config, reason, &hooks)
    }

    fn no_op_hdmi_displayport_volume_control_hooks() -> HdmiDisplayPortVolumeControlHooks<'static> {
        HdmiDisplayPortVolumeControlHooks {
            ensure: &no_op_hdmi_displayport_volume_control,
            recover: &no_op_hdmi_displayport_volume_control_recovery,
        }
    }

    fn no_op_scalar_webapi_device_wake(
        _config: &Config,
        _devices: &[OutputDevice],
        _uid: &str,
    ) -> Result<Option<ScalarWebApiDeviceWakeResult>, RustyJackError> {
        Ok(None)
    }

    fn no_op_daemon_hooks(fallback: ScalarWebApiDeviceFallbackPermission) -> DaemonHooks<'static> {
        DaemonHooks {
            hdmi_displayport_volume_control: no_op_hdmi_displayport_volume_control_hooks(),
            scalar_webapi_device: ScalarWebApiDeviceHooks {
                fallback,
                wake_on_output_selected: &no_op_scalar_webapi_device_wake,
                wake_on_activity: &no_op_scalar_webapi_device_wake,
            },
        }
    }

    fn test_hooks<'a>(
        fallback: ScalarWebApiDeviceFallbackPermission,
        wake_on_output_selected: &'a ScalarWebApiDeviceWakeFn<'a>,
        wake_on_activity: &'a ScalarWebApiDeviceWakeFn<'a>,
    ) -> DaemonHooks<'a> {
        DaemonHooks {
            hdmi_displayport_volume_control: no_op_hdmi_displayport_volume_control_hooks(),
            scalar_webapi_device: ScalarWebApiDeviceHooks {
                fallback,
                wake_on_output_selected,
                wake_on_activity,
            },
        }
    }

    fn daemon_hooks_with_hdmi_displayport_volume_control<'a>(
        ensure: &'a HdmiDisplayPortVolumeControlEnsureFn<'a>,
        recover: &'a HdmiDisplayPortVolumeControlRecoverFn<'a>,
    ) -> DaemonHooks<'a> {
        DaemonHooks {
            hdmi_displayport_volume_control: HdmiDisplayPortVolumeControlHooks { ensure, recover },
            scalar_webapi_device: ScalarWebApiDeviceHooks {
                fallback: ScalarWebApiDeviceFallbackPermission::Allowed,
                wake_on_output_selected: &no_op_scalar_webapi_device_wake,
                wake_on_activity: &no_op_scalar_webapi_device_wake,
            },
        }
    }

    fn scalar_webapi_device_config(uid: &str) -> Config {
        let mut config = test_config(uid);
        config.scalar_webapi_device = Some(ScalarWebApiDeviceConfig {
            enabled: true,
            model: "ScalarWebAPI device".into(),
            host: Some("scalarwebapi-device.local".into()),
            port: 10000,
            path: protocol_path(),
            mac_output: DeviceSelectorConfig {
                name: None,
                uid: Some(uid.into()),
            },
            triggers: vec!["output_selected".into(), "keyboard".into(), "mouse".into()],
            wake_debounce_ms: 30_000,
            request_timeout_ms: 3_000,
            require_quick_start: true,
        });
        config
    }

    fn fake_scalar_webapi_device_wake_result() -> ScalarWebApiDeviceWakeResult {
        ScalarWebApiDeviceWakeResult {
            endpoint: format!(
                "http://scalarwebapi-device.local:10000/{}/system",
                protocol_path()
            ),
            status_code: 200,
            previous_status: Some("standby".into()),
        }
    }

    fn protocol_path() -> String {
        ["so", "ny"].concat()
    }

    #[test]
    fn test_daemon_tick_switches_when_needed() {
        let hal = MockHal::new(vec![
            hdmi_device("builtin", "Built-in"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("builtin");

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Scheduled,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_daemon_tick_no_change_does_not_switch() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Scheduled,
        )
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

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Startup,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_tick_no_change_when_virtual_default_routes_to_target() {
        let mut hdmi = hdmi_device("hdmi-1", "DELL U3219Q");
        hdmi.is_active = true;
        let hal = MockHal::new(vec![hdmi]).with_default("EQMOutputCapture");

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Scheduled,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_no_change_wakes_selected_scalar_webapi_device_output() {
        let hal = MockHal::new(vec![builtin_speakers("BuiltInHeadphoneOutputDevice")])
            .with_default("BuiltInHeadphoneOutputDevice");
        let config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        let wake_calls = Mutex::new(Vec::<String>::new());
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], uid: &str| {
            wake_calls.lock().unwrap().push(uid.to_string());
            Ok(Some(fake_scalar_webapi_device_wake_result()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Startup,
            &test_hooks(
                ScalarWebApiDeviceFallbackPermission::Suppressed,
                &wake_on_output_selected,
                &wake_on_activity,
            ),
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
    fn test_daemon_startup_no_change_keeps_selected_scalar_webapi_device_when_wake_fails() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInHeadphoneOutputDevice");
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Startup,
            &test_hooks(
                ScalarWebApiDeviceFallbackPermission::Suppressed,
                &wake_on_output_selected,
                &wake_on_activity,
            ),
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
    fn test_daemon_startup_retry_no_change_keeps_selected_scalar_webapi_device_when_wake_fails() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInHeadphoneOutputDevice");
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::StartupRetry,
            &test_hooks(
                ScalarWebApiDeviceFallbackPermission::Suppressed,
                &wake_on_output_selected,
                &wake_on_activity,
            ),
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
    fn test_daemon_startup_switches_to_scalar_webapi_device_instead_of_fallback_when_wake_fails() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInSpeakerDevice");
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Startup,
            &test_hooks(
                ScalarWebApiDeviceFallbackPermission::Suppressed,
                &wake_on_output_selected,
                &wake_on_activity,
            ),
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInHeadphoneOutputDevice")
        );
    }

    #[test]
    fn test_daemon_scheduled_no_change_falls_back_when_network_changed() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInHeadphoneOutputDevice");
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Scheduled,
            &test_hooks(
                ScalarWebApiDeviceFallbackPermission::Allowed,
                &wake_on_output_selected,
                &wake_on_activity,
            ),
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInSpeakerDevice")
        );
    }

    #[test]
    fn test_daemon_scheduled_no_change_keeps_scalar_webapi_device_when_network_unchanged() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInHeadphoneOutputDevice");
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Scheduled,
            &test_hooks(
                ScalarWebApiDeviceFallbackPermission::Suppressed,
                &wake_on_output_selected,
                &wake_on_activity,
            ),
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
    fn test_daemon_scheduled_switches_to_scalar_webapi_device_when_network_unchanged() {
        let hal = MockHal::new(vec![
            builtin_speakers("BuiltInHeadphoneOutputDevice"),
            builtin_speakers("BuiltInSpeakerDevice"),
        ])
        .with_default("BuiltInSpeakerDevice");
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config.fallback_uids = vec!["BuiltInSpeakerDevice".into()];
        let wake_on_output_selected = |_: &Config, _: &[OutputDevice], _: &str| {
            Err(RustyJackError::Speaker("speaker unreachable".into()))
        };
        let wake_on_activity = |_: &Config, _: &[OutputDevice], _: &str| Ok(None);

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &config,
            DaemonTickReason::Scheduled,
            &test_hooks(
                ScalarWebApiDeviceFallbackPermission::Suppressed,
                &wake_on_output_selected,
                &wake_on_activity,
            ),
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("BuiltInHeadphoneOutputDevice")
        );
    }

    #[test]
    fn test_daemon_no_change_checks_eqmac_health_for_hdmi_target() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let calls = std::sync::Mutex::new(Vec::<String>::new());
        let ensure_volume_control = |devices: &[OutputDevice], target_uid: &str| {
            assert!(devices.iter().any(|device| device.uid == target_uid));
            calls.lock().unwrap().push(target_uid.to_string());
            Ok(HdmiDisplayPortVolumeControlEnsureResult {
                action: HdmiDisplayPortVolumeControlEnsureAction::EqMacAlreadyRunning,
            })
        };

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Scheduled,
            &daemon_hooks_with_hdmi_displayport_volume_control(
                &ensure_volume_control,
                &no_op_hdmi_displayport_volume_control_recovery,
            ),
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert_eq!(calls.lock().unwrap().as_slice(), ["hdmi-1"]);
        assert!(hal.set_calls().is_empty());
        assert!(hal.volume_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_restarts_stale_eqmac_and_reapplies_route() {
        let mut hdmi = hdmi_device("hdmi-1", "DELL U3219Q");
        hdmi.is_active = true;
        let hal = MockHal::new(vec![hdmi]).with_default("EQMOutputCapture");
        let recover_calls = std::sync::Mutex::new(Vec::<String>::new());
        let recover_volume_control = |devices: &[OutputDevice], target_uid: &str| {
            assert!(devices.iter().any(|device| device.uid == target_uid));
            recover_calls.lock().unwrap().push(target_uid.to_string());
            Ok(HdmiDisplayPortVolumeControlEnsureResult {
                action: HdmiDisplayPortVolumeControlEnsureAction::EqMacRestarted,
            })
        };
        let ensure_volume_control = |_: &[OutputDevice], _: &str| {
            Ok(HdmiDisplayPortVolumeControlEnsureResult {
                action: HdmiDisplayPortVolumeControlEnsureAction::EqMacAlreadyRunning,
            })
        };

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Startup,
            &daemon_hooks_with_hdmi_displayport_volume_control(
                &ensure_volume_control,
                &recover_volume_control,
            ),
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::Switched(_)));
        assert_eq!(recover_calls.lock().unwrap().as_slice(), ["hdmi-1"]);
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_daemon_user_activity_ensures_eqmac_without_restart() {
        let mut hdmi = hdmi_device("hdmi-1", "DELL U3219Q");
        hdmi.is_active = true;
        let hal = MockHal::new(vec![hdmi]).with_default("EQMOutputCapture");
        let ensure_calls = std::sync::Mutex::new(Vec::<String>::new());
        let recover_volume_control =
            |_: &[OutputDevice], _: &str| panic!("user-activity ticks should not restart eqMac");
        let ensure_volume_control = |_: &[OutputDevice], target_uid: &str| {
            ensure_calls.lock().unwrap().push(target_uid.to_string());
            Ok(HdmiDisplayPortVolumeControlEnsureResult {
                action: HdmiDisplayPortVolumeControlEnsureAction::EqMacAlreadyRunning,
            })
        };

        let (result, _list) = daemon_tick_with_hooks(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::UserActivity,
            &daemon_hooks_with_hdmi_displayport_volume_control(
                &ensure_volume_control,
                &recover_volume_control,
            ),
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert_eq!(ensure_calls.lock().unwrap().as_slice(), ["hdmi-1"]);
        assert!(hal.set_calls().is_empty());
        assert!(hal.volume_calls().is_empty());
        assert_eq!(
            hal.default_output_uid().unwrap().as_deref(),
            Some("EQMOutputCapture")
        );
    }

    #[test]
    fn test_daemon_startup_applies_volume_when_already_on_target() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let mut config = test_config("hdmi-1");
        config.volume = Some(25);

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &config,
            DaemonTickReason::Startup,
        )
        .unwrap();

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

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &config,
            DaemonTickReason::Scheduled,
        )
        .unwrap();

        assert!(matches!(result, DaemonTickResult::NoChange(_)));
        assert!(hal.volume_calls().is_empty());
    }

    #[test]
    fn test_daemon_startup_no_change_on_fallback_does_not_use_preferred_volume() {
        let hal = MockHal::new(vec![builtin_speakers("BuiltInSpeakerDevice")])
            .with_default("BuiltInSpeakerDevice");
        let mut config = test_config("missing-hdmi");
        config.volume = Some(25);

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &config,
            DaemonTickReason::Startup,
        )
        .unwrap();

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

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &config,
            DaemonTickReason::Startup,
        )
        .unwrap();

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

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &config,
            DaemonTickReason::Scheduled,
        )
        .unwrap();

        assert_eq!(result, DaemonTickResult::AutoSwitchDisabled);
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_daemon_tick_switches_to_builtin_fallback_when_preferred_missing() {
        let hal = MockHal::new(vec![builtin_speakers("BuiltInSpeakerDevice")])
            .with_default("disconnected-hdmi");

        let (result, _list) = daemon_tick_no_hdmi_displayport_volume_control(
            &hal,
            &test_config("hdmi-1"),
            DaemonTickReason::Scheduled,
        )
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

        assert!(state.allow_scalar_webapi_device_wake(now, Duration::from_secs(30)));
        assert!(!state.allow_scalar_webapi_device_wake(
            now + Duration::from_secs(1),
            Duration::from_secs(30)
        ));
        assert!(state.allow_scalar_webapi_device_wake(
            now + Duration::from_secs(31),
            Duration::from_secs(30)
        ));
    }

    #[test]
    fn test_network_access_change_detects_interface_gateway_or_ip_changes() {
        let mut state = DaemonState::new();
        let initial = NetworkAccessSnapshot {
            interface: "en0".into(),
            gateway: Some("192.168.86.1".into()),
            ip_address: Some("192.168.86.100".into()),
        };
        let changed = NetworkAccessSnapshot {
            ip_address: Some("192.168.86.101".into()),
            ..initial.clone()
        };

        assert_eq!(
            state.observe_network_access(Some(initial.clone())),
            NetworkAccessChange::Unchanged
        );
        assert_eq!(
            state.observe_network_access(Some(initial)),
            NetworkAccessChange::Unchanged
        );
        assert_eq!(
            state.observe_network_access(Some(changed)),
            NetworkAccessChange::Changed
        );
    }

    #[test]
    fn test_network_access_change_detects_lost_default_route() {
        let mut state = DaemonState::new();
        let initial = NetworkAccessSnapshot {
            interface: "en0".into(),
            gateway: Some("192.168.86.1".into()),
            ip_address: Some("192.168.86.100".into()),
        };

        assert_eq!(
            state.observe_network_access(Some(initial)),
            NetworkAccessChange::Unchanged
        );
        assert_eq!(
            state.observe_network_access(None),
            NetworkAccessChange::Changed
        );
    }

    #[test]
    fn test_scalar_webapi_device_fallback_permission_requires_network_change() {
        assert_eq!(
            scalar_webapi_device_fallback_permission(NetworkAccessChange::Unchanged),
            ScalarWebApiDeviceFallbackPermission::Suppressed
        );
        assert_eq!(
            scalar_webapi_device_fallback_permission(NetworkAccessChange::Changed),
            ScalarWebApiDeviceFallbackPermission::Allowed
        );
    }

    #[test]
    fn test_scalar_webapi_device_startup_fallback_grace_has_floor() {
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config
            .scalar_webapi_device
            .as_mut()
            .unwrap()
            .wake_debounce_ms = 2_000;

        assert_eq!(
            scalar_webapi_device_startup_fallback_grace(&config),
            MIN_SCALAR_WEBAPI_DEVICE_STARTUP_FALLBACK_GRACE
        );
    }

    #[test]
    fn test_scalar_webapi_device_startup_fallback_grace_allows_longer_debounce() {
        let mut config = scalar_webapi_device_config("BuiltInHeadphoneOutputDevice");
        config
            .scalar_webapi_device
            .as_mut()
            .unwrap()
            .wake_debounce_ms = 45_000;

        assert_eq!(
            scalar_webapi_device_startup_fallback_grace(&config),
            Duration::from_secs(45)
        );
    }
}
