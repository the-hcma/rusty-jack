//! HAL plugin scanning (platform-specific).

/// Parsed `codesign -dv` authority line for a driver bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverBundleSignature {
    Unsigned,
    AdHoc,
    Authority(String),
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    driver_bundle_has_developer_id_signature, driver_bundle_info, driver_bundle_signature,
    installed_hal_drivers, match_hal_driver,
};

#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub use stub::{
    driver_bundle_has_developer_id_signature, driver_bundle_info, driver_bundle_signature,
    installed_hal_drivers, match_hal_driver,
};
