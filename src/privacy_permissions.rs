//! Re-check macOS privacy permissions on install/upgrade (issue #216).
//!
//! - **Accessibility** — required when `activity_monitor` is `event_tap` (keyboard/mouse wake).
//! - **Local Network** — required when ScalarWebAPI is enabled (SSDP discovery).

use crate::config::{load_config_optional, resolve_config_path, Config};
#[cfg(target_os = "macos")]
use crate::scalar_webapi_device::discover_scalar_webapi_devices_on_lan;
use crate::RustyJackError;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

/// Short SSDP window used only as a Local Network permission heuristic.
const LOCAL_NETWORK_PROBE_TIMEOUT_MS: u64 = 1_500;

/// Privacy permission snapshot for install/upgrade (and `--json` output).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrivacyPermissionStatus {
    pub accessibility_required: bool,
    /// `Some(true/false)` on macOS when Accessibility is required; otherwise `None`.
    pub accessibility_trusted: Option<bool>,
    pub force_daemon_restart: bool,
    pub local_network_ok: Option<bool>,
    pub local_network_required: bool,
    pub notes: Vec<String>,
}

/// Load optional config and ensure required privacy permissions for install/upgrade.
pub fn ensure_privacy_permissions_for_setup(
    interactive: bool,
    config_cli_path: Option<&Path>,
) -> Result<PrivacyPermissionStatus, RustyJackError> {
    let path = resolve_config_path(config_cli_path);
    let config = match path.as_deref() {
        Some(path) => load_config_optional(path, false)?,
        None => None,
    };
    match config.as_ref() {
        Some(config) => ensure_privacy_permissions(config, interactive),
        None => Ok(PrivacyPermissionStatus {
            accessibility_required: false,
            accessibility_trusted: None,
            force_daemon_restart: false,
            local_network_ok: None,
            local_network_required: false,
            notes: vec!["no config loaded; skipped privacy permission checks".into()],
        }),
    }
}

/// Check (and interactively prompt for) privacy permissions implied by `config`.
pub fn ensure_privacy_permissions(
    config: &Config,
    interactive: bool,
) -> Result<PrivacyPermissionStatus, RustyJackError> {
    let mut status = check_privacy_permissions(config)?;
    if !(status.accessibility_required || status.local_network_required) {
        return Ok(status);
    }

    // New binary / LaunchAgent must restart so TCC grants attach to the running daemon.
    status.force_daemon_restart = true;

    if interactive {
        prompt_missing_permissions(&mut status)?;
    } else {
        push_noninteractive_notes(&mut status);
    }

    Ok(status)
}

/// Probe required permissions without prompting.
pub fn check_privacy_permissions(
    config: &Config,
) -> Result<PrivacyPermissionStatus, RustyJackError> {
    let accessibility_required = config.activity_monitor.eq_ignore_ascii_case("event_tap");
    let local_network_required = config
        .scalar_webapi_device
        .as_ref()
        .is_some_and(|api| api.enabled);

    let accessibility_trusted = if accessibility_required {
        Some(accessibility_is_trusted())
    } else {
        None
    };

    let local_network_ok = if local_network_required {
        Some(probe_local_network_permission()?)
    } else {
        None
    };

    let mut notes = Vec::new();
    if accessibility_required {
        match accessibility_trusted {
            Some(true) => notes.push("Accessibility permission looks granted".into()),
            Some(false) => notes.push(
                "Accessibility permission missing (required for activity_monitor=event_tap)".into(),
            ),
            None => {}
        }
    }
    if local_network_required {
        match local_network_ok {
            Some(true) => notes.push("Local Network SSDP probe found at least one device".into()),
            Some(false) => notes.push(
                "Local Network may be missing or no ScalarWebAPI speakers answered SSDP".into(),
            ),
            None => {}
        }
    }

    Ok(PrivacyPermissionStatus {
        accessibility_required,
        accessibility_trusted,
        force_daemon_restart: false,
        local_network_ok,
        local_network_required,
        notes,
    })
}

