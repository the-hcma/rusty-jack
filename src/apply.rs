//! Apply routing policy once (set default output).

use crate::config::Config;
use crate::coreaudio::AudioHal;
use crate::policy::{select_routing_target, RoutingTarget, RoutingTargetSource};
use crate::RustyJackError;
use serde::Serialize;

/// Result of `rusty-jack apply`.
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
    },
    NoChange {
        uid: String,
        device_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        monitor_name: Option<String>,
        reason: String,
    },
}

/// Apply config policy: resolve target device and set default output if needed.
pub fn apply_policy(hal: &dyn AudioHal, config: &Config) -> Result<ApplyResult, RustyJackError> {
    let list = hal.list_outputs()?;
    let target = select_routing_target(config, &list.devices)
        .map_err(|err| RustyJackError::Config(err.to_string()))?;

    let current = hal.default_output_uid()?;
    if current.as_deref() == Some(target.uid.as_str()) {
        return Ok(no_change_result(&target, "default output already on target"));
    }

    hal.set_default_output(&target.uid, config.also_set_system_output)?;

    Ok(ApplyResult::Switched {
        from_uid: current,
        to_uid: target.uid.clone(),
        device_name: target.name.clone(),
        monitor_name: target.monitor_name.clone(),
        source: target.source,
        also_set_system_output: config.also_set_system_output,
    })
}

fn no_change_result(target: &RoutingTarget, reason: &str) -> ApplyResult {
    ApplyResult::NoChange {
        uid: target.uid.clone(),
        device_name: target.name.clone(),
        monitor_name: target.monitor_name.clone(),
        reason: reason.into(),
    }
}

/// Print human-readable apply result.
pub fn print_text(result: &ApplyResult) {
    match result {
        ApplyResult::Switched {
            from_uid,
            to_uid,
            device_name,
            monitor_name,
            source,
            also_set_system_output,
        } => {
            let label = format_device_label(device_name, monitor_name.as_deref());
            let from = from_uid.as_deref().unwrap_or("(none)");
            let via = match source {
                RoutingTargetSource::Preferred => "preferred device".to_string(),
                RoutingTargetSource::Fallback { index } => format!("fallback #{index}"),
            };
            println!("Switched default output to {label} ({via})");
            println!("  from: {from}");
            println!("  to:   {to_uid}");
            if *also_set_system_output {
                println!("  also set system (alert) output");
            }
        }
        ApplyResult::NoChange {
            uid,
            device_name,
            monitor_name,
            reason,
        } => {
            let label = format_device_label(device_name, monitor_name.as_deref());
            println!("No change: {reason}");
            println!("  device: {label}");
            println!("  uid:    {uid}");
        }
    }
}

fn format_device_label(name: &str, monitor_name: Option<&str>) -> String {
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
            preferred_device: DeviceSelectorConfig {
                uid: None,
                monitor_name: Some(monitor.into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            sony_speaker: None,
        }
    }

    #[test]
    fn test_apply_switches_default() {
        let hal = MockHal::new(vec![
            hdmi_device("builtin", "Built-in"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("builtin");

        let result = apply_policy(&hal, &test_config("DELL U3219Q")).unwrap();
        assert!(matches!(result, ApplyResult::Switched { .. }));
        assert_eq!(hal.set_calls().len(), 1);
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_apply_no_change_when_already_default() {
        let hal = MockHal::new(vec![hdmi_device("hdmi-1", "DELL U3219Q")]).with_default("hdmi-1");
        let result = apply_policy(&hal, &test_config("DELL U3219Q")).unwrap();
        assert!(matches!(result, ApplyResult::NoChange { .. }));
        assert!(hal.set_calls().is_empty());
    }
}
