//! Routing policy evaluation (status + future apply/daemon).

use crate::config::Config;
use crate::device_select::{resolve_device_selector, ResolveError};
use crate::output_device::OutputDevice;
use crate::status::PolicyStatus;
use crate::system_default::DeviceList;
use std::path::Path;

/// Evaluate whether the active output matches configured policy.
#[must_use]
pub fn evaluate_policy(
    list: &DeviceList,
    config: Option<&Config>,
    config_path: Option<&Path>,
) -> PolicyStatus {
    let Some(config) = config else {
        let path_hint = config_path
            .map(|p| p.display().to_string())
            .or_else(|| crate::config::default_config_path().map(|p| p.display().to_string()))
            .unwrap_or_else(|| "~/.config/rusty-jack/config.json".into());

        return PolicyStatus {
            configured: false,
            config_path: config_path.map(|p| p.display().to_string()),
            preferred_monitor_name: None,
            preferred_device_uid: None,
            active_device_uid: active_uid(list),
            matches_preferred: None,
            preferred_present: None,
            preferred_alive: None,
            auto_switch: None,
            config_volume: None,
            message: format!("no config at {path_hint}"),
        };
    };

    let selector = config.preferred_selector();
    let preferred_monitor_name = selector.monitor_name.clone();
    let active = active_uid(list);

    let resolved = resolve_device_selector(&selector, &list.devices);

    let (preferred_uid, message, matches, preferred_present, preferred_alive) = match resolved {
        Ok(uid) => {
            let preferred_device = find_device(&list.devices, &uid);
            let matches = active.as_deref() == Some(uid.as_str());
            let message = if preferred_device.is_none() {
                format!("preferred device `{uid}` is not connected")
            } else if preferred_device.is_some_and(|d| !d.is_alive) {
                format!("preferred device `{uid}` is not alive")
            } else if matches {
                preferred_match_message(&selector, &uid)
            } else {
                format!(
                    "active output is `{}`; preferred is `{uid}`",
                    active.as_deref().unwrap_or("(none)")
                )
            };
            (
                Some(uid),
                message,
                Some(matches),
                Some(preferred_device.is_some()),
                preferred_device.map(|d| d.is_alive),
            )
        }
        Err(ResolveError::NotSpecified) => (
            None,
            "set preferred_device.monitor_name or preferred_device.uid".into(),
            None,
            None,
            None,
        ),
        Err(err) => (None, err.to_string(), None, None, None),
    };

    PolicyStatus {
        configured: true,
        config_path: config_path.map(|p| p.display().to_string()),
        preferred_monitor_name,
        preferred_device_uid: preferred_uid,
        active_device_uid: active,
        matches_preferred: matches,
        preferred_present,
        preferred_alive,
        auto_switch: Some(config.auto_switch),
        config_volume: config.volume,
        message,
    }
}

fn preferred_match_message(selector: &crate::device_select::DeviceSelector, uid: &str) -> String {
    if let Some(name) = selector.monitor_name.as_deref() {
        format!("active output matches preferred monitor `{name}` ({uid})")
    } else {
        "active output matches preferred device".into()
    }
}

fn active_uid(list: &DeviceList) -> Option<String> {
    list.devices
        .iter()
        .find(|d| d.is_active)
        .map(|d| d.uid.clone())
}

fn find_device<'a>(devices: &'a [OutputDevice], uid: &str) -> Option<&'a OutputDevice> {
    devices.iter().find(|d| d.uid == uid)
}

/// Why a device was chosen as the routing target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingTargetSource {
    Preferred,
    Fallback { index: usize },
    BuiltInFallback,
    Picker,
}

/// Device to route system audio to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RoutingTarget {
    pub uid: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_name: Option<String>,
    pub source: RoutingTargetSource,
}

/// Failed to pick a routing target from config + live devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectTargetError {
    Resolve(ResolveError),
    NoCandidateAvailable,
}

impl std::fmt::Display for SelectTargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resolve(err) => write!(f, "{err}"),
            Self::NoCandidateAvailable => {
                write!(f, "no preferred or fallback output device is connected")
            }
        }
    }
}

fn alive_device<'a>(devices: &'a [OutputDevice], uid: &str) -> Option<&'a OutputDevice> {
    devices
        .iter()
        .find(|d| d.uid == uid && d.is_alive)
        .or_else(|| devices.iter().find(|d| d.uid == uid))
}

fn to_routing_target(device: &OutputDevice, source: RoutingTargetSource) -> RoutingTarget {
    RoutingTarget {
        uid: device.uid.clone(),
        name: device.name.clone(),
        monitor_name: device.monitor_name.clone(),
        source,
    }
}

