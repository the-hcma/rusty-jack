//! In-memory HAL for unit tests.

use crate::coreaudio::traits::AudioHal;
use crate::output_device::OutputDevice;
use crate::system_default::DeviceList;
use crate::RustyJackError;
use std::sync::Mutex;

/// Recorded call to [`AudioHal::set_default_output`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetDefaultCall {
    pub uid: String,
    pub also_system: bool,
}

#[derive(Debug, Default)]
struct MockState {
    default_uid: Option<String>,
    set_calls: Vec<SetDefaultCall>,
}

/// Mock [`AudioHal`] returning a fixed device list.
#[derive(Debug)]
pub struct MockHal {
    devices: Vec<OutputDevice>,
    state: Mutex<MockState>,
}

impl MockHal {
    #[must_use]
    pub fn new(devices: Vec<OutputDevice>) -> Self {
        Self {
            devices,
            state: Mutex::new(MockState::default()),
        }
    }

    #[must_use]
    pub fn with_default(mut self, uid: &str) -> Self {
        self.state.get_mut().unwrap().default_uid = Some(uid.to_string());
        self
    }

    pub fn set_calls(&self) -> Vec<SetDefaultCall> {
        self.state.lock().unwrap().set_calls.clone()
    }

    fn device_list(&self) -> DeviceList {
        let default_uid = self.state.lock().unwrap().default_uid.clone();
        let mut devices = self.devices.clone();
        for device in &mut devices {
            device.is_default = default_uid.as_deref() == Some(device.uid.as_str());
        }
        DeviceList {
            devices,
            system_default: None,
        }
    }
}

impl AudioHal for MockHal {
    fn list_outputs(&self) -> Result<DeviceList, RustyJackError> {
        Ok(self.device_list())
    }

    fn set_default_output(&self, uid: &str, also_system: bool) -> Result<(), RustyJackError> {
        if !self.devices.iter().any(|d| d.uid == uid) {
            return Err(RustyJackError::CoreAudio(format!(
                "no device with uid `{uid}`"
            )));
        }

        let mut state = self.state.lock().unwrap();
        state.default_uid = Some(uid.to_string());
        state.set_calls.push(SetDefaultCall {
            uid: uid.to_string(),
            also_system,
        });
        Ok(())
    }

    fn default_output_uid(&self) -> Result<Option<String>, RustyJackError> {
        Ok(self.state.lock().unwrap().default_uid.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportKind;

    #[test]
    fn test_mock_hal_returns_devices() {
        let hal = MockHal::new(vec![OutputDevice {
            id: 1,
            uid: "hdmi".into(),
            name: "TV".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: true,
            monitor_name: None,
        }]);
        assert_eq!(hal.list_outputs().unwrap().devices.len(), 1);
    }

    #[test]
    fn test_mock_hal_set_default() {
        let hal = MockHal::new(vec![OutputDevice {
            id: 1,
            uid: "hdmi".into(),
            name: "TV".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: None,
        }]);
        hal.set_default_output("hdmi", true).unwrap();
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi"));
        assert_eq!(hal.set_calls().len(), 1);
    }
}
