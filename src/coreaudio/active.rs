//! Resolve which listed device is actively playing system audio.

use crate::output_device::OutputDevice;
use crate::transport::TransportKind;

fn is_builtin_route_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower.contains("built-in")
        || lower.contains("builtin")
        || lower.contains("internal speaker")
        || (lower.contains("macbook") && lower.contains("speaker"))
        || lower == "speakers"
}

fn matches_eqmac_route_target(device: &OutputDevice, target: &str) -> bool {
    if device.name.eq_ignore_ascii_case(target) {
        return true;
    }
    if is_builtin_route_label(target) && device.transport == TransportKind::BuiltIn {
        return true;
    }
    false
}

/// Map the system default output to a UID in `devices` (handles virtual routers like eqMac).
#[must_use]
pub fn resolve_active_uid(
    default_uid: &str,
    default_name: &str,
    devices: &[OutputDevice],
) -> Option<String> {
    if devices.iter().any(|d| d.uid == default_uid) {
        return Some(default_uid.to_string());
    }

    if !OutputDevice::is_excluded_by_name(default_name)
        && !default_uid.contains("EQM")
        && !default_uid.to_ascii_lowercase().contains("eqmac")
    {
        return None;
    }

    if let Some(stripped) = default_name.strip_suffix(" (eqMac)") {
        let target = stripped.trim();
        if let Some(d) = devices
            .iter()
            .find(|d| matches_eqmac_route_target(d, target))
        {
            return Some(d.uid.clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportKind;

    fn device(uid: &str, name: &str, transport: TransportKind) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: name.into(),
            transport,
            is_alive: true,
            is_default: false,
            is_active: false,
        }
    }

    #[test]
    fn test_direct_default_match() {
        let devices = vec![device("builtin", "Speakers", TransportKind::BuiltIn)];
        assert_eq!(
            resolve_active_uid("builtin", "Speakers", &devices).as_deref(),
            Some("builtin")
        );
    }

    #[test]
    fn test_eqmac_virtual_maps_to_device_name() {
        let devices = vec![
            device("hdmi", "HDMI", TransportKind::Hdmi),
            device("dp", "DisplayPort", TransportKind::DisplayPort),
        ];
        assert_eq!(
            resolve_active_uid("EQMOutputCapture", "HDMI (eqMac)", &devices).as_deref(),
            Some("hdmi")
        );
    }

    #[test]
    fn test_eqmac_virtual_maps_to_monitor_name() {
        let devices = vec![device("hdmi", "DELL U3219Q", TransportKind::Hdmi)];
        assert_eq!(
            resolve_active_uid("EQMOutputCapture", "DELL U3219Q (eqMac)", &devices).as_deref(),
            Some("hdmi")
        );
    }

    #[test]
    fn test_eqmac_internal_speakers_maps_to_builtin() {
        let devices = vec![
            device("builtin", "Built-in Output", TransportKind::BuiltIn),
            device("hdmi", "DELL U3219Q", TransportKind::Hdmi),
        ];
        assert_eq!(
            resolve_active_uid("EQMOutputCapture", "Internal Speakers (eqMac)", &devices,)
                .as_deref(),
            Some("builtin")
        );
    }

    #[test]
    fn test_eqmac_built_in_output_maps_to_builtin() {
        let devices = vec![device("builtin", "Built-in Output", TransportKind::BuiltIn)];
        assert_eq!(
            resolve_active_uid("EQMOutputCapture", "Built-in Output (eqMac)", &devices,).as_deref(),
            Some("builtin")
        );
    }
}
