//! HAL plugin scanning (platform-specific).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{driver_bundle_info, installed_hal_drivers, match_hal_driver};

#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub use stub::{driver_bundle_info, installed_hal_drivers, match_hal_driver};
