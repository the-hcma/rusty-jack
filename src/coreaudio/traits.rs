//! Testable abstraction over CoreAudio.

use crate::volume_result::VolumeEnsureResult;
use crate::system_default::DeviceList;
use crate::RustyJackError;

/// Hardware abstraction for listing and selecting output devices.
pub trait AudioHal: Send + Sync {
    fn list_outputs(&self) -> Result<DeviceList, RustyJackError>;

    /// Set the system default output to `uid`. When `also_system` is true, also set the
    /// default system (alert) output to the same device.
    fn set_default_output(&self, uid: &str, also_system: bool) -> Result<(), RustyJackError>;

    /// Set output volume (0–100) on `uid`, retrying until readback matches when possible.
    fn set_output_volume(
        &self,
        uid: &str,
        percent: u8,
    ) -> Result<VolumeEnsureResult, RustyJackError>;

    /// UID of the current default output device.
    fn default_output_uid(&self) -> Result<Option<String>, RustyJackError>;

    /// Current output volume (0–100) for `uid`, when readable.
    fn output_volume_percent(&self, uid: &str) -> Option<u8>;
}
