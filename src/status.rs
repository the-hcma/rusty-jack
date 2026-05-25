//! Build and format `rusty-jack status` output.

use crate::config::Config;
use crate::list_fmt::{self, format_labeled_section};
use crate::policy::evaluate_policy;
use crate::system_default::DeviceList;
use anyhow::Result;
use serde::Serialize;
use std::io::{self, Write};
use std::path::Path;

/// Policy evaluation for the current routing state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyStatus {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_monitor_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_device_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_device_uid: Option<String>,
    pub matches_preferred: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_alive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_switch: Option<bool>,
    /// Volume (0–100) from config, applied when switching to preferred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_volume: Option<u8>,
    pub message: String,
}

/// Snapshot returned by `rusty-jack status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusSnapshot {
    pub devices: Vec<crate::output_device::OutputDevice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_default: Option<crate::system_default::SystemDefaultInfo>,
    pub policy: PolicyStatus,
    /// Current effective output volume (0–100) for the active route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_percent: Option<u8>,
}

/// Build a status snapshot from a device list and optional config.
#[must_use]
pub fn build_status(
    list: DeviceList,
    config: Option<&Config>,
    config_path: Option<&Path>,
    volume_percent: Option<u8>,
) -> StatusSnapshot {
    let policy = evaluate_policy(
        &DeviceList {
            devices: list.devices.clone(),
            system_default: list.system_default.clone(),
        },
        config,
        config_path,
    );

    StatusSnapshot {
        devices: list.devices,
        system_default: list.system_default,
        policy,
        volume_percent,
    }
}

fn format_policy_block(policy: &PolicyStatus, volume_percent: Option<u8>) -> String {
    let mut rows: Vec<(&str, String)> = vec![(
        "configured",
        if policy.configured {
            "yes".into()
        } else {
            "no".into()
        },
    )];

    if let Some(path) = &policy.config_path {
        rows.push(("config", path.clone()));
    }

    if let Some(name) = &policy.preferred_monitor_name {
        rows.push(("monitor", name.clone()));
    }

    if let Some(uid) = &policy.preferred_device_uid {
        rows.push(("preferred", uid.clone()));
    }

    if let Some(uid) = &policy.active_device_uid {
        rows.push(("active", uid.clone()));
    }

    if let Some(matches) = policy.matches_preferred {
        rows.push((
            "matches",
            if matches { "yes".into() } else { "no".into() },
        ));
    }

    if let Some(auto) = policy.auto_switch {
        rows.push((
            "auto_switch",
            if auto { "yes".into() } else { "no".into() },
        ));
    }

    if let Some(volume) = policy.config_volume {
        rows.push(("config volume", format!("{volume}%")));
    }

    if let Some(volume) = volume_percent {
        rows.push(("volume", format!("{volume}%")));
    }

    rows.push(("note", policy.message.clone()));

    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format_labeled_section("Policy", "  ", &borrowed)
}

/// Print human-readable status: device table, virtual default block, policy.
pub fn print_text(snapshot: &StatusSnapshot) -> Result<()> {
    let list = DeviceList {
        devices: snapshot.devices.clone(),
        system_default: snapshot.system_default.clone(),
    };

    list_fmt::print_device_table(&list)?;
    list_fmt::print_virtual_default_footer(&list)?;
    let mut out = io::stdout().lock();
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        format_policy_block(&snapshot.policy, snapshot.volume_percent)
    )?;
    Ok(())
}

