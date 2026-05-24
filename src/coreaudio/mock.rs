//! In-memory HAL for unit tests.

use crate::coreaudio::traits::AudioHal;
use crate::output_device::OutputDevice;
use crate::system_default::DeviceList;
use crate::RustyJackError;

/// Mock [`AudioHal`] returning a fixed device list.
#[derive(Debug, Clone)]
pub struct MockHal {
    devices: Vec<OutputDevice>,
}

impl MockHal {
    #[must_use]
    pub fn new(devices: Vec<OutputDevice>) -> Self {
        Self { devices }
    }
}

impl AudioHal for MockHal {
    fn list_outputs(&self) -> Result<DeviceList, RustyJackError> {
        Ok(DeviceList {
            devices: self.devices.clone(),
            system_default: None,
        })
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
}
