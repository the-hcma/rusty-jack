//! eqMac presence detection and auto-launch for HDMI software volume.

use crate::config::default_config_path;
use crate::output_device::OutputDevice;
use crate::RustyJackError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const EQMAC_APP_NAME: &str = "eqMac";
const EQMAC_APP_PATH: &str = "/Applications/eqMac.app";
/// HAL driver shipped inside the eqMac app bundle (fallback when managed backup is gone).
pub const EQMAC_EMBEDDED_DRIVER_PATH: &str =
    "/Applications/eqMac.app/Contents/Resources/Embedded/eqMac.driver";
pub const EQMAC_HAL_DRIVER_PATH: &str = "/Library/Audio/Plug-Ins/HAL/eqMac.driver";
const EQMAC_STARTUP_WAIT: Duration = Duration::from_millis(1500);
const EQMAC_DRIVER_BACKUP_DIR_NAME: &str = "driver-backups";
const EQMAC_DRIVER_BACKUP_METADATA_NAME: &str = "eqMac.driver.json";

/// Whether eqMac is present on the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EqMacInstallState {
    NotInstalled,
    Installed,
}

/// What `ensure_eqmac_running` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EqMacEnsureAction {
    /// Target route does not need eqMac (e.g. built-in only).
    NotNeeded,
    /// eqMac was already running.
    AlreadyRunning,
    /// eqMac was launched successfully.
    Launched,
    /// eqMac was restarted to recover a stale route.
    Restarted,
    /// HDMI-class route but eqMac is not installed.
    NotInstalled,
}

/// Outcome of ensuring eqMac is available for software volume on HDMI/DP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EqMacEnsureResult {
    pub action: EqMacEnsureAction,
}

/// Metadata for a managed backup of the eqMac HAL driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EqMacDriverBackupInfo {
    pub original_path: String,
    pub backup_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backed_up_at_unix: Option<u64>,
}

/// True when routing to `uid` needs a virtual volume router (HDMI-class outputs).
#[must_use]
pub fn routing_needs_eqmac(devices: &[OutputDevice], uid: &str) -> bool {
    devices
        .iter()
        .find(|d| d.uid == uid)
        .is_some_and(|d| d.transport.is_hdmi_class())
}

/// Detect whether the eqMac app is installed.
#[must_use]
pub fn eqmac_install_state() -> EqMacInstallState {
    if eqmac_app_path().is_some() {
        EqMacInstallState::Installed
    } else {
        EqMacInstallState::NotInstalled
    }
}

/// Path to the eqMac app bundle when present.
#[must_use]
pub fn eqmac_app_path() -> Option<String> {
    Path::new(EQMAC_APP_PATH)
        .exists()
        .then(|| EQMAC_APP_PATH.to_string())
}

/// Path to the eqMac HAL driver when present.
#[must_use]
pub fn eqmac_hal_driver_path() -> Option<String> {
    Path::new(EQMAC_HAL_DRIVER_PATH)
        .exists()
        .then(|| EQMAC_HAL_DRIVER_PATH.to_string())
}

/// Managed backup directory for temporary eqMac HAL driver swaps.
#[must_use]
pub fn eqmac_driver_backup_dir() -> Option<PathBuf> {
    default_config_path()
        .as_deref()
        .map(eqmac_driver_backup_dir_for_config_path)
}

#[must_use]
pub fn eqmac_driver_backup_dir_for_config_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(EQMAC_DRIVER_BACKUP_DIR_NAME)
}

/// Managed backup path for the eqMac HAL driver bundle.
#[must_use]
pub fn eqmac_driver_backup_path() -> Option<PathBuf> {
    eqmac_driver_backup_dir().map(|dir| dir.join("eqMac.driver"))
}

/// Managed metadata path for the eqMac HAL driver backup.
#[must_use]
pub fn eqmac_driver_backup_metadata_path() -> Option<PathBuf> {
    eqmac_driver_backup_dir().map(|dir| dir.join(EQMAC_DRIVER_BACKUP_METADATA_NAME))
}

