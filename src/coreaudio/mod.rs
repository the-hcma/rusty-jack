//! CoreAudio HAL access (macOS only).

#[cfg(target_os = "macos")]
mod default_output;
#[cfg(target_os = "macos")]
mod display;
#[cfg(target_os = "macos")]
mod hal;
#[cfg(target_os = "macos")]
mod property;

mod active;
#[cfg(target_os = "macos")]
mod system_default;
#[cfg(target_os = "macos")]
pub mod volume;
pub use active::resolve_active_uid;
#[cfg(target_os = "macos")]
pub use system_default::build_system_default_info;

#[cfg(target_os = "macos")]
pub use default_output::device_id_for_uid;
#[cfg(target_os = "macos")]
pub use hal::CoreAudioHal;

pub mod mock;
mod traits;
pub use traits::AudioHal;

#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub use stub::StubHal;

/// Platform HAL implementation.
pub fn platform_hal() -> anyhow::Result<Box<dyn AudioHal>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(CoreAudioHal::new()?))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(Box::new(StubHal))
    }
}
