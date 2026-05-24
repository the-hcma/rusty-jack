//! Live CoreAudio HAL (`coreaudio-sys`).

use crate::coreaudio::property::{
    all_device_ids, default_output_device_id, device_has_output_streams, device_is_alive,
    device_name, device_transport_type, device_uid,
};
use crate::coreaudio::resolve_active_uid;
#[cfg(target_os = "macos")]
use crate::coreaudio::system_default::build_system_default_info;
use crate::coreaudio::traits::AudioHal;
use crate::display;
use crate::output_device::OutputDevice;
use crate::system_default::DeviceList;
use crate::transport::TransportKind;
use crate::RustyJackError;

/// CoreAudio-backed [`AudioHal`].
#[derive(Debug, Default)]
pub struct CoreAudioHal;

impl CoreAudioHal {
    pub fn new() -> Result<Self, RustyJackError> {
        Ok(Self)
    }
}

impl AudioHal for CoreAudioHal {
    fn list_outputs(&self) -> Result<DeviceList, RustyJackError> {
        let default_id = default_output_device_id();
        let default_uid = default_id
            .and_then(|id| device_uid(id).ok())
            .unwrap_or_default();
        let default_name = default_id
            .and_then(|id| device_name(id).ok())
            .unwrap_or_default();
        let ids = all_device_ids()?;
        let mut out = Vec::new();

        for id in ids {
            if !device_has_output_streams(id) {
                continue;
            }

            let uid = match device_uid(id) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let name = device_name(id).unwrap_or_else(|_| uid.clone());
            if OutputDevice::is_excluded_by_name(&name) {
                continue;
            }

            let transport_code = device_transport_type(id).unwrap_or(0);
            let transport = TransportKind::from_fourcc(transport_code);
            let is_alive = device_is_alive(id).unwrap_or(false);
            let monitor_name = if transport.is_hdmi_class() {
                display::monitor_name_for_audio_uid(&uid)
            } else {
                None
            };

            out.push(OutputDevice {
                id,
                uid,
                name,
                transport,
                is_alive,
                is_default: false,
                is_active: false,
                monitor_name,
            });
        }

        let active_uid = resolve_active_uid(&default_uid, &default_name, &out);
        for device in &mut out {
            device.is_default = default_id == Some(device.id);
            device.is_active = active_uid.as_deref() == Some(device.uid.as_str());
        }

        out.sort_by_key(|d| d.name.to_lowercase());

        #[cfg(target_os = "macos")]
        let system_default =
            build_system_default_info(&out, active_uid.as_deref());
        #[cfg(not(target_os = "macos"))]
        let system_default = None;

        Ok(DeviceList {
            devices: out,
            system_default,
        })
    }
}
