//! HDMI/DisplayPort audio volume control routing support.

use crate::eqmac::{self, EqMacDriverBackupInfo, EqMacEnsureAction, EqMacInstallState};
use crate::output_device::OutputDevice;
use crate::system_default::HalDriverInfo;
use crate::transport::TransportKind;
use crate::RustyJackError;
use serde::Serialize;

pub const RUSTY_JACK_DRIVER_BUNDLE_ID: &str = "com.the-hcma.rusty-jack.driver";
pub const RUSTY_JACK_DRIVER_NAME: &str = "Rusty Jack";
/// CoreAudio UID published by the Rusty Jack HAL driver virtual output.
pub const RUSTY_JACK_VIRTUAL_OUTPUT_UID: &str = "com.the-hcma.rusty-jack.driver.output";

/// What HDMI/DisplayPort volume-control support did for a selected route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HdmiDisplayPortVolumeControlEnsureAction {
    /// Target route is not an HDMI/DisplayPort output.
    NotNeeded,
    /// Rusty Jack's native driver is installed and should own HDMI/DP volume control.
    NativeDriverInstalled,
    /// eqMac is installed and already running as compatibility fallback.
    EqMacAlreadyRunning,
    /// eqMac was launched as compatibility fallback.
    EqMacLaunched,
    /// eqMac was restarted to recover a stale compatibility-fallback route.
    EqMacRestarted,
    /// HDMI/DisplayPort route, but the Rusty Jack native driver is not installed.
    NativeDriverRecommended,
}

/// Outcome of ensuring HDMI/DisplayPort volume control is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HdmiDisplayPortVolumeControlEnsureResult {
    pub action: HdmiDisplayPortVolumeControlEnsureAction,
}

/// Status shown in `rusty-jack status` and `install --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HdmiDisplayPortVolumeControlStatus {
    pub connected_output_present: bool,
    pub native_driver_installed: bool,
    pub native_driver_recommended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_driver_recommendation_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_driver_install_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_driver_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_driver_stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_driver_warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_driver: Option<HalDriverInfo>,
    pub eqmac_installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eqmac_app_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eqmac_hal_driver_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orphaned_eqmac_hal_driver_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eqmac_driver_backup: Option<EqMacDriverBackupInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
}

/// True when routing to `uid` needs HDMI/DisplayPort audio volume control.
#[must_use]
pub fn route_needs_hdmi_displayport_volume_control(devices: &[OutputDevice], uid: &str) -> bool {
    devices
        .iter()
        .find(|device| device.uid == uid)
        .is_some_and(is_hdmi_displayport_output)
}

/// True when a connected, selectable HDMI or DisplayPort output is visible.
#[must_use]
pub fn connected_hdmi_displayport_output_present(devices: &[OutputDevice]) -> bool {
    devices.iter().any(|device| {
        device.is_alive && device.is_selectable() && is_hdmi_displayport_output(device)
    })
}

/// Installed Rusty Jack HAL driver, when present.
#[must_use]
pub fn native_driver_info() -> Option<HalDriverInfo> {
    let driver_name = RUSTY_JACK_DRIVER_NAME.to_ascii_lowercase();
    crate::hal_plugin::installed_hal_drivers()
        .iter()
        .find(|driver| {
            driver.bundle_id == RUSTY_JACK_DRIVER_BUNDLE_ID
                || driver.name.to_ascii_lowercase().contains(&driver_name)
        })
        .cloned()
}

/// Ensure an HDMI/DisplayPort route has volume control, preferring Rusty Jack's driver over eqMac.
///
/// # Errors
///
/// Returns an error when the eqMac compatibility fallback is installed but cannot be launched.
pub fn ensure_hdmi_displayport_volume_control_for_target(
    devices: &[OutputDevice],
    target_uid: &str,
) -> Result<HdmiDisplayPortVolumeControlEnsureResult, RustyJackError> {
    if !route_needs_hdmi_displayport_volume_control(devices, target_uid) {
        return Ok(result(HdmiDisplayPortVolumeControlEnsureAction::NotNeeded));
    }

    if native_driver_info().is_some() {
        return Ok(result(
            HdmiDisplayPortVolumeControlEnsureAction::NativeDriverInstalled,
        ));
    }

    if eqmac::eqmac_install_state() == EqMacInstallState::NotInstalled {
        return Ok(result(
            HdmiDisplayPortVolumeControlEnsureAction::NativeDriverRecommended,
        ));
    }

    eqmac::ensure_eqmac_for_target(devices, target_uid).map(map_eqmac_result)
}

