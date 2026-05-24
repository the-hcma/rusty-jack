//! Build and format `rusty-jack status` output.

use crate::list_fmt;
use crate::output_device::OutputDevice;
use crate::system_default::{DeviceList, SystemDefaultInfo};
use crate::transport::TransportKind;
use anyhow::Result;
use serde::Serialize;
use std::io::{self, Write};

/// Summary of one output device for status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceSummary {
    pub uid: String,
    pub name: String,
    pub transport: TransportKind,
    pub is_alive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_name: Option<String>,
}

impl From<&OutputDevice> for DeviceSummary {
    fn from(device: &OutputDevice) -> Self {
        Self {
            uid: device.uid.clone(),
            name: device.name.clone(),
            transport: device.transport,
            is_alive: device.is_alive,
            monitor_name: device.monitor_name.clone(),
        }
    }
}

/// Where macOS routes the system default output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DefaultOutput {
    /// Default is a listed physical (or non-virtual) device.
    Device(DeviceSummary),
    /// Default is a virtual router omitted from the device table (eqMac, etc.).
    Virtual(SystemDefaultInfo),
    /// Could not resolve a default output.
    Unknown,
}

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
    pub active: Option<DeviceSummary>,
    pub default: DefaultOutput,
    pub policy: PolicyStatus,
}

/// Build a status snapshot from a device list.
#[must_use]
pub fn build_status(list: &DeviceList) -> StatusSnapshot {
    let active = list.devices.iter().find(|d| d.is_active).map(DeviceSummary::from);

    let default = if let Some(device) = list.devices.iter().find(|d| d.is_default) {
        DefaultOutput::Device(DeviceSummary::from(device))
    } else if let Some(info) = list.system_default.clone() {
        DefaultOutput::Virtual(info)
    } else {
        DefaultOutput::Unknown
    };

    StatusSnapshot {
        active,
        default,
        policy: PolicyStatus {
            configured: false,
            matches_preferred: None,
            message: "no config loaded yet (Phase 3: `config init` / `apply`)".into(),
        },
    }
}

fn format_device_section(title: &str, device: &DeviceSummary) -> String {
    let monitor = device.monitor_name.as_deref().unwrap_or("-");
    let alive = if device.is_alive { "yes" } else { "no" };
    format!(
        "{title}\n  name:      {}\n  monitor:   {monitor}\n  transport: {}\n  alive:     {alive}\n  uid:       {}",
        device.name, device.transport, device.uid
    )
}

fn format_default_section(default: &DefaultOutput) -> String {
    match default {
        DefaultOutput::Device(device) => format_device_section("Default output", device),
        DefaultOutput::Virtual(info) => list_fmt::format_system_default_block(info),
        DefaultOutput::Unknown => "Default output\n  (unknown)".to_string(),
    }
}

fn format_policy_section(policy: &PolicyStatus) -> String {
    format!(
        "Policy\n  configured: {}\n  note:       {}",
        if policy.configured { "yes" } else { "no" },
        policy.message
    )
}

/// Print human-readable status to stdout.
pub fn print_text(snapshot: &StatusSnapshot) -> Result<()> {
    let mut out = io::stdout().lock();

    if let Some(active) = &snapshot.active {
        writeln!(out, "{}", format_device_section("Active output", active))?;
        writeln!(out)?;
    } else {
        writeln!(out, "Active output\n  (none resolved)")?;
        writeln!(out)?;
    }

    writeln!(out, "{}", format_default_section(&snapshot.default))?;
    writeln!(out)?;
    writeln!(out, "{}", format_policy_section(&snapshot.policy))?;

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
    use crate::system_default::HalDriverInfo;

    fn hdmi_device(uid: &str, active: bool, default: bool) -> OutputDevice {
        OutputDevice {
            id: 2,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: default,
            is_active: active,
            monitor_name: Some("DELL U3219Q".into()),
        }
    }

    #[test]
    fn test_build_status_physical_default() {
        let list = DeviceList {
            devices: vec![hdmi_device("hdmi-1", true, true)],
            system_default: None,
        };
        let status = build_status(&list);
        assert_eq!(status.active.as_ref().map(|d| d.uid.as_str()), Some("hdmi-1"));
        assert!(matches!(status.default, DefaultOutput::Device(_)));
        assert!(!status.policy.configured);
    }

    #[test]
    fn test_build_status_virtual_default() {
        let list = DeviceList {
            devices: vec![hdmi_device("hdmi-1", true, false)],
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
        };
        let status = build_status(&list);
        assert!(matches!(status.default, DefaultOutput::Virtual(_)));
        assert_eq!(status.active.as_ref().map(|d| d.uid.as_str()), Some("hdmi-1"));
    }

    #[test]
    fn test_print_text_sections() {
        let list = DeviceList {
            devices: vec![hdmi_device("hdmi-1", true, true)],
            system_default: None,
        };
        let snapshot = build_status(&list);
        let text = format!(
            "{}\n\n{}\n\n{}",
            format_device_section("Active output", snapshot.active.as_ref().unwrap()),
            format_default_section(&snapshot.default),
            format_policy_section(&snapshot.policy),
        );
        assert!(text.contains("Active output"));
        assert!(text.contains("Default output"));
        assert!(text.contains("Policy"));
        assert!(text.contains("hdmi-1"));
    }

    #[test]
    fn test_print_json_serializes() {
        let list = DeviceList {
            devices: vec![hdmi_device("hdmi-1", true, true)],
            system_default: None,
        };
        let snapshot = build_status(&list);
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"active\""));
        assert!(json.contains("\"configured\":false"));
    }
}
