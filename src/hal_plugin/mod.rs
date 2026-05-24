//! HAL plugin scanning (platform-specific).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{installed_hal_drivers, match_hal_driver};

#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub use stub::{installed_hal_drivers, match_hal_driver};