/// Recover an HDMI/DisplayPort route after wake by restarting eqMac only when it is installed.
///
/// # Errors
///
/// Returns an error when the eqMac compatibility fallback is installed but cannot be relaunched.
pub fn recover_hdmi_displayport_volume_control_for_target(
    devices: &[OutputDevice],
    target_uid: &str,
) -> Result<HdmiDisplayPortVolumeControlEnsureResult, RustyJackError> {
    if !route_needs_hdmi_displayport_volume_control(devices, target_uid) {
        return Ok(result(HdmiDisplayPortVolumeControlEnsureAction::NotNeeded));
    }

    if native_driver_info().is_some() {
        return Ok(result(
            HdmiDisplayPortVolumeControlEnsureAction::NativeDriverInstalled,
        ));
    }

    if eqmac::eqmac_install_state() == EqMacInstallState::NotInstalled {
        return Ok(result(
            HdmiDisplayPortVolumeControlEnsureAction::NativeDriverRecommended,
        ));
    }

    eqmac::restart_eqmac_for_target(devices, target_uid).map(map_eqmac_result)
}

/// Human-readable messages after ensuring HDMI/DisplayPort volume control.
#[must_use]
pub fn format_ensure_messages(result: HdmiDisplayPortVolumeControlEnsureResult) -> Vec<String> {
    match result.action {
        HdmiDisplayPortVolumeControlEnsureAction::NotNeeded
        | HdmiDisplayPortVolumeControlEnsureAction::NativeDriverInstalled
        | HdmiDisplayPortVolumeControlEnsureAction::EqMacAlreadyRunning => vec![],
        HdmiDisplayPortVolumeControlEnsureAction::EqMacLaunched => {
            vec!["Started eqMac for HDMI/DisplayPort audio volume control.".into()]
        }
        HdmiDisplayPortVolumeControlEnsureAction::EqMacRestarted => {
            vec!["Restarted eqMac to recover HDMI/DisplayPort audio.".into()]
        }
        HdmiDisplayPortVolumeControlEnsureAction::NativeDriverRecommended => {
            driver_offer_messages()
        }
    }
}

/// Build status/recommendation for the visible device set.
#[must_use]
pub fn hdmi_displayport_volume_control_status(
    devices: &[OutputDevice],
) -> HdmiDisplayPortVolumeControlStatus {
    hdmi_displayport_volume_control_status_for_target(devices, None)
}

/// Build status/recommendation for a specific route, when one is known.
#[must_use]
pub fn hdmi_displayport_volume_control_status_for_target(
    devices: &[OutputDevice],
    target_uid: Option<&str>,
) -> HdmiDisplayPortVolumeControlStatus {
    let connected_output_present = connected_hdmi_displayport_output_present(devices);
    let native_driver = native_driver_info();
    let native_driver_installed = native_driver.is_some();
    let target_needs_driver =
        target_uid.map(|uid| route_needs_hdmi_displayport_volume_control(devices, uid));
    let native_driver_recommended =
        target_needs_driver.unwrap_or(connected_output_present) && !native_driver_installed;
    let native_driver_recommendation_reason = driver_recommendation_reason(
        target_needs_driver,
        connected_output_present,
        native_driver_installed,
    );
    let native_driver_install_path = native_driver
        .as_ref()
        .map(|driver| driver.install_path.clone());
    let native_driver_scope = native_driver
        .as_ref()
        .map(|driver| native_driver_scope(&driver.install_path).to_string());
    let native_driver_stage = native_driver
        .as_ref()
        .and_then(|driver| driver.stage.clone());
    let mut native_driver_warning = native_driver
        .as_ref()
        .and_then(|driver| native_driver_warning(driver.stage.as_deref()));
    if let Some(extra) = native_driver_virtual_output_warning(devices, native_driver.as_ref()) {
        native_driver_warning = Some(match native_driver_warning {
            Some(existing) => format!("{existing} {extra}"),
            None => extra,
        });
    }
    let eqmac_app_path = eqmac::eqmac_app_path();
    let eqmac_hal_driver_path = eqmac::eqmac_hal_driver_path();
    let orphaned_eqmac_hal_driver_path = eqmac::orphaned_eqmac_hal_driver_path();
    let eqmac_driver_backup = eqmac::eqmac_driver_backup_info();
    let eqmac_installed = eqmac_app_path.is_some();
    let recommendation = if native_driver_recommended {
        Some(driver_offer_message(eqmac_installed))
    } else {
        None
    };

    HdmiDisplayPortVolumeControlStatus {
        connected_output_present,
        native_driver_installed,
        native_driver_recommended,
        native_driver_recommendation_reason,
        native_driver_install_path,
        native_driver_scope,
        native_driver_stage,
        native_driver_warning,
        native_driver,
        eqmac_installed,
        eqmac_app_path,
        eqmac_hal_driver_path,
        orphaned_eqmac_hal_driver_path,
        eqmac_driver_backup,
        recommendation,
    }
}

