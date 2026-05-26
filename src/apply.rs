//! Apply routing policy once (set default output).

use crate::config::Config;
use crate::coreaudio::AudioHal;
use crate::eqmac::{ensure_eqmac_for_target, format_ensure_messages};
use crate::policy::{select_routing_target, RoutingTarget, RoutingTargetSource};
use crate::system_default::DeviceList;
use crate::volume_memory::{remember_active_non_preferred, remembered_volume};
use crate::volume_result::VolumeEnsureResult;
use crate::RustyJackError;
use serde::Serialize;

/// Options when switching the default output device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchOptions {
    pub also_set_system_output: bool,
    /// When `Some`, set output volume after switching. Omitted for manual picks.
    pub volume: Option<u8>,
}

/// Result of `rusty-jack apply` / `rusty-jack picker`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ApplyResult {
    Switched {
        from_uid: Option<String>,
        to_uid: String,
        device_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        monitor_name: Option<String>,
        source: RoutingTargetSource,
        also_set_system_output: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        volume: Option<VolumeEnsureResult>,
    },
    NoChange {
        uid: String,
        device_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        monitor_name: Option<String>,
        reason: String,
    },
}

/// Switch default output to `target` when it differs from the current default.
pub fn switch_output(
    hal: &dyn AudioHal,
    target: &RoutingTarget,
    options: &SwitchOptions,
) -> Result<ApplyResult, RustyJackError> {
    let current = hal.default_output_uid()?;
    if current.as_deref() == Some(target.uid.as_str()) {
        return Ok(no_change_result(target, "default output already on target"));
    }

    if let Some(percent) = options.volume {
        // Set the target before the route becomes audible, then verify again after switching.
        let _ = hal.set_output_volume(&target.uid, percent)?;
    }

    hal.set_default_output(&target.uid, options.also_set_system_output)?;

    let volume = if let Some(percent) = options.volume {
        Some(hal.set_output_volume(&target.uid, percent)?)
    } else {
        None
    };

    Ok(ApplyResult::Switched {
        from_uid: current,
        to_uid: target.uid.clone(),
        device_name: target.name.clone(),
        monitor_name: target.monitor_name.clone(),
        source: target.source.clone(),
        also_set_system_output: options.also_set_system_output,
        volume,
    })
}

/// Apply config policy: resolve target device and set default output if needed.
pub fn apply_policy(
    hal: &dyn AudioHal,
    config: &Config,
) -> Result<(ApplyResult, DeviceList), RustyJackError> {
    let list = hal.list_outputs()?;
    let target = select_routing_target(config, &list.devices)
        .map_err(|err| RustyJackError::Config(err.to_string()))?;
    let preferred_uid = preferred_uid(config, &list.devices);
    remember_active_non_preferred(hal, &list.devices, preferred_uid.as_deref(), &target.uid)?;
    let volume = volume_for_target(config, &target, &preferred_uid);

    let eqmac = ensure_eqmac_for_target(&list.devices, &target.uid)?;
    for line in format_ensure_messages(eqmac) {
        eprintln!("{line}");
    }

    let result = switch_output(
        hal,
        &target,
        &SwitchOptions {
            also_set_system_output: config.also_set_system_output,
            volume,
        },
    )?;
    crate::scalar_webapi_device::warn_on_output_selected(config, &list.devices, &target.uid);

    Ok((result, list))
}

#[must_use]
pub fn preferred_uid(
    config: &Config,
    devices: &[crate::output_device::OutputDevice],
) -> Option<String> {
    crate::device_select::resolve_device_selector(&config.preferred_selector(), devices).ok()
}

#[must_use]
pub fn volume_for_target(
    config: &Config,
    target: &RoutingTarget,
    preferred_uid: &Option<String>,
) -> Option<u8> {
    if preferred_uid.as_deref() == Some(target.uid.as_str())
        || matches!(target.source, RoutingTargetSource::Preferred)
    {
        return config.volume;
    }
    remembered_volume(&target.uid)
}

fn no_change_result(target: &RoutingTarget, reason: &str) -> ApplyResult {
    ApplyResult::NoChange {
        uid: target.uid.clone(),
        device_name: target.name.clone(),
        monitor_name: target.monitor_name.clone(),
        reason: reason.into(),
    }
}

/// Resolve a UID to a human-readable device label using a device list snapshot.
#[must_use]
pub fn label_for_uid(list: &DeviceList, uid: &str) -> String {
    if let Some(device) = list.devices.iter().find(|d| d.uid == uid) {
        return device.friendly_label();
    }
    if let Some(system_default) = &list.system_default {
        if system_default.uid == uid {
            return system_default.name.clone();
        }
    }
    uid.to_string()
}

