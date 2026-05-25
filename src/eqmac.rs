//! eqMac presence detection and auto-launch for HDMI software volume.

use crate::output_device::OutputDevice;
use crate::RustyJackError;
use serde::Serialize;
use std::path::Path;
use std::thread;
use std::time::Duration;

const EQMAC_APP_NAME: &str = "eqMac";
const EQMAC_APP_PATH: &str = "/Applications/eqMac.app";
const EQMAC_HAL_DRIVER_PATH: &str = "/Library/Audio/Plug-Ins/HAL/eqMac.driver";
const EQMAC_STARTUP_WAIT: Duration = Duration::from_millis(1500);

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
    /// HDMI-class route but eqMac is not installed.
    NotInstalled,
}

/// Outcome of ensuring eqMac is available for software volume on HDMI/DP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EqMacEnsureResult {
    pub action: EqMacEnsureAction,
}

/// True when routing to `uid` needs a virtual volume router (HDMI-class outputs).
#[must_use]
pub fn routing_needs_eqmac(devices: &[OutputDevice], uid: &str) -> bool {
    devices
        .iter()
        .find(|d| d.uid == uid)
        .is_some_and(|d| d.transport.is_hdmi_class())
}

/// Detect whether eqMac app or HAL driver is installed.
#[must_use]
pub fn eqmac_install_state() -> EqMacInstallState {
    if Path::new(EQMAC_APP_PATH).exists() || Path::new(EQMAC_HAL_DRIVER_PATH).exists() {
        EqMacInstallState::Installed
    } else {
        EqMacInstallState::NotInstalled
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
        EqMacEnsureAction::NotInstalled => vec![
            "warning: eqMac is not installed; volume buttons cannot control HDMI/DisplayPort output."
                .into(),
            "  Download eqMac from https://eqmac.app to enable software volume control.".into(),
        ],
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
            monitor_name: None,
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
    fn test_format_ensure_not_installed_message_has_url() {
        let lines = format_ensure_messages(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        });
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("volume buttons cannot control HDMI/DisplayPort"));
        assert!(lines[1].contains("https://eqmac.app"));
    }

    #[test]
    fn test_missing_eqmac_app_is_not_fatal() {
        let result =
            classify_eqmac_launch(false, "Unable to find application named 'eqMac'\n").unwrap();
        assert_eq!(result, EqMacLaunchAction::NotInstalled);
    }

    #[test]
    fn test_other_eqmac_launch_failure_stays_fatal() {
        let err = classify_eqmac_launch(false, "permission denied").unwrap_err();
        assert!(matches!(err, RustyJackError::Launchd(_)));
    }
}
