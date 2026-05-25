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
/// Returns an error when eqMac is installed but `open` fails to launch the app.
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

    launch_eqmac_app()?;
    thread::sleep(EQMAC_STARTUP_WAIT);

    Ok(EqMacEnsureResult {
        action: EqMacEnsureAction::Launched,
    })
}

fn launch_eqmac_app() -> Result<(), RustyJackError> {
    let output = std::process::Command::new("open")
        .args(["-a", EQMAC_APP_NAME])
        .output()
        .map_err(RustyJackError::Io)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(RustyJackError::Launchd(format!(
            "failed to launch eqMac: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
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
            "warning: eqMac is not installed — volume keys on HDMI/DisplayPort may not work."
                .into(),
            "  Install eqMac from https://eqmac.app or wait for rusty-jack's own virtual driver."
                .into(),
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
}