/// Current managed eqMac HAL driver backup, using metadata when available.
#[must_use]
pub fn eqmac_driver_backup_info() -> Option<EqMacDriverBackupInfo> {
    if let Some(path) = eqmac_driver_backup_metadata_path() {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if let Ok(info) = serde_json::from_str::<EqMacDriverBackupInfo>(&contents) {
                return Some(info);
            }
        }
    }

    let backup_path = eqmac_driver_backup_path()?;
    backup_path.exists().then(|| EqMacDriverBackupInfo {
        original_path: EQMAC_HAL_DRIVER_PATH.into(),
        backup_path: backup_path.to_string_lossy().into_owned(),
        version: crate::hal_plugin::driver_bundle_info(&backup_path).and_then(|info| info.version),
        backed_up_at_unix: None,
    })
}

pub fn write_eqmac_driver_backup_info(
    backup_path: &Path,
    version: Option<String>,
) -> Result<EqMacDriverBackupInfo, RustyJackError> {
    let metadata_path = eqmac_driver_backup_metadata_path().ok_or_else(|| {
        RustyJackError::Config("HOME is not set; cannot locate eqMac driver backup metadata".into())
    })?;
    if let Some(parent) = metadata_path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }

    let info = EqMacDriverBackupInfo {
        original_path: EQMAC_HAL_DRIVER_PATH.into(),
        backup_path: backup_path.to_string_lossy().into_owned(),
        version,
        backed_up_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs()),
    };
    let json = serde_json::to_string_pretty(&info).map_err(|err| {
        RustyJackError::Config(format!("backup metadata serialization failed: {err}"))
    })?;
    std::fs::write(metadata_path, format!("{json}\n")).map_err(RustyJackError::Io)?;
    Ok(info)
}

pub fn remove_eqmac_driver_backup_info() -> Result<(), RustyJackError> {
    let Some(path) = eqmac_driver_backup_metadata_path() else {
        return Ok(());
    };
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(RustyJackError::Io(err)),
    }
}

/// A leftover eqMac HAL driver without the app bundle is not a usable fallback.
#[must_use]
pub fn orphaned_eqmac_hal_driver_path() -> Option<String> {
    if eqmac_app_path().is_none() {
        eqmac_hal_driver_path()
    } else {
        None
    }
}

/// True when the eqMac application process is running.
#[must_use]
pub fn eqmac_is_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", EQMAC_APP_NAME])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Start eqMac if installed, not running, and the target route needs software volume.
///
/// # Errors
///
/// Returns an error when eqMac is installed but `open` fails to launch the app.
pub fn ensure_eqmac_for_target(
    devices: &[OutputDevice],
    target_uid: &str,
) -> Result<EqMacEnsureResult, RustyJackError> {
    if !routing_needs_eqmac(devices, target_uid) {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotNeeded,
        });
    }

    ensure_eqmac_running()
}

/// Start eqMac when installed but not running.
///
/// # Errors
///
/// Returns an error when eqMac is installed but `open` fails to launch the app
/// for reasons other than the app being unavailable.
pub fn ensure_eqmac_running() -> Result<EqMacEnsureResult, RustyJackError> {
    if eqmac_install_state() == EqMacInstallState::NotInstalled {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        });
    }

    if eqmac_is_running() {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::AlreadyRunning,
        });
    }

    match launch_eqmac_app()? {
        EqMacLaunchAction::Launched => {
            thread::sleep(EQMAC_STARTUP_WAIT);
            Ok(EqMacEnsureResult {
                action: EqMacEnsureAction::Launched,
            })
        }
        EqMacLaunchAction::NotInstalled => Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        }),
    }
}

