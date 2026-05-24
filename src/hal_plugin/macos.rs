//! Installed CoreAudio HAL audio server plug-ins (`.driver` bundles).

use crate::system_default::HalDriverInfo;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

static DRIVERS: OnceLock<Vec<HalDriverInfo>> = OnceLock::new();

fn hal_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Library/Audio/Plug-Ins/HAL")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(
            PathBuf::from(home)
                .join("Library/Audio/Plug-Ins/HAL"),
        );
    }
    dirs
}

fn parse_driver_bundle(path: &Path) -> Option<HalDriverInfo> {
    let plist = path.join("Contents/Info.plist");
    if !plist.is_file() {
        return None;
    }

    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = serde_json::from_slice::<Value>(&output.stdout).ok()?;
    let bundle_id = value.get("CFBundleIdentifier")?.as_str()?.to_string();
    let name = value
        .get("CFBundleName")
        .or_else(|| value.get("CFBundleDisplayName"))
        .and_then(Value::as_str)
        .unwrap_or(bundle_id.as_str())
        .to_string();
    let version = value
        .get("CFBundleShortVersionString")
        .or_else(|| value.get("CFBundleVersion"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(HalDriverInfo {
        name,
        bundle_id,
        version,
        install_path: path.to_string_lossy().into_owned(),
    })
}

fn scan_hal_plugins() -> Vec<HalDriverInfo> {
    let mut drivers = Vec::new();

    for dir in hal_plugin_dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("driver") {
                continue;
            }
            if let Some(info) = parse_driver_bundle(&path) {
                drivers.push(info);
            }
        }
    }

    drivers.sort_by_key(|a| a.name.to_lowercase());
    drivers
}

/// Cached list of installed HAL `.driver` bundles.
#[must_use]
pub fn installed_hal_drivers() -> &'static [HalDriverInfo] {
    DRIVERS.get_or_init(scan_hal_plugins)
}

/// Match a virtual device to an installed HAL driver bundle.
#[must_use]
pub fn match_hal_driver(
    uid: &str,
    name: &str,
    manufacturer: Option<&str>,
    plugin_bundle_id: Option<&str>,
) -> Option<HalDriverInfo> {
    let drivers = installed_hal_drivers();

    if let Some(bundle_id) = plugin_bundle_id {
        if let Some(driver) = drivers.iter().find(|d| d.bundle_id == bundle_id) {
            return Some(driver.clone());
        }
    }

    if uid.contains("EQM") || name.contains("(eqMac)") || manufacturer.is_some_and(|m| m.contains("Bitgapp"))
    {
        return drivers
            .iter()
            .find(|d| d.bundle_id == "com.bitgapp.eqmac.driver" || d.name.eq_ignore_ascii_case("eqMac"))
            .cloned();
    }

    if uid.to_ascii_lowercase().contains("blackhole") || name.to_ascii_lowercase().contains("blackhole") {
        return drivers
            .iter()
            .find(|d| {
                d.bundle_id.to_ascii_lowercase().contains("blackhole")
                    || d.name.to_ascii_lowercase().contains("blackhole")
            })
            .cloned();
    }

    if uid.contains("zoom.us") || name.contains("ZoomAudio") {
        return drivers
            .iter()
            .find(|d| d.bundle_id.contains("zoom") || d.name.to_ascii_lowercase().contains("zoom"))
            .cloned();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_eqmac_driver_by_uid() {
        let driver = match_hal_driver("EQMOutputCapture", "DELL U3219Q (eqMac)", Some("Bitgapp Ltd"), None);
        if installed_hal_drivers().is_empty() {
            return;
        }
        assert!(driver.is_some(), "expected eqMac driver on this machine");
        assert_eq!(driver.unwrap().bundle_id, "com.bitgapp.eqmac.driver");
    }
}
