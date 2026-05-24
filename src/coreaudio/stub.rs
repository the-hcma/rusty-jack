//! Non-macOS stub (compile-check only).

use crate::coreaudio::traits::AudioHal;
use crate::system_default::DeviceList;
use crate::RustyJackError;

/// Placeholder HAL for non-macOS targets.
#[derive(Debug, Default)]
pub struct StubHal;

impl AudioHal for StubHal {
    fn list_outputs(&self) -> Result<DeviceList, RustyJackError> {
        Err(RustyJackError::CoreAudio(
            "rusty-jack requires macOS (CoreAudio is not available on this platform)".into(),
        ))
    }

    fn set_default_output(&self, _uid: &str, _also_system: bool) -> Result<(), RustyJackError> {
        Err(RustyJackError::CoreAudio(
            "rusty-jack requires macOS (CoreAudio is not available on this platform)".into(),
        ))
    }

    fn default_output_uid(&self) -> Result<Option<String>, RustyJackError> {
        Err(RustyJackError::CoreAudio(
            "rusty-jack requires macOS (CoreAudio is not available on this platform)".into(),
        ))
    }
}