/// Print JSON status to stdout.
pub fn print_json(snapshot: &StatusSnapshot) -> Result<()> {
    let value = serde_json::to_string_pretty(snapshot)?;
    println!("{value}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig};
    use crate::output_device::OutputDevice;
    use crate::system_default::{HalDriverInfo, SystemDefaultInfo};
    use crate::transport::TransportKind;

    fn hdmi_device(active: bool) -> OutputDevice {
        OutputDevice {
            id: 2,
            uid: "hdmi-1".into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
            monitor_name: Some("DELL U3219Q".into()),
        }
    }

    #[test]
    fn test_build_status_without_config() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
            },
            None,
            None,
            Some(42),
        );
        assert!(!snapshot.policy.configured);
        assert_eq!(snapshot.volume_percent, Some(42));
        assert_eq!(snapshot.policy.active_device_uid.as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_build_status_with_matching_config() {
        let config = Config {
            version: 1,
            auto_switch: true,
            preferred_device: DeviceSelectorConfig {
                uid: Some("hdmi-1".into()),
                monitor_name: None,
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            sony_speaker: None,
        };
        let mut config = config;
        config.volume = Some(13);
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
            },
            Some(&config),
            Some(Path::new("/tmp/config.json")),
            Some(13),
        );
        assert!(snapshot.policy.configured);
        assert_eq!(snapshot.policy.matches_preferred, Some(true));
        assert_eq!(snapshot.policy.config_volume, Some(13));
        assert_eq!(snapshot.volume_percent, Some(13));
    }

    #[test]
    fn test_build_status_preserves_virtual_default() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: Some(SystemDefaultInfo {
                    uid: "EQMOutputCapture".into(),
                    name: "DELL U3219Q (eqMac)".into(),
                    transport: TransportKind::Virtual,
                    manufacturer: Some("Bitgapp Ltd".into()),
                    model_uid: None,
                    router: Some("eqMac".into()),
                    driver: Some(HalDriverInfo {
                        name: "eqMac".into(),
                        bundle_id: "com.bitgapp.eqmac.driver".into(),
                        version: Some("2.6.0".into()),
                        install_path: "/Library/Audio/Plug-Ins/HAL/eqMac.driver".into(),
                    }),
                    routed_to_uid: Some("hdmi-1".into()),
                    routed_to_label: Some("HDMI (DELL U3219Q)".into()),
                }),
            },
            None,
            None,
            None,
        );
        assert!(snapshot.system_default.is_some());
    }

    #[test]
    fn test_format_policy_block_includes_volume() {
        let block = format_policy_block(
            &PolicyStatus {
                configured: true,
                config_path: Some("/tmp/c.json".into()),
                preferred_monitor_name: Some("DELL U3219Q".into()),
                preferred_device_uid: Some("hdmi-1".into()),
                active_device_uid: Some("hdmi-1".into()),
                matches_preferred: Some(true),
                preferred_present: Some(true),
                preferred_alive: Some(true),
                auto_switch: Some(true),
                config_volume: Some(13),
                message: "ok".into(),
            },
            Some(13),
        );
        assert!(block.contains("matches"));
        assert!(block.contains("config volume"));
        assert!(block.contains("13%"));
        let detail_lines: Vec<&str> = block
            .lines()
            .filter(|line| line.starts_with("  ") && line.contains(": "))
            .collect();
        let value_starts: Vec<usize> = detail_lines
            .iter()
            .map(|line| line.find(": ").unwrap() + 2)
            .collect();
        assert!(value_starts.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn test_format_policy_block_includes_match_line() {
        let block = format_policy_block(
            &PolicyStatus {
                configured: true,
                config_path: Some("/tmp/c.json".into()),
                preferred_monitor_name: Some("DELL U3219Q".into()),
                preferred_device_uid: Some("hdmi-1".into()),
                active_device_uid: Some("hdmi-1".into()),
                matches_preferred: Some(true),
                preferred_present: Some(true),
                preferred_alive: Some(true),
                auto_switch: Some(true),
                config_volume: None,
                message: "ok".into(),
            },
            None,
        );
        assert!(block.contains("matches"));
        assert!(block.contains("preferred"));
        assert!(block.contains("hdmi-1"));
    }

    #[test]
    fn test_print_json_serializes_policy_fields() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
            },
            None,
            None,
            None,
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"active_device_uid\""));
        assert!(json.contains("\"policy\""));
    }

    #[test]
    fn test_print_json_includes_volume_percent() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
            },
            None,
            None,
            Some(13),
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"volume_percent\":13"));
    }
}
