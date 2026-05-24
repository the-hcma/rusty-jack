//! Build and format `rusty-jack status` output.

use crate::list_fmt;
use crate::system_default::DeviceList;
use anyhow::Result;
use serde::Serialize;
use std::io::{self, Write};

/// Policy evaluation state (full policy arrives in Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyStatus {
    pub configured: bool,
    pub matches_preferred: Option<bool>,
    pub message: String,
}

/// Snapshot returned by `rusty-jack status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusSnapshot {
    pub devices: Vec<crate::output_device::OutputDevice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_default: Option<crate::system_default::SystemDefaultInfo>,
    pub policy: PolicyStatus,
}

impl From<DeviceList> for StatusSnapshot {
    fn from(list: DeviceList) -> Self {
        Self {
            devices: list.devices,
            system_default: list.system_default,
            policy: PolicyStatus {
                configured: false,
                matches_preferred: None,
                message: "no config loaded yet (Phase 3: `config init` / `apply`)".into(),
            },
        }
    }
}

/// Build a status snapshot from a device list.
#[must_use]
pub fn build_status(list: DeviceList) -> StatusSnapshot {
    list.into()
}

fn format_policy_line(policy: &PolicyStatus) -> String {
    let match_line = policy
        .matches_preferred
        .map(|m| format!("\n  matches preferred: {}", if m { "yes" } else { "no" }))
        .unwrap_or_default();
    format!(
        "Policy\n  configured: {}{match_line}\n  note:       {}",
        if policy.configured { "yes" } else { "no" },
        policy.message
    )
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
    writeln!(out, "{}", format_policy_line(&snapshot.policy))?;
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
    fn test_build_status_includes_policy() {
        let snapshot = build_status(DeviceList {
            devices: vec![hdmi_device(true)],
            system_default: None,
        });
        assert_eq!(snapshot.devices.len(), 1);
        assert!(!snapshot.policy.configured);
    }

    #[test]
    fn test_build_status_preserves_virtual_default() {
        let snapshot = build_status(DeviceList {
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
        });
        assert!(snapshot.system_default.is_some());
    }

    #[test]
    fn test_format_policy_line_unconfigured() {
        let line = format_policy_line(&PolicyStatus {
            configured: false,
            matches_preferred: None,
            message: "no config".into(),
        });
        assert!(line.contains("configured: no"));
        assert!(line.contains("no config"));
    }

    #[test]
    fn test_print_json_serializes() {
        let snapshot = build_status(DeviceList {
            devices: vec![hdmi_device(true)],
            system_default: None,
        });
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"devices\""));
        assert!(json.contains("\"policy\""));
    }
}