/// User-facing offer copy for interactive commands.
#[must_use]
pub fn driver_offer_messages() -> Vec<String> {
    vec![
        "warning: HDMI/DisplayPort volume keys need Rusty Jack's native audio driver.".into(),
        "  Install the Rusty Jack driver to control volume for connected HDMI/DisplayPort outputs."
            .into(),
    ]
}

fn is_hdmi_displayport_output(device: &OutputDevice) -> bool {
    matches!(
        device.transport,
        TransportKind::Hdmi | TransportKind::DisplayPort
    )
}

#[must_use]
pub fn native_driver_scope(install_path: &str) -> &'static str {
    let user_hal_suffix = "/Library/Audio/Plug-Ins/HAL/";
    if std::env::var("HOME")
        .ok()
        .is_some_and(|home| install_path.starts_with(&format!("{home}{user_hal_suffix}")))
    {
        "user"
    } else if install_path.starts_with(user_hal_suffix) {
        "system"
    } else {
        "custom"
    }
}

#[must_use]
pub fn native_driver_scope_note(install_path: &str) -> &'static str {
    match native_driver_scope(install_path) {
        "user" => "user-scope HAL driver; no sudo required",
        "system" => "system-scope HAL driver; sudo may be required",
        _ => "custom HAL driver location",
    }
}

fn driver_recommendation_reason(
    target_needs_driver: Option<bool>,
    connected_output_present: bool,
    native_driver_installed: bool,
) -> Option<String> {
    match target_needs_driver {
        Some(false) => Some("selected output is not HDMI/DisplayPort".into()),
        Some(true) if native_driver_installed => Some("native driver is already installed".into()),
        Some(true) => Some("selected output is HDMI/DisplayPort".into()),
        None if native_driver_installed => Some("native driver is already installed".into()),
        None if connected_output_present => {
            Some("connected HDMI/DisplayPort output detected".into())
        }
        None => Some("no connected HDMI/DisplayPort output detected".into()),
    }
}

fn native_driver_virtual_output_warning(
    devices: &[OutputDevice],
    driver: Option<&HalDriverInfo>,
) -> Option<String> {
    let driver = driver?;
    if devices
        .iter()
        .any(|device| device.uid == RUSTY_JACK_VIRTUAL_OUTPUT_UID)
    {
        return None;
    }
    let scope = native_driver_scope(&driver.install_path);
    Some(format!(
        "CoreAudio has not published the Rusty Jack virtual output yet; use a {scope}-scope install under /Library/Audio/Plug-Ins/HAL/, restart coreaudiod, and a signed driver build for production."
    ))
}

fn native_driver_warning(stage: Option<&str>) -> Option<String> {
    match stage {
        Some("virtual-output-null") | Some("loadable-skeleton") | None => Some(
            "Rusty Jack is currently a null output until passthrough audio is implemented.".into(),
        ),
        Some(crate::passthrough::PASSTHROUGH_SKELETON_DRIVER_STAGE) => Some(
            "Rusty Jack passthrough skeleton is armed in the daemon; live CoreAudio capture/render is not wired yet.".into(),
        ),
        Some(crate::passthrough::PASSTHROUGH_ACTIVE_DRIVER_STAGE) => None,
        Some(_) => None,
    }
}

fn driver_offer_message(eqmac_installed: bool) -> String {
    if eqmac_installed {
        "Rusty Jack native audio driver is recommended for HDMI/DisplayPort volume keys; eqMac is installed and remains available as a fallback.".into()
    } else {
        "Rusty Jack native audio driver is recommended for HDMI/DisplayPort volume keys.".into()
    }
}

