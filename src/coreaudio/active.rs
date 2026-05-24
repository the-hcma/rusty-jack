//! Resolve which listed device is actively playing system audio.

use crate::output_device::OutputDevice;

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
        if let Some(d) = devices.iter().find(|d| {
            d.monitor_name.as_deref() == Some(target) || d.name.eq_ignore_ascii_case(target)
        }) {
            return Some(d.uid.clone());
        }
    }

    for d in devices {
        if let Some(monitor) = &d.monitor_name {
            if default_name.contains(monitor.as_str()) {
                return Some(d.uid.clone());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportKind;

    fn device(uid: &str, name: &str, monitor: Option<&str>) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: name.into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: monitor.map(str::to_string),
        }
    }

    #[test]
    fn test_direct_default_match() {
        let devices = vec![device("builtin", "Speakers", None)];
        assert_eq!(
            resolve_active_uid("builtin", "Speakers", &devices).as_deref(),
            Some("builtin")
        );
    }

    #[test]
    fn test_eqmac_virtual_maps_to_monitor() {
        let devices = vec![
            device(
                "hdmi",
                "HDMI",
                Some("DELL U3219Q"),
            ),
            device("dp", "DisplayPort", Some("DELL U3223QE")),
        ];
        assert_eq!(
            resolve_active_uid(
                "EQMOutputCapture",
                "DELL U3219Q (eqMac)",
                &devices,
            )
            .as_deref(),
            Some("hdmi")
        );
    }
}
