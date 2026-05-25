//! Build [`SystemDefaultInfo`] from CoreAudio default output.

use crate::coreaudio::property::{
    default_output_device_id, device_manufacturer, device_model_uid, device_name,
    device_plugin_bundle_id, device_transport_type, device_uid,
};
use crate::hal_plugin::match_hal_driver;
use crate::output_device::OutputDevice;
use crate::system_default::{identify_router, routed_to_label, SystemDefaultInfo};
use crate::transport::TransportKind;

/// Describe the current system default when it is a virtual router not shown in `devices`.
#[must_use]
pub fn build_system_default_info(
    devices: &[OutputDevice],
    active_uid: Option<&str>,
) -> Option<SystemDefaultInfo> {
    let default_id = default_output_device_id()?;
    let uid = device_uid(default_id).ok()?;
    let name = device_name(default_id).unwrap_or_else(|_| uid.clone());
    let transport = TransportKind::from_fourcc(device_transport_type(default_id).unwrap_or(0));
    let manufacturer = device_manufacturer(default_id);
    let model_uid = device_model_uid(default_id);
    let plugin_bundle_id = device_plugin_bundle_id(default_id);

    let driver = match_hal_driver(
        &uid,
        &name,
        manufacturer.as_deref(),
        plugin_bundle_id.as_deref(),
    );
    let router = identify_router(&uid, &name, manufacturer.as_deref(), driver.as_ref());

    let is_virtual = transport == TransportKind::Virtual
        || driver.is_some()
        || router.is_some()
        || uid.contains("EQM")
        || name.contains("(eqMac)")
        || OutputDevice::is_excluded_by_name(&name);

    if devices.iter().any(|d| d.uid == uid) {
        return None;
    }

    if !is_virtual {
        return None;
    }

    let routed_to_uid = active_uid
        .filter(|active| *active != uid.as_str())
        .map(str::to_string);
    let routed_to_label = routed_to_uid
        .as_deref()
        .and_then(|u| routed_to_label(devices, u));

    Some(SystemDefaultInfo {
        uid,
        name,
        transport,
        manufacturer,
        model_uid,
        router,
        driver,
        routed_to_uid,
        routed_to_label,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn test_build_system_default_on_hardware() {
        let _ = build_system_default_info(&[], None);
    }
}