/// Restart eqMac for a target route that needs HDMI/DP software volume.
///
/// # Errors
///
/// Returns an error when eqMac is installed but cannot be relaunched.
pub fn restart_eqmac_for_target(
    devices: &[OutputDevice],
    target_uid: &str,
) -> Result<EqMacEnsureResult, RustyJackError> {
    if !routing_needs_eqmac(devices, target_uid) {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotNeeded,
        });
    }
    if eqmac_install_state() == EqMacInstallState::NotInstalled {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        });
    }

    if eqmac_is_running() {
        quit_eqmac_app();
        thread::sleep(EQMAC_STARTUP_WAIT);
        if eqmac_is_running() {
            kill_eqmac_app();
            thread::sleep(EQMAC_STARTUP_WAIT);
        }
    }

    match launch_eqmac_app()? {
        EqMacLaunchAction::Launched => {
            thread::sleep(EQMAC_STARTUP_WAIT);
            Ok(EqMacEnsureResult {
                action: EqMacEnsureAction::Restarted,
            })
        }
        EqMacLaunchAction::NotInstalled => Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EqMacLaunchAction {
    Launched,
    NotInstalled,
}

fn launch_eqmac_app() -> Result<EqMacLaunchAction, RustyJackError> {
    let output = std::process::Command::new("open")
        .args(["-a", EQMAC_APP_NAME])
        .output()
        .map_err(RustyJackError::Io)?;

    classify_eqmac_launch(
        output.status.success(),
        &String::from_utf8_lossy(&output.stderr),
    )
}

fn quit_eqmac_app() {
    let _ = std::process::Command::new("osascript")
        .args(["-e", "tell application \"eqMac\" to quit"])
        .output();
}

fn kill_eqmac_app() {
    let _ = std::process::Command::new("pkill")
        .args(["-x", EQMAC_APP_NAME])
        .output();
}

fn classify_eqmac_launch(success: bool, stderr: &str) -> Result<EqMacLaunchAction, RustyJackError> {
    if success {
        return Ok(EqMacLaunchAction::Launched);
    }
    if stderr.contains("Unable to find application named") {
        return Ok(EqMacLaunchAction::NotInstalled);
    }

    Err(RustyJackError::Launchd(format!(
        "failed to launch eqMac: {stderr}"
    )))
}

/// Human-readable lines for stderr after ensuring eqMac.
#[must_use]
pub fn format_ensure_messages(result: EqMacEnsureResult) -> Vec<String> {
    match result.action {
        EqMacEnsureAction::NotNeeded | EqMacEnsureAction::AlreadyRunning => vec![],
        EqMacEnsureAction::Launched => {
            vec!["Started eqMac (software volume for HDMI/DisplayPort).".into()]
        }
        EqMacEnsureAction::Restarted => {
            vec!["Restarted eqMac to recover HDMI/DisplayPort audio.".into()]
        }
        EqMacEnsureAction::NotInstalled => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn device(uid: &str, transport: TransportKind) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "Out".into(),
            transport,
            is_alive: true,
            is_default: false,
            is_active: false,
        }
    }

    #[test]
    fn test_routing_needs_eqmac_for_hdmi() {
        let devices = vec![device("hdmi", TransportKind::Hdmi)];
        assert!(routing_needs_eqmac(&devices, "hdmi"));
    }

    #[test]
    fn test_routing_needs_eqmac_not_for_builtin() {
        let devices = vec![device("builtin", TransportKind::BuiltIn)];
        assert!(!routing_needs_eqmac(&devices, "builtin"));
    }

    #[test]
    fn test_format_ensure_launched_message() {
        let lines = format_ensure_messages(EqMacEnsureResult {
            action: EqMacEnsureAction::Launched,
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Started eqMac"));
    }

    #[test]
    fn test_format_ensure_restarted_message() {
        let lines = format_ensure_messages(EqMacEnsureResult {
            action: EqMacEnsureAction::Restarted,
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Restarted eqMac"));
    }

    #[test]
    fn test_format_ensure_not_installed_stays_quiet() {
        let lines = format_ensure_messages(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        });
        assert!(lines.is_empty());
    }

    #[test]
    fn test_missing_eqmac_app_is_not_fatal() {
        let result =
            classify_eqmac_launch(false, "Unable to find application named 'eqMac'\n").unwrap();
        assert_eq!(result, EqMacLaunchAction::NotInstalled);
    }

    #[test]
    fn test_eqmac_driver_backup_dir_uses_config_parent() {
        let backup_dir = eqmac_driver_backup_dir_for_config_path(Path::new(
            "/Users/example/.config/rusty-jack/config.json",
        ));
        assert_eq!(
            backup_dir,
            PathBuf::from("/Users/example/.config/rusty-jack/driver-backups")
        );
    }

    #[test]
    fn test_other_eqmac_launch_failure_stays_fatal() {
        let err = classify_eqmac_launch(false, "permission denied").unwrap_err();
        assert!(matches!(err, RustyJackError::Launchd(_)));
    }
}
