//! Testable abstraction over CoreAudio.

use crate::system_default::DeviceList;
use crate::RustyJackError;

/// Hardware abstraction for listing and selecting output devices.
pub trait AudioHal: Send + Sync {
    fn list_outputs(&self) -> Result<DeviceList, RustyJackError>;
}