/// Print a short human summary after install/upgrade.
pub fn print_privacy_permission_status(status: &PrivacyPermissionStatus) {
    if !(status.accessibility_required || status.local_network_required) {
        return;
    }
    println!();
    println!("Privacy permissions");
    if status.accessibility_required {
        let state = match status.accessibility_trusted {
            Some(true) => "granted",
            Some(false) => "missing",
            None => "unchecked",
        };
        println!("  Accessibility: {state}");
    }
    if status.local_network_required {
        let state = match status.local_network_ok {
            Some(true) => "ok (SSDP probe heard a device)",
            Some(false) => "check System Settings (SSDP heard nothing)",
            None => "unchecked",
        };
        println!("  Local Network: {state}");
    }
    for note in &status.notes {
        println!("  note: {note}");
    }
    if status.force_daemon_restart {
        println!("  daemon: force-restarted so permission changes apply");
    }
}

fn accessibility_is_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::ax_is_process_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn probe_local_network_permission() -> Result<bool, RustyJackError> {
    #[cfg(target_os = "macos")]
    {
        match discover_scalar_webapi_devices_on_lan(LOCAL_NETWORK_PROBE_TIMEOUT_MS) {
            Ok(devices) => Ok(!devices.is_empty()),
            Err(err) => {
                tracing::warn!(
                    target: "setup",
                    "[privacy] Local Network SSDP probe failed: {}",
                    err.detail_message()
                );
                Ok(false)
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(false)
    }
}

fn prompt_missing_permissions(status: &mut PrivacyPermissionStatus) -> Result<(), RustyJackError> {
    use dialoguer::console::style;
    use dialoguer::Confirm;

    if status.accessibility_required && status.accessibility_trusted == Some(false) {
        eprintln!(
            "{}",
            style(
                "Accessibility permission is required for activity_monitor=event_tap \
                 (keyboard/mouse ScalarWebAPI wake)."
            )
            .yellow()
        );
        eprintln!(
            "Grant Access for rusty-jack in System Settings → Privacy & Security → Accessibility, \
             then restart the daemon."
        );
        if Confirm::new()
            .with_prompt("Open Accessibility settings now?")
            .default(true)
            .interact()
            .map_err(|err| RustyJackError::Config(format!("Accessibility prompt failed: {err}")))?
        {
            open_privacy_pane(PrivacyPane::Accessibility)?;
        }
        let _ = Confirm::new()
            .with_prompt("Press Enter after granting Accessibility (or skip if already done)")
            .default(true)
            .interact()
            .map_err(|err| {
                RustyJackError::Config(format!("Accessibility confirmation failed: {err}"))
            })?;
        let trusted = accessibility_is_trusted();
        status.accessibility_trusted = Some(trusted);
        if trusted {
            status
                .notes
                .push("Accessibility permission confirmed after prompt".into());
        } else {
            status.notes.push(
                "Accessibility still missing; daemon will start but event-tap wake may fall back"
                    .into(),
            );
        }
    }

    if status.local_network_required && status.local_network_ok == Some(false) {
        eprintln!(
            "{}",
            style("Local Network permission may be missing (SSDP found 0 ScalarWebAPI speakers).")
                .yellow()
        );
        eprintln!(
            "If a speaker is on the LAN, grant Local Network for rusty-jack in \
             System Settings → Privacy & Security → Local Network, then restart the daemon."
        );
        if Confirm::new()
            .with_prompt("Open Local Network settings now?")
            .default(true)
            .interact()
            .map_err(|err| RustyJackError::Config(format!("Local Network prompt failed: {err}")))?
        {
            open_privacy_pane(PrivacyPane::LocalNetwork)?;
        }
        let _ = Confirm::new()
            .with_prompt("Press Enter after granting Local Network (or skip if already done)")
            .default(true)
            .interact()
            .map_err(|err| {
                RustyJackError::Config(format!("Local Network confirmation failed: {err}"))
            })?;
        let ok = probe_local_network_permission()?;
        status.local_network_ok = Some(ok);
        if ok {
            status
                .notes
                .push("Local Network SSDP probe succeeded after prompt".into());
        } else {
            status.notes.push(
                "SSDP still empty after prompt; wake can use cache/config port if set correctly"
                    .into(),
            );
        }
    }

    Ok(())
}

fn push_noninteractive_notes(status: &mut PrivacyPermissionStatus) {
    if status.accessibility_required && status.accessibility_trusted == Some(false) {
        status
            .notes
            .push("grant Accessibility to rusty-jack, then run: rusty-jack upgrade --force".into());
    }
    if status.local_network_required && status.local_network_ok == Some(false) {
        status.notes.push(
            "grant Local Network to rusty-jack if SSDP is blocked, then: rusty-jack upgrade --force"
                .into(),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivacyPane {
    Accessibility,
    LocalNetwork,
}

fn open_privacy_pane(pane: PrivacyPane) -> Result<(), RustyJackError> {
    let urls: &[&str] = match pane {
        PrivacyPane::Accessibility => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility",
        ],
        PrivacyPane::LocalNetwork => &[
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_LocalNetwork",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork",
        ],
    };
    for url in urls {
        let status = Command::new("open")
            .arg(url)
            .status()
            .map_err(RustyJackError::Io)?;
        if status.success() {
            return Ok(());
        }
    }
    Err(RustyJackError::Config(format!(
        "could not open System Settings for {pane:?}"
    )))
}

#[cfg(target_os = "macos")]
mod macos {
    #![allow(unsafe_code)]

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    pub(super) fn ax_is_process_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeviceSelectorConfig, ScalarWebApiDeviceConfig};

    #[test]
    fn test_check_privacy_permissions_idle_without_scalar_skips() {
        let status = check_privacy_permissions(&Config::default()).unwrap();
        assert!(!status.accessibility_required);
        assert!(!status.local_network_required);
        assert!(!status.force_daemon_restart);
        assert!(status.accessibility_trusted.is_none());
        assert!(status.local_network_ok.is_none());
    }

    #[test]
    fn test_check_privacy_permissions_requires_accessibility_for_event_tap() {
        let config = Config {
            activity_monitor: "event_tap".into(),
            ..Config::default()
        };
        let status = check_privacy_permissions(&config).unwrap();
        assert!(status.accessibility_required);
        assert!(status.accessibility_trusted.is_some());
        assert!(!status.local_network_required);
    }

    #[test]
    fn test_check_privacy_permissions_requires_local_network_for_scalar() {
        let config = Config {
            scalar_webapi_device: Some(ScalarWebApiDeviceConfig {
                enabled: true,
                model: "SRS-ZR5".into(),
                host: Some("192.168.86.18".into()),
                port: 54_480,
                path: "sony/av".into(),
                mac_output: DeviceSelectorConfig {
                    name: None,
                    uid: Some("uid".into()),
                },
                triggers: vec!["output_selected".into()],
                wake_debounce_ms: 5_000,
                request_timeout_ms: 3_000,
                require_quick_start: true,
                speaker_input: None,
            }),
            ..Config::default()
        };
        let status = check_privacy_permissions(&config).unwrap();
        assert!(status.local_network_required);
        assert!(status.local_network_ok.is_some());
        assert!(!status.accessibility_required);
    }

    #[test]
    fn test_ensure_privacy_permissions_forces_restart_when_features_configured() {
        let config = Config {
            activity_monitor: "event_tap".into(),
            ..Config::default()
        };
        let status = ensure_privacy_permissions(&config, false).unwrap();
        assert!(status.force_daemon_restart);
        assert!(status
            .notes
            .iter()
            .any(|note| note.contains("upgrade --force") || note.contains("Accessibility")));
    }
}
