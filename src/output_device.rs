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

    /// True when the device can be chosen as the system output route.
    #[must_use]
    pub fn is_selectable(&self) -> bool {
        is_routable_output(&self.uid, &self.name, self.transport)
    }

    /// Short explanation when [`Self::is_selectable`] is false.
    #[must_use]
    pub fn non_selectable_reason(&self) -> Option<&'static str> {
        non_selectable_reason(&self.uid, &self.name, self.transport)
    }

    #[must_use]
    pub fn matches_hdmi_filter(&self) -> bool {
        self.transport.is_hdmi_class() && !Self::is_excluded_by_name(&self.name)
    }

    /// Human-readable label (monitor name when available).
    #[must_use]
    pub fn friendly_label(&self) -> String {
        if let Some(monitor) = &self.monitor_name {
            format!("{} ({monitor})", self.name)
        } else {
            self.name.clone()
        }
    }
}

/// Whether this endpoint can route audible system output (excludes app virtual drivers).
#[must_use]
pub fn is_routable_output(uid: &str, name: &str, transport: TransportKind) -> bool {
    if OutputDevice::is_excluded_by_name(name) {
        return false;
    }
    if transport == TransportKind::Aggregate {
        return false;
    }
    !is_app_virtual_output(uid, name)
}

#[must_use]
pub fn non_selectable_reason(
    uid: &str,
    name: &str,
    transport: TransportKind,
) -> Option<&'static str> {
    if OutputDevice::is_excluded_by_name(name) {
        return Some("virtual router entry");
    }
    if transport == TransportKind::Aggregate {
        return Some("aggregate device");
    }
    if is_app_virtual_output(uid, name) {
        return Some("app virtual audio — not a speaker");
    }
    None
}

fn is_app_virtual_output(uid: &str, name: &str) -> bool {
    let uid_lower = uid.to_ascii_lowercase();
    name.contains("ZoomAudio") || uid_lower.contains("zoom.us")
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

    #[test]
    fn test_zoom_device_not_selectable() {
        let zoom = sample(
            "zoom.us:123",
            "ZoomAudioDevice",
            TransportKind::Virtual,
            false,
        );
        assert!(!zoom.is_selectable());
        assert!(zoom
            .non_selectable_reason()
            .unwrap()
            .contains("app virtual"));
    }

    #[test]
    fn test_hdmi_device_is_selectable() {
        let hdmi = sample("hdmi-1", "HDMI", TransportKind::Hdmi, false);
        assert!(hdmi.is_selectable());
    }

    #[test]
    fn test_aggregate_not_selectable() {
        let agg = sample("agg", "Multi Output", TransportKind::Aggregate, false);
        assert!(!agg.is_selectable());
    }
}
