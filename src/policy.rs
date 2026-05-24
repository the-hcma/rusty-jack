//! Routing policy evaluation (status + future apply/daemon).

use crate::config::Config;
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
            preferred_device_uid: None,
            active_device_uid: active_uid(list),
            matches_preferred: None,
            preferred_present: None,
            preferred_alive: None,
            auto_switch: None,
            message: format!("no config at {path_hint}"),
        };
    };

    if config.preferred_uid_is_placeholder() {
        return PolicyStatus {
            configured: true,
            config_path: config_path.map(|p| p.display().to_string()),
            preferred_device_uid: Some(config.preferred_device_uid.clone()),
            active_device_uid: active_uid(list),
            matches_preferred: None,
            preferred_present: None,
            preferred_alive: None,
            auto_switch: Some(config.auto_switch),
            message: "preferred_device_uid not set — copy a UID from `rusty-jack list`".into(),
        };
    }

    let preferred = &config.preferred_device_uid;
    let preferred_device = find_device(&list.devices, preferred);
    let active = active_uid(list);
    let matches = active.as_deref() == Some(preferred.as_str());

    let message = if preferred_device.is_none() {
        format!("preferred device `{preferred}` is not connected")
    } else if preferred_device.is_some_and(|d| !d.is_alive) {
        format!("preferred device `{preferred}` is not alive")
    } else if matches {
        "active output matches preferred device".into()
    } else {
        format!(
            "active output is `{}`; preferred is `{preferred}`",
            active.as_deref().unwrap_or("(none)")
        )
    };

    PolicyStatus {
        configured: true,
        config_path: config_path.map(|p| p.display().to_string()),
        preferred_device_uid: Some(preferred.clone()),
        active_device_uid: active,
        matches_preferred: Some(matches),
        preferred_present: Some(preferred_device.is_some()),
        preferred_alive: preferred_device.map(|d| d.is_alive),
        auto_switch: Some(config.auto_switch),
        message,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn hdmi(uid: &str, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
            monitor_name: Some("TV".into()),
        }
    }

    fn config_with(preferred: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            preferred_device_uid: preferred.into(),
            fallback_uids: vec![],
        }
    }

    #[test]
    fn test_no_config_not_configured() {
        let list = DeviceList {
            devices: vec![hdmi("hdmi-1", true)],
            system_default: None,
        };
        let policy = evaluate_policy(&list, None, None);
        assert!(!policy.configured);
        assert!(policy.message.contains("no config"));
    }

    #[test]
    fn test_matches_preferred() {
        let list = DeviceList {
            devices: vec![hdmi("hdmi-1", true)],
            system_default: None,
        };
        let cfg = config_with("hdmi-1");
        let policy = evaluate_policy(&list, Some(&cfg), None);
        assert_eq!(policy.matches_preferred, Some(true));
        assert!(policy.message.contains("matches preferred"));
    }

    #[test]
    fn test_does_not_match_preferred() {
        let list = DeviceList {
            devices: vec![
                hdmi("hdmi-1", true),
                hdmi("hdmi-2", false),
            ],
            system_default: None,
        };
        let cfg = config_with("hdmi-2");
        let policy = evaluate_policy(&list, Some(&cfg), None);
        assert_eq!(policy.matches_preferred, Some(false));
        assert!(policy.message.contains("hdmi-1"));
    }

    #[test]
    fn test_placeholder_preferred() {
        let list = DeviceList {
            devices: vec![hdmi("hdmi-1", true)],
            system_default: None,
        };
        let cfg = config_with("PASTE-UID-FROM-rusty-jack-list");
        let policy = evaluate_policy(&list, Some(&cfg), None);
        assert!(policy.configured);
        assert!(policy.matches_preferred.is_none());
        assert!(policy.message.contains("not set"));
    }
}