/// Pick the best connected output device from config (preferred, then fallbacks).
pub fn select_routing_target(
    config: &Config,
    devices: &[OutputDevice],
) -> Result<RoutingTarget, SelectTargetError> {
    match resolve_device_selector(&config.preferred_selector(), devices) {
        Ok(uid) => {
            if let Some(device) = alive_device(devices, &uid) {
                if device.is_alive {
                    return Ok(to_routing_target(device, RoutingTargetSource::Preferred));
                }
                // fall through to fallbacks when preferred is unplugged
            }
        }
        Err(err @ ResolveError::MonitorAmbiguous { .. })
        | Err(err @ ResolveError::NotSpecified) => {
            return Err(SelectTargetError::Resolve(err));
        }
        Err(ResolveError::MonitorNotFound(_)) | Err(ResolveError::UidNotFound(_)) => {}
    }

    if let Some(target) = select_fallback_target(config, devices) {
        return Ok(target);
    }

    Err(SelectTargetError::NoCandidateAvailable)
}

/// Pick the configured fallback, or the Mac's internal built-in output when none is configured.
#[must_use]
pub fn select_fallback_target(config: &Config, devices: &[OutputDevice]) -> Option<RoutingTarget> {
    for (index, fallback_uid) in config.fallback_uids.iter().enumerate() {
        if crate::config::is_placeholder_uid(fallback_uid) {
            continue;
        }
        if let Some(device) = alive_device(devices, fallback_uid) {
            if device.is_alive {
                return Some(to_routing_target(
                    device,
                    RoutingTargetSource::Fallback { index },
                ));
            }
        }
    }

    devices
        .iter()
        .find(|device| device.is_internal_builtin_output())
        .map(|device| to_routing_target(device, RoutingTargetSource::BuiltInFallback))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig};
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn hdmi(uid: &str, monitor: &str, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
            monitor_name: Some(monitor.into()),
        }
    }

    fn config_with_monitor(name: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            preferred_device: DeviceSelectorConfig {
                uid: None,
                monitor_name: Some(name.into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            sony_speaker: None,
        }
    }

    fn config_with_uid(uid: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
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

    fn config_with_fallback(preferred_monitor: &str, fallback_uid: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            preferred_device: DeviceSelectorConfig {
                uid: None,
                monitor_name: Some(preferred_monitor.into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![fallback_uid.into()],
            also_set_system_output: true,
            volume: None,
            sony_speaker: None,
        }
    }

    #[test]
    fn test_no_config_not_configured() {
        let list = DeviceList {
            devices: vec![hdmi("hdmi-1", "TV", true)],
            system_default: None,
        };
        let policy = evaluate_policy(&list, None, None);
        assert!(!policy.configured);
    }

    #[test]
    fn test_matches_preferred_by_monitor_name() {
        let list = DeviceList {
            devices: vec![hdmi("hdmi-1", "DELL U3219Q", true)],
            system_default: None,
        };
        let policy = evaluate_policy(&list, Some(&config_with_monitor("DELL U3219Q")), None);
        assert_eq!(policy.matches_preferred, Some(true));
        assert_eq!(policy.preferred_device_uid.as_deref(), Some("hdmi-1"));
        assert!(policy.message.contains("DELL U3219Q"));
    }

    #[test]
    fn test_does_not_match_preferred() {
        let list = DeviceList {
            devices: vec![
                hdmi("hdmi-1", "DELL U3219Q", true),
                hdmi("hdmi-2", "DELL U3223QE", false),
            ],
            system_default: None,
        };
        let policy = evaluate_policy(&list, Some(&config_with_uid("hdmi-2")), None);
        assert_eq!(policy.matches_preferred, Some(false));
    }

    #[test]
    fn test_monitor_not_found() {
        let list = DeviceList {
            devices: vec![hdmi("hdmi-1", "LG TV", true)],
            system_default: None,
        };
        let policy = evaluate_policy(&list, Some(&config_with_monitor("DELL U3219Q")), None);
        assert!(policy.message.contains("no connected output"));
    }

    #[test]
    fn test_select_routing_target_by_monitor() {
        let devices = vec![hdmi("hdmi-1", "DELL U3219Q", true)];
        let target = select_routing_target(&config_with_monitor("DELL U3219Q"), &devices).unwrap();
        assert_eq!(target.uid, "hdmi-1");
        assert!(matches!(target.source, RoutingTargetSource::Preferred));
    }

    #[test]
    fn test_select_routing_target_uses_fallback() {
        let devices = vec![hdmi("dp-1", "DELL U3223QE", true)];
        let target =
            select_routing_target(&config_with_fallback("DELL U3219Q", "dp-1"), &devices).unwrap();
        assert_eq!(target.uid, "dp-1");
        assert!(matches!(
            target.source,
            RoutingTargetSource::Fallback { index: 0 }
        ));
    }

    #[test]
    fn test_select_routing_target_uses_builtin_when_no_fallback_configured() {
        let mut builtin = hdmi("BuiltInSpeakerDevice", "Built-in", false);
        builtin.name = "Mac mini Speakers".into();
        builtin.monitor_name = None;
        builtin.transport = TransportKind::BuiltIn;
        let devices = vec![builtin];

        let target = select_routing_target(&config_with_monitor("DELL U3219Q"), &devices).unwrap();
        assert_eq!(target.uid, "BuiltInSpeakerDevice");
        assert!(matches!(
            target.source,
            RoutingTargetSource::BuiltInFallback
        ));
    }
}
