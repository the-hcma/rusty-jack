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
}