fn map_eqmac_result(eqmac: eqmac::EqMacEnsureResult) -> HdmiDisplayPortVolumeControlEnsureResult {
    result(match eqmac.action {
        EqMacEnsureAction::NotNeeded => HdmiDisplayPortVolumeControlEnsureAction::NotNeeded,
        EqMacEnsureAction::AlreadyRunning => {
            HdmiDisplayPortVolumeControlEnsureAction::EqMacAlreadyRunning
        }
        EqMacEnsureAction::Launched => HdmiDisplayPortVolumeControlEnsureAction::EqMacLaunched,
        EqMacEnsureAction::Restarted => HdmiDisplayPortVolumeControlEnsureAction::EqMacRestarted,
        EqMacEnsureAction::NotInstalled => {
            HdmiDisplayPortVolumeControlEnsureAction::NativeDriverRecommended
        }
    })
}

fn result(
    action: HdmiDisplayPortVolumeControlEnsureAction,
) -> HdmiDisplayPortVolumeControlEnsureResult {
    HdmiDisplayPortVolumeControlEnsureResult { action }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_route_needs_volume_control_for_hdmi() {
        let devices = vec![device("hdmi", TransportKind::Hdmi)];
        assert!(route_needs_hdmi_displayport_volume_control(
            &devices, "hdmi"
        ));
    }

    #[test]
    fn test_route_needs_volume_control_for_displayport() {
        let devices = vec![device("dp", TransportKind::DisplayPort)];
        assert!(route_needs_hdmi_displayport_volume_control(&devices, "dp"));
    }

    #[test]
    fn test_route_needs_volume_control_not_for_usb() {
        let devices = vec![device("usb", TransportKind::Usb)];
        assert!(!route_needs_hdmi_displayport_volume_control(
            &devices, "usb"
        ));
    }

    #[test]
    fn test_connected_output_present_requires_alive_output() {
        let mut hdmi = device("hdmi", TransportKind::Hdmi);
        hdmi.is_alive = false;
        let devices = vec![device("builtin", TransportKind::BuiltIn), hdmi];
        assert!(!connected_hdmi_displayport_output_present(&devices));
    }

    #[test]
    fn test_driver_offer_without_eqmac_does_not_mention_eqmac() {
        let message = driver_offer_message(false);
        assert!(!message.contains("eqMac"));
    }

    #[test]
    fn test_driver_recommendation_reason_for_selected_builtin() {
        assert_eq!(
            driver_recommendation_reason(Some(false), true, false).as_deref(),
            Some("selected output is not HDMI/DisplayPort")
        );
    }

    #[test]
    fn test_driver_recommendation_reason_for_selected_hdmi() {
        assert_eq!(
            driver_recommendation_reason(Some(true), true, false).as_deref(),
            Some("selected output is HDMI/DisplayPort")
        );
    }

    #[test]
    fn test_native_driver_scope_note_for_user_install() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{home}/Library/Audio/Plug-Ins/HAL/RustyJack.driver");
        assert_eq!(native_driver_scope(&path), "user");
        assert_eq!(
            native_driver_scope_note(&path),
            "user-scope HAL driver; no sudo required"
        );
    }

    #[test]
    fn test_native_driver_warning_for_null_output_stage() {
        assert!(native_driver_warning(Some("virtual-output-null"))
            .unwrap()
            .contains("null output"));
    }

    #[test]
    fn test_native_driver_warning_for_passthrough_skeleton_stage() {
        assert!(
            native_driver_warning(Some(crate::passthrough::PASSTHROUGH_SKELETON_DRIVER_STAGE))
                .unwrap()
                .contains("passthrough skeleton")
        );
    }

    #[test]
    fn test_native_driver_virtual_output_warning_when_bundle_without_device() {
        let devices = vec![device("hdmi", TransportKind::Hdmi)];
        let driver = HalDriverInfo {
            name: "Rusty Jack".into(),
            bundle_id: RUSTY_JACK_DRIVER_BUNDLE_ID.into(),
            version: Some("0.1.1".into()),
            stage: Some(crate::passthrough::PASSTHROUGH_ACTIVE_DRIVER_STAGE.into()),
            install_path: "/Library/Audio/Plug-Ins/HAL/RustyJack.driver".into(),
        };
        let warning = native_driver_virtual_output_warning(&devices, Some(&driver)).unwrap();
        assert!(warning.contains("virtual output"));
        assert!(warning.contains("coreaudiod"));
    }
}
