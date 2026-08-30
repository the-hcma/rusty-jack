//! Re-check macOS privacy permissions on install/upgrade (issue #216).
//!
//! - **Accessibility** — required when `activity_monitor` is `event_tap` (keyboard/mouse wake).
//! - **Local Network** — required when ScalarWebAPI is enabled (SSDP discovery).

use crate::config::{load_config_readonly, resolve_config_path, Config};
#[cfg(target_os = "macos")]
use crate::network::lan_connectivity_ready;
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
    /// `Some(true)` when SSDP heard a device; `None` when unchecked/inconclusive
    /// (no LAN route, empty SSDP, probe error, or skipped). The SSDP heuristic
    /// never returns `Some(false)` — empty/error is inconclusive, not denial.
    pub local_network_ok: Option<bool>,
    pub local_network_required: bool,
    pub notes: Vec<String>,
}

/// Load optional config and ensure required privacy permissions for install/upgrade.
///
/// Config load/validation failures are tolerated: permission checks are skipped so
/// `upgrade` can still refresh the LaunchAgent (matching pre-#216 behavior).
/// Reads config without rewriting the file (preserves hand-edits and symlinks).
pub fn ensure_privacy_permissions_for_setup(
    interactive: bool,
    config_cli_path: Option<&Path>,
) -> Result<PrivacyPermissionStatus, RustyJackError> {
    let path = resolve_config_path(config_cli_path);
    let config = match path.as_deref() {
        Some(path) => match load_config_readonly(path) {
            Ok(config) => Some(config),
            Err(RustyJackError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                tracing::warn!(
                    target: "setup",
                    "[privacy] skipping permission checks; could not load config {}: {}",
                    path.display(),
                    err.detail_message()
                );
                return Ok(PrivacyPermissionStatus {
                    accessibility_required: false,
                    accessibility_trusted: None,
                    force_daemon_restart: false,
                    local_network_ok: None,
                    local_network_required: false,
                    notes: vec![format!(
                        "skipped privacy checks (config load failed: {})",
                        err.detail_message()
                    )],
                });
            }
        },
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
    ensure_privacy_permissions_with(config, interactive, probe_local_network_permission)
}

fn ensure_privacy_permissions_with(
    config: &Config,
    interactive: bool,
    local_network_probe: impl Fn() -> Option<bool>,
) -> Result<PrivacyPermissionStatus, RustyJackError> {
    let mut status = check_privacy_permissions_with(config, &local_network_probe)?;
    if !(status.accessibility_required || status.local_network_required) {
        return Ok(status);
    }

    // New binary / LaunchAgent must restart so TCC grants attach to the running daemon.
    status.force_daemon_restart = true;

    if interactive {
        prompt_missing_permissions(&mut status, &local_network_probe)?;
    } else {
        push_noninteractive_notes(&mut status);
    }

    Ok(status)
}

/// Probe required permissions without prompting.
pub fn check_privacy_permissions(
    config: &Config,
) -> Result<PrivacyPermissionStatus, RustyJackError> {
    check_privacy_permissions_with(config, probe_local_network_permission)
}

fn check_privacy_permissions_with(
    config: &Config,
    local_network_probe: impl Fn() -> Option<bool>,
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
        local_network_probe()
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
            // Probe never emits Some(false); treat any other value as inconclusive.
            _ => notes.push(
                "Local Network not confirmed (no LAN route or SSDP inconclusive; not treating as denied)"
                    .into(),
            ),
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
            // Probe never emits Some(false); empty/error stays inconclusive.
            _ => "unchecked / inconclusive",
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

/// Map SSDP discovery outcome to Local Network status.
///
/// `Ok(count > 0)` → `Some(true)`; empty or `Err` → `None` (never `Some(false)`).
fn local_network_status_from_discovery(device_count: Result<usize, ()>) -> Option<bool> {
    match device_count {
        Ok(count) if count > 0 => Some(true),
        Ok(_) | Err(()) => None,
    }
}

/// Probe Local Network via a short SSDP scan.
///
/// Returns `Some(true)` only when at least one speaker answers. Empty SSDP or no
/// LAN route is `None` (inconclusive) — never treated as a confirmed denial.
fn probe_local_network_permission() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        if !lan_connectivity_ready() {
            tracing::info!(
                target: "setup",
                "[privacy] skipping Local Network SSDP probe; LAN connectivity not ready"
            );
            return None;
        }
        match discover_scalar_webapi_devices_on_lan(LOCAL_NETWORK_PROBE_TIMEOUT_MS) {
            Ok(devices) => local_network_status_from_discovery(Ok(devices.len())),
            Err(err) => {
                tracing::warn!(
                    target: "setup",
                    "[privacy] Local Network SSDP probe failed: {}",
                    err.detail_message()
                );
                local_network_status_from_discovery(Err(()))
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn prompt_missing_permissions(
    status: &mut PrivacyPermissionStatus,
    local_network_probe: &impl Fn() -> Option<bool>,
) -> Result<(), RustyJackError> {
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
            open_privacy_pane(PrivacyPane::Accessibility);
        }
        let _ = Confirm::new()
            .with_prompt("Press Enter after granting Accessibility (or skip if already done)")
            .default(true)
            .interact()
            .map_err(|err| {
                RustyJackError::Config(format!("Accessibility confirmation failed: {err}"))
            })?;
        // TCC Accessibility grants bind at process start; this CLI process cannot
        // observe a fresh toggle via AXIsProcessTrusted. Leave the pre-prompt
        // status and note that the force-restarted daemon will pick up the grant.
        status.notes.push(
            "Accessibility grant (if toggled) applies after daemon restart; not re-checked in this process"
                .into(),
        );
    }

    // Local Network: only advisory when inconclusive. Do not force System Settings
    // for empty SSDP (cannot distinguish permission denial from offline speaker).
    if status.local_network_required && status.local_network_ok.is_none() {
        eprintln!(
            "{}",
            style("Local Network could not be confirmed (no LAN route or SSDP heard no speakers).")
                .dim()
        );
        eprintln!(
            "If discovery stays empty while a speaker is on the LAN, grant Local Network for \
             rusty-jack in System Settings → Privacy & Security → Local Network."
        );
        if Confirm::new()
            .with_prompt("Open Local Network settings now? (optional)")
            .default(false)
            .interact()
            .map_err(|err| RustyJackError::Config(format!("Local Network prompt failed: {err}")))?
        {
            open_privacy_pane(PrivacyPane::LocalNetwork);
            let _ = Confirm::new()
                .with_prompt("Press Enter after granting Local Network (or skip if already done)")
                .default(true)
                .interact()
                .map_err(|err| {
                    RustyJackError::Config(format!("Local Network confirmation failed: {err}"))
                })?;
            status.local_network_ok = local_network_probe();
            if status.local_network_ok == Some(true) {
                status
                    .notes
                    .push("Local Network SSDP probe succeeded after grant".into());
            }
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
    if status.local_network_required && status.local_network_ok.is_none() {
        status.notes.push(
            "if SSDP discovery stays empty, grant Local Network then: rusty-jack upgrade --force"
                .into(),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivacyPane {
    Accessibility,
    LocalNetwork,
}

/// Best-effort System Settings open; failures are non-fatal (note only).
fn open_privacy_pane(pane: PrivacyPane) {
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
        match Command::new("open").arg(url).status() {
            Ok(status) if status.success() => return,
            Ok(_) | Err(_) => continue,
        }
    }
    eprintln!(
        "note: could not open System Settings for {pane:?}; open Privacy & Security manually"
    );
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
    use std::io::Write;

    fn scalar_enabled_config() -> Config {
        Config {
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
        }
    }

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
    fn test_check_privacy_permissions_local_network_ok_when_probe_hears_device() {
        let status =
            check_privacy_permissions_with(&scalar_enabled_config(), || Some(true)).unwrap();
        assert!(status.local_network_required);
        assert_eq!(status.local_network_ok, Some(true));
        assert!(status
            .notes
            .iter()
            .any(|note| note.contains("found at least one device")));
    }

    #[test]
    fn test_check_privacy_permissions_local_network_inconclusive_when_probe_empty() {
        let status = check_privacy_permissions_with(&scalar_enabled_config(), || None).unwrap();
        assert!(status.local_network_required);
        assert!(status.local_network_ok.is_none());
        assert!(status
            .notes
            .iter()
            .any(|note| note.contains("inconclusive") || note.contains("not treating as denied")));
    }

    #[test]
    fn test_ensure_privacy_permissions_forces_restart_for_event_tap() {
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

    #[test]
    fn test_ensure_privacy_permissions_forces_restart_for_scalar_webapi() {
        let status =
            ensure_privacy_permissions_with(&scalar_enabled_config(), false, || None).unwrap();
        assert!(status.local_network_required);
        assert!(status.force_daemon_restart);
    }

    #[test]
    fn test_ensure_privacy_permissions_for_setup_tolerates_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{{ \"version\": 1 }}").unwrap();
        drop(file);

        let status = ensure_privacy_permissions_for_setup(false, Some(&path)).unwrap();
        assert!(!status.force_daemon_restart);
        assert!(status
            .notes
            .iter()
            .any(|note| note.contains("config load failed")));
    }

    #[test]
    fn test_load_config_readonly_does_not_rewrite_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // Intentionally unsorted keys so rewrite_config_if_needed would change it.
        let original = r#"{
  "version": 1,
  "preferred_device": { "uid": "uid-1" },
  "auto_switch": true
}
"#;
        std::fs::write(&path, original).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let _ = load_config_readonly(&path).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn test_local_network_status_from_discovery_maps_ssdp_outcomes() {
        assert_eq!(local_network_status_from_discovery(Ok(1)), Some(true));
        assert_eq!(local_network_status_from_discovery(Ok(3)), Some(true));
        assert_eq!(local_network_status_from_discovery(Ok(0)), None);
        assert_eq!(local_network_status_from_discovery(Err(())), None);
    }
}