/// Print human-readable apply/picker result.
pub fn print_text(result: &ApplyResult, list: &DeviceList) {
    match result {
        ApplyResult::Switched {
            from_uid,
            to_uid: _,
            device_name,
            monitor_name,
            source,
            also_set_system_output,
            volume,
        } => {
            let to = friendly_label(device_name, monitor_name.as_deref());
            let from = from_uid
                .as_deref()
                .map(|uid| label_for_uid(list, uid))
                .unwrap_or_else(|| "(none)".into());
            let via = match source {
                RoutingTargetSource::Preferred => "preferred device".to_string(),
                RoutingTargetSource::Fallback { index } => format!("fallback #{index}"),
                RoutingTargetSource::BuiltInFallback => "built-in fallback".to_string(),
                RoutingTargetSource::Picker => "picker".to_string(),
            };
            println!("Switched default output to {to} ({via})");
            println!("  from: {from}");
            println!("  to:   {to}");
            if *also_set_system_output {
                println!("  also set system (alert) output");
            }
            if let Some(result) = volume {
                if result.verified {
                    if let Some(actual) = result.actual {
                        if actual == result.target {
                            println!("  volume set to {}%", result.target);
                        } else {
                            println!(
                                "  volume set to {}% (read back {}% after {} attempts)",
                                result.target, actual, result.attempts
                            );
                        }
                    } else {
                        println!("  volume set to {}%", result.target);
                    }
                } else if let Some(actual) = result.actual {
                    eprintln!(
                        "  warning: volume target {}% but read back {}% after {} attempts",
                        result.target, actual, result.attempts
                    );
                } else {
                    eprintln!(
                        "  warning: could not verify volume {}% after {} attempts",
                        result.target, result.attempts
                    );
                }
            }
        }
        ApplyResult::NoChange {
            uid: _,
            device_name,
            monitor_name,
            reason,
        } => {
            let label = friendly_label(device_name, monitor_name.as_deref());
            println!("No change: {reason}");
            println!("  device: {label}");
        }
    }
}

#[must_use]
fn friendly_label(name: &str, monitor_name: Option<&str>) -> String {
    if let Some(monitor) = monitor_name {
        format!("{name} ({monitor})")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig};
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::system_default::DeviceList;
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

    fn test_config(monitor: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            preferred_device: DeviceSelectorConfig {
                uid: None,
                monitor_name: Some(monitor.into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            scalar_webapi_device: None,
        }
    }

    #[test]
    fn test_apply_switches_default() {
        let hal = MockHal::new(vec![
            hdmi_device("builtin", "Built-in"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("builtin");

        let (result, _list) = apply_policy(&hal, &test_config("DELL U3219Q")).unwrap();
        assert!(matches!(result, ApplyResult::Switched { .. }));
        assert_eq!(hal.set_calls().len(), 1);
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_apply_sets_volume_on_switch() {
        let hal = MockHal::new(vec![
            hdmi_device("builtin", "Built-in"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("builtin");

        let mut config = test_config("DELL U3219Q");
        config.volume = Some(42);

        let (result, _list) = apply_policy(&hal, &config).unwrap();
        assert!(matches!(result, ApplyResult::Switched { .. }));
        assert_eq!(hal.set_calls().len(), 1);
        assert_eq!(
            hal.volume_calls(),
            vec![
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi-1".into(),
                    percent: 42,
                },
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi-1".into(),
                    percent: 42,
                }
            ]
        );
    }

    #[test]
    fn test_apply_no_volume_when_already_default() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let mut config = test_config("DELL U3219Q");
        config.volume = Some(42);

        let (result, _list) = apply_policy(&hal, &config).unwrap();
        assert!(matches!(result, ApplyResult::NoChange { .. }));
        assert!(hal.volume_calls().is_empty());
    }

    #[test]
    fn test_apply_no_change_when_already_default() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let (result, _list) = apply_policy(&hal, &test_config("DELL U3219Q")).unwrap();
        assert!(matches!(result, ApplyResult::NoChange { .. }));
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_label_for_uid_uses_monitor_name() {
        let list = DeviceList {
            devices: vec![hdmi_device("hdmi-1", "DELL U3219Q")],
            system_default: None,
        };
        assert_eq!(label_for_uid(&list, "hdmi-1"), "HDMI (DELL U3219Q)");
    }

    #[test]
    fn test_label_for_uid_uses_system_default_name() {
        use crate::system_default::SystemDefaultInfo;
        use crate::transport::TransportKind;

        let list = DeviceList {
            devices: vec![],
            system_default: Some(SystemDefaultInfo {
                uid: "EQMOutputCapture".into(),
                name: "Internal Speakers (eqMac)".into(),
                transport: TransportKind::Virtual,
                manufacturer: None,
                model_uid: None,
                router: Some("eqMac".into()),
                driver: None,
                routed_to_uid: None,
                routed_to_label: None,
            }),
        };
        assert_eq!(
            label_for_uid(&list, "EQMOutputCapture"),
            "Internal Speakers (eqMac)"
        );
    }
}
