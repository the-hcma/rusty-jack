//! Output device model and filtering.

use crate::transport::TransportKind;
use serde::Serialize;

/// A CoreAudio output endpoint exposed to users and config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputDevice {
    pub id: u32,
    pub uid: String,
    pub name: String,
    pub transport: TransportKind,
    pub is_alive: bool,
    pub is_default: bool,
    /// Device currently routing audible system audio (may differ from default when a virtual router is default).
    pub is_active: bool,
    /// Matched display product name (HDMI/DP), when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_name: Option<String>,
}

impl OutputDevice {
    #[must_use]
    pub fn is_excluded_by_name(name: &str) -> bool {
        name.contains("CADefaultDeviceAggregate") || name.contains("(eqMac)")
    }

    #[must_use]
    pub fn matches_hdmi_filter(&self) -> bool {
        self.transport.is_hdmi_class() && !Self::is_excluded_by_name(&self.name)
    }
}

/// Filter listed devices for `list --hdmi`.
#[must_use]
pub fn filter_hdmi_devices(devices: &[OutputDevice]) -> Vec<OutputDevice> {
    devices
        .iter()
        .filter(|d| d.matches_hdmi_filter())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(uid: &str, name: &str, transport: TransportKind, default: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: name.into(),
            transport,
            is_alive: true,
            is_default: default,
            is_active: default,
            monitor_name: None,
        }
    }

    #[test]
    fn test_filter_hdmi_includes_hdmi_and_dp() {
        let devices = vec![
            sample("builtin", "MacBook Speakers", TransportKind::BuiltIn, true),
            sample("hdmi-1", "LG TV", TransportKind::Hdmi, false),
            sample("dp-1", "Studio Display", TransportKind::DisplayPort, false),
        ];
        let filtered = filter_hdmi_devices(&devices);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|d| d.uid == "hdmi-1"));
        assert!(filtered.iter().any(|d| d.uid == "dp-1"));
    }

    #[test]
    fn test_excludes_aggregate_name() {
        let devices = vec![sample(
            "agg",
            "CADefaultDeviceAggregate-123",
            TransportKind::Hdmi,
            false,
        )];
        assert!(filter_hdmi_devices(&devices).is_empty());
    }

    #[test]
    fn test_is_excluded_by_name() {
        assert!(OutputDevice::is_excluded_by_name("Foo (eqMac)"));
        assert!(!OutputDevice::is_excluded_by_name("CalDigit TS4 Audio"));
    }
}
