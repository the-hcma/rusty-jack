//! Non-macOS stub for HAL plugin scanning.

use crate::system_default::HalDriverInfo;
use std::path::Path;

#[must_use]
pub fn driver_bundle_info(_path: &Path) -> Option<HalDriverInfo> {
    None
}

#[must_use]
pub fn installed_hal_drivers() -> &'static [HalDriverInfo] {
    &[]
}

#[must_use]
pub fn match_hal_driver(
    _uid: &str,
    _name: &str,
    _manufacturer: Option<&str>,
    _plugin_bundle_id: Option<&str>,
) -> Option<HalDriverInfo> {
    None
}
