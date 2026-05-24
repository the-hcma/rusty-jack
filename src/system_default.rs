//! System default output device details (especially virtual HAL routers).

use crate::output_device::OutputDevice;
use crate::transport::TransportKind;
use serde::Serialize;

/// Installed CoreAudio HAL plugin (`.driver` bundle).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HalDriverInfo {
    pub name: String,
    pub bundle_id: String,
    pub version: Option<String>,
    pub install_path: String,
}

/// CoreAudio default output when it is a virtual router (eqMac, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemDefaultInfo {
    pub uid: String,
    pub name: String,
    pub transport: TransportKind,
    pub manufacturer: Option<String>,
    pub model_uid: Option<String>,
    /// Friendly router label, e.g. "eqMac".
    pub router: Option<String>,
    pub driver: Option<HalDriverInfo>,
    /// Physical device UID receiving the routed stream (when known).
    pub routed_to_uid: Option<String>,
    pub routed_to_label: Option<String>,
}

impl SystemDefaultInfo {
    #[must_use]
    pub fn is_virtual_router(&self) -> bool {
        self.transport == TransportKind::Virtual
            || self.uid.contains("EQM")
            || self.name.contains("(eqMac)")
            || self
                .manufacturer
                .as_deref()
                .is_some_and(|m| m.contains("Bitgapp"))
            || self.driver.is_some()
    }
}

/// Result of enumerating outputs plus default-device context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceList {
    pub devices: Vec<OutputDevice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_default: Option<SystemDefaultInfo>,
}

/// Pick a human-readable label for the routed physical device.
#[must_use]
pub fn routed_to_label(devices: &[OutputDevice], routed_uid: &str) -> Option<String> {
    devices.iter().find(|d| d.uid == routed_uid).map(|d| {
        if let Some(monitor) = &d.monitor_name {
            format!("{} ({})", d.name, monitor)
        } else {
            d.name.clone()
        }
    })
}

/// Guess router product name from device metadata and optional HAL driver.
#[must_use]
pub fn identify_router(
    uid: &str,
    name: &str,
    manufacturer: Option<&str>,
    driver: Option<&HalDriverInfo>,
) -> Option<String> {
    if let Some(driver) = driver {
        return Some(driver.name.clone());
    }

    if uid.contains("EQM") || name.contains("(eqMac)") {
        return Some("eqMac".into());
    }

    if let Some(m) = manufacturer {
        if m.contains("Bitgapp") {
            return Some("eqMac".into());
        }
    }

    if uid.to_ascii_lowercase().contains("blackhole")
        || name.to_ascii_lowercase().contains("blackhole")
    {
        return Some("BlackHole".into());
    }

    if name.contains("Soundflower") || uid.to_ascii_lowercase().contains("soundflower") {
        return Some("Soundflower".into());
    }

    if name.contains("ZoomAudio") || uid.contains("zoom.us") {
        return Some("Zoom".into());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_eqmac_from_uid() {
        assert_eq!(
            identify_router("EQMOutputCapture", "DELL U3219Q (eqMac)", Some("Bitgapp Ltd"), None),
            Some("eqMac".into())
        );
    }
}
