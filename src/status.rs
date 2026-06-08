//! Build and format `rusty-jack status` output.

use crate::config::Config;
use crate::hdmi_displayport_volume_control::{
    hdmi_displayport_volume_control_status_for_target, HdmiDisplayPortVolumeControlStatus,
};
use crate::launchd::{DaemonLogPaths, DaemonStatus};
use crate::list_fmt::{self, format_labeled_section};
use crate::policy::evaluate_policy;
use crate::scalar_webapi_device::{self, ScalarWebApiMacOutputLink};
use crate::state::ActivitySnapshot;
use crate::system_default::DeviceList;
use anyhow::Result;
use chrono::{Local, TimeZone};
use serde::Serialize;
use std::io::{self, Write};
use std::path::Path;

/// Policy evaluation for the current routing state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyStatus {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_device_name: Option<String>,
    /// Human-readable label for the resolved preferred device (when connected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_device_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_device_uid: Option<String>,
    /// Human-readable label for the active routed device (when known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_device_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_device_uid: Option<String>,
    pub matches_preferred: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_alive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_switch: Option<bool>,
    /// Volume (0–100) from config, restored on route switches and daemon startup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_volume: Option<u8>,
    pub message: String,
}

/// Snapshot returned by `rusty-jack status`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusSnapshot {
    pub devices: Vec<crate::output_device::OutputDevice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_default: Option<crate::system_default::SystemDefaultInfo>,
    pub policy: PolicyStatus,
    /// Current effective output volume (0–100) for the active route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_percent: Option<u8>,
    pub hdmi_displayport_volume_control: HdmiDisplayPortVolumeControlStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scalar_webapi: Option<ScalarWebApiStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scalar_webapi_mac_output: Option<ScalarWebApiMacOutputLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon: Option<DaemonStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_logs: Option<DaemonLogPaths>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<ActivitySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScalarWebApiStatus {
    pub enabled: bool,
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub mac_output_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_output_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_status: Option<String>,
}

/// Build a status snapshot from a device list and optional config.
#[must_use]
pub fn build_status(
    list: DeviceList,
    config: Option<&Config>,
    config_path: Option<&Path>,
    volume_percent: Option<u8>,
    daemon: Option<DaemonStatus>,
    daemon_logs: Option<DaemonLogPaths>,
    activity: Option<ActivitySnapshot>,
) -> StatusSnapshot {
    let policy = evaluate_policy(
        &DeviceList {
            devices: list.devices.clone(),
            system_default: list.system_default.clone(),
            scalar_webapi_mac_output: list.scalar_webapi_mac_output.clone(),
        },
        config,
        config_path,
    );

    let selected_uid = policy
        .preferred_device_uid
        .as_deref()
        .or(policy.active_device_uid.as_deref());
    let hdmi_displayport_volume_control =
        hdmi_displayport_volume_control_status_for_target(&list.devices, selected_uid);
    let scalar_webapi_mac_output = config.and_then(|config| {
        scalar_webapi_device::scalar_webapi_mac_output_link(config, &list.devices)
    });
    let scalar_webapi = build_scalar_webapi_status(config, scalar_webapi_mac_output.as_ref());

    StatusSnapshot {
        devices: list.devices,
        system_default: list.system_default,
        policy,
        volume_percent,
        hdmi_displayport_volume_control,
        scalar_webapi,
        scalar_webapi_mac_output,
        daemon,
        daemon_logs,
        activity,
    }
}

fn build_scalar_webapi_status(
    config: Option<&Config>,
    link: Option<&ScalarWebApiMacOutputLink>,
) -> Option<ScalarWebApiStatus> {
    let api = config
        .and_then(|c| c.scalar_webapi_device.as_ref())
        .filter(|api| api.enabled)?;
    Some(ScalarWebApiStatus {
        enabled: api.enabled,
        host: api.host.clone(),
        model: Some(api.model.clone()),
        mac_output_uid: link
            .map(|link| link.mac_output_uid.clone())
            .or_else(|| api.mac_output.uid.clone()),
        mac_output_label: link.and_then(|link| link.mac_output_label.clone()),
        power_status: scalar_webapi_device::current_power_status(api).ok(),
    })
}

fn format_scalar_webapi_block(status: &ScalarWebApiStatus) -> String {
    let mac_output = match (
        status.mac_output_label.as_deref(),
        status.mac_output_uid.as_deref(),
    ) {
        (Some(label), Some(uid)) => format!("{label} ({uid})"),
        (Some(label), None) => label.to_string(),
        (None, Some(uid)) => uid.to_string(),
        (None, None) => "(unset)".into(),
    };

    let mut rows: Vec<(&str, String)> = vec![
        (
            "enabled",
            if status.enabled {
                "yes".into()
            } else {
                "no".into()
            },
        ),
        (
            "host",
            status.host.clone().unwrap_or_else(|| "(unset)".into()),
        ),
    ];

    if let Some(model) = &status.model {
        rows.push(("model", model.clone()));
    }

    rows.extend([
        ("mac output", mac_output),
        (
            "power",
            status
                .power_status
                .clone()
                .unwrap_or_else(|| "unknown".into()),
        ),
    ]);
    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format_labeled_section("ScalarWebAPI", "  ", &borrowed)
}

fn format_hdmi_displayport_volume_control_block(
    status: &HdmiDisplayPortVolumeControlStatus,
) -> String {
    let mut rows: Vec<(&str, String)> = vec![
        (
            "connected output",
            if status.connected_output_present {
                "detected".into()
            } else {
                "not detected".into()
            },
        ),
        (
            "native driver",
            if let Some(driver) = &status.native_driver {
                format!("installed ({})", driver.name)
            } else {
                "not installed".into()
            },
        ),
        ("driver recommended", format_driver_recommended(status)),
        (
            "eqMac fallback",
            if let Some(path) = &status.eqmac_app_path {
                format!("installed at {path}")
            } else {
                "not installed".into()
            },
        ),
    ];
    if let Some(scope) = &status.native_driver_scope {
        rows.push(("driver scope", scope.clone()));
    }
    if let Some(path) = &status.native_driver_install_path {
        rows.push(("driver path", path.clone()));
    }
    if let Some(driver) = &status.native_driver {
        if let Some(version) = &driver.version {
            rows.push(("driver version", version.clone()));
        }
    }
    if let Some(stage) = &status.native_driver_stage {
        rows.push(("driver stage", stage.clone()));
    }
    if let Some(warning) = &status.native_driver_warning {
        rows.push(("driver note", warning.clone()));
    }
    if let Some(path) = &status.orphaned_eqmac_hal_driver_path {
        rows.push(("eqMac driver", format!("orphaned at {path}")));
        rows.push((
            "cleanup",
            format!("remove stale eqMac driver with `sudo rm -rf {path}`"),
        ));
    } else if let Some(path) = &status.eqmac_hal_driver_path {
        rows.push(("eqMac driver", format!("installed at {path}")));
    }
    if let Some(backup) = &status.eqmac_driver_backup {
        rows.push(("eqMac backup", backup.backup_path.clone()));
        rows.push(("swap restore", "rusty-jack driver swap-out".into()));
    }
    if let Some(recommendation) = &status.recommendation {
        rows.push(("note", recommendation.clone()));
    }

    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format_labeled_section("HDMI/DisplayPort Volume Control", "  ", &borrowed)
}

fn format_driver_recommended(status: &HdmiDisplayPortVolumeControlStatus) -> String {
    let value = if status.native_driver_recommended {
        "yes"
    } else {
        "no"
    };
    status
        .native_driver_recommendation_reason
        .as_ref()
        .map_or_else(|| value.into(), |reason| format!("{value} ({reason})"))
}

fn format_daemon_block(daemon: &DaemonStatus, daemon_logs: Option<&DaemonLogPaths>) -> String {
    let mut rows: Vec<(&str, String)> = vec![];
    match daemon {
        DaemonStatus::Running {
            label,
            plist_path,
            service,
            pid,
        } => {
            rows.push(("state", "running".into()));
            rows.push(("installed", "yes".into()));
            rows.push(("running", "yes".into()));
            rows.push(("paused", "no".into()));
            rows.push(("label", label.clone()));
            rows.push(("service", service.clone()));
            if let Some(pid) = pid {
                rows.push(("pid", pid.to_string()));
            }
            rows.push(("plist", plist_path.clone()));
        }
        DaemonStatus::Paused {
            label,
            plist_path,
            service,
            pause_reason,
        } => {
            rows.push(("state", "paused".into()));
            rows.push(("installed", "yes".into()));
            rows.push(("running", "no".into()));
            rows.push(("paused", "yes".into()));
            if let Some(reason) = pause_reason {
                rows.push(("reason", reason.label().into()));
                rows.push(("note", reason.message()));
            }
            rows.push(("label", label.clone()));
            rows.push(("service", service.clone()));
            rows.push(("plist", plist_path.clone()));
        }
        DaemonStatus::NotInstalled { plist_path } => {
            rows.push(("state", "not installed".into()));
            rows.push(("installed", "no".into()));
            rows.push(("running", "no".into()));
            rows.push(("paused", "no".into()));
            rows.push(("expected plist", plist_path.clone()));
        }
        DaemonStatus::Unknown {
            label,
            plist_path,
            message,
        } => {
            rows.push(("state", "unknown".into()));
            rows.push(("installed", "unknown".into()));
            rows.push(("running", "unknown".into()));
            rows.push(("paused", "unknown".into()));
            rows.push(("label", label.clone()));
            rows.push(("plist", plist_path.clone()));
            rows.push(("note", message.clone()));
        }
    }

    if let Some(logs) = daemon_logs {
        rows.push(("log", logs.file.clone()));
    }

    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format_labeled_section("Daemon", "  ", &borrowed)
}

fn format_daemon_logs_block(logs: &DaemonLogPaths) -> String {
    format_labeled_section("Daemon", "  ", &[("log", logs.file.as_str())])
}

fn format_activity_block(snapshot: &ActivitySnapshot) -> String {
    let mut rows: Vec<(&str, String)> = vec![
        (
            "last sample",
            format_unix_local(snapshot.sampled_at_unix_seconds),
        ),
        (
            "idle",
            format!(
                "{:.1}s (threshold {:.1}s)",
                snapshot.idle_seconds, snapshot.threshold_seconds
            ),
        ),
        (
            "state",
            if snapshot.is_idle {
                "idle".into()
            } else {
                "active".into()
            },
        ),
    ];

    if let Some(at) = snapshot.last_became_active_at_unix_seconds {
        let user = snapshot
            .last_became_active_console_user
            .as_deref()
            .unwrap_or("(unknown)");
        rows.push((
            "last became_active",
            format!("{} (console user {user})", format_unix_local(at)),
        ));
    } else {
        rows.push(("last became_active", "(none recorded)".into()));
    }

    rows.push((
        "console user",
        snapshot
            .console_user
            .clone()
            .unwrap_or_else(|| "(unknown)".into()),
    ));
    rows.push(("daemon user", snapshot.daemon_user.clone()));
    rows.push((
        "wake triggers",
        if snapshot.triggers.is_empty() {
            "(none)".into()
        } else {
            snapshot.triggers.join(", ")
        },
    ));

    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format_labeled_section("Activity", "  ", &borrowed)
}

fn format_unix_local(unix: u64) -> String {
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|dt| format!("{} local", dt.format("%Y-%m-%d %H:%M:%S")))
        .unwrap_or_else(|| "(unknown)".into())
}

fn format_policy_block(policy: &PolicyStatus, volume_percent: Option<u8>) -> String {
    let mut rows: Vec<(&str, String)> = vec![(
        "configured",
        if policy.configured {
            "yes".into()
        } else {
            "no".into()
        },
    )];

    if let Some(path) = &policy.config_path {
        rows.push(("config", path.clone()));
    }

    if let Some(name) = &policy.preferred_device_name {
        rows.push(("device", name.clone()));
    }

    if let Some(uid) = &policy.preferred_device_uid {
        let label = policy
            .preferred_device_label
            .as_deref()
            .or(policy.preferred_device_name.as_deref())
            .unwrap_or("(unknown)");
        rows.push(("preferred", format!("{label} ({uid})")));
    }

    if let Some(uid) = &policy.active_device_uid {
        let label = policy.active_device_label.as_deref().unwrap_or("(unknown)");
        rows.push(("active", format!("{label} ({uid})")));
    }

    if let Some(matches) = policy.matches_preferred {
        rows.push(("matches", if matches { "yes".into() } else { "no".into() }));
    }

    if let Some(auto) = policy.auto_switch {
        rows.push(("auto_switch", if auto { "yes".into() } else { "no".into() }));
    }

    if let Some(volume) = policy.config_volume {
        rows.push(("config volume", format!("{volume}%")));
    }

    if let Some(volume) = volume_percent {
        rows.push(("volume", format!("{volume}%")));
    }

    rows.push(("note", policy.message.clone()));

    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format_labeled_section("Policy", "  ", &borrowed)
}

/// Print human-readable status: device table, virtual default block, policy.
pub fn print_text(snapshot: &StatusSnapshot) -> Result<()> {
    let list = DeviceList {
        devices: snapshot.devices.clone(),
        system_default: snapshot.system_default.clone(),
        scalar_webapi_mac_output: snapshot.scalar_webapi_mac_output.clone(),
    };

    list_fmt::print_device_table(&list)?;
    list_fmt::print_virtual_default_footer(&list)?;
    let mut out = io::stdout().lock();
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        format_policy_block(&snapshot.policy, snapshot.volume_percent)
    )?;
    if snapshot
        .hdmi_displayport_volume_control
        .connected_output_present
        || snapshot
            .hdmi_displayport_volume_control
            .native_driver_installed
        || snapshot.hdmi_displayport_volume_control.eqmac_installed
        || snapshot
            .hdmi_displayport_volume_control
            .eqmac_driver_backup
            .is_some()
    {
        writeln!(out)?;
        writeln!(
            out,
            "{}",
            format_hdmi_displayport_volume_control_block(&snapshot.hdmi_displayport_volume_control)
        )?;
    }
    if let Some(daemon) = &snapshot.daemon {
        writeln!(out)?;
        writeln!(
            out,
            "{}",
            format_daemon_block(daemon, snapshot.daemon_logs.as_ref())
        )?;
    } else if let Some(logs) = &snapshot.daemon_logs {
        writeln!(out)?;
        writeln!(out, "{}", format_daemon_logs_block(logs))?;
    }
    if let Some(scalar) = &snapshot.scalar_webapi {
        writeln!(out)?;
        writeln!(out, "{}", format_scalar_webapi_block(scalar))?;
    }
    if let Some(activity) = &snapshot.activity {
        writeln!(out)?;
        writeln!(out, "{}", format_activity_block(activity))?;
    }
    Ok(())
}

/// Print JSON status to stdout.
pub fn print_json(snapshot: &StatusSnapshot) -> Result<()> {
    let value = serde_json::to_string_pretty(snapshot)?;
    println!("{value}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig};
    use crate::eqmac::EqMacDriverBackupInfo;
    use crate::output_device::OutputDevice;
    use crate::system_default::{HalDriverInfo, SystemDefaultInfo};
    use crate::transport::TransportKind;

    fn hdmi_device(active: bool) -> OutputDevice {
        OutputDevice {
            id: 2,
            uid: "hdmi-1".into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
        }
    }

    #[test]
    fn test_build_status_without_config() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
                scalar_webapi_mac_output: None,
            },
            None,
            None,
            Some(42),
            None,
            None,
            None,
        );
        assert!(!snapshot.policy.configured);
        assert_eq!(snapshot.volume_percent, Some(42));
        assert_eq!(snapshot.policy.active_device_uid.as_deref(), Some("hdmi-1"));
    }

    #[test]
    fn test_build_status_with_matching_config() {
        let config = Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            preferred_device: DeviceSelectorConfig {
                name: None,
                uid: Some("hdmi-1".into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            scalar_webapi_device: None,
            ..Default::default()
        };
        let mut config = config;
        config.volume = Some(13);
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
                scalar_webapi_mac_output: None,
            },
            Some(&config),
            Some(Path::new("/tmp/config.json")),
            Some(13),
            None,
            None,
            None,
        );
        assert!(snapshot.policy.configured);
        assert_eq!(snapshot.policy.matches_preferred, Some(true));
        assert_eq!(snapshot.policy.config_volume, Some(13));
        assert_eq!(snapshot.volume_percent, Some(13));
    }

    #[test]
    fn test_build_status_preserves_virtual_default() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: Some(SystemDefaultInfo {
                    uid: "EQMOutputCapture".into(),
                    name: "DELL U3219Q (eqMac)".into(),
                    transport: TransportKind::Virtual,
                    manufacturer: Some("Bitgapp Ltd".into()),
                    model_uid: None,
                    router: Some("eqMac".into()),
                    driver: Some(HalDriverInfo {
                        name: "eqMac".into(),
                        bundle_id: "com.bitgapp.eqmac.driver".into(),
                        version: Some("2.6.0".into()),
                        stage: None,
                        install_path: "/Library/Audio/Plug-Ins/HAL/eqMac.driver".into(),
                    }),
                    routed_to_uid: Some("hdmi-1".into()),
                    routed_to_label: Some("HDMI (DELL U3219Q)".into()),
                }),
                scalar_webapi_mac_output: None,
            },
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(snapshot.system_default.is_some());
    }

    #[test]
    fn test_format_policy_block_includes_volume() {
        let block = format_policy_block(
            &PolicyStatus {
                configured: true,
                config_path: Some("/tmp/c.json".into()),
                preferred_device_name: Some("HDMI".into()),
                preferred_device_label: Some("HDMI (DELL U3219Q)".into()),
                preferred_device_uid: Some("hdmi-1".into()),
                active_device_label: Some("HDMI (DELL U3219Q)".into()),
                active_device_uid: Some("hdmi-1".into()),
                matches_preferred: Some(true),
                preferred_present: Some(true),
                preferred_alive: Some(true),
                auto_switch: Some(true),
                config_volume: Some(13),
                message: "ok".into(),
            },
            Some(13),
        );
        assert!(block.contains("matches"));
        assert!(block.contains("config volume"));
        assert!(block.contains("13%"));
        let detail_lines: Vec<&str> = block
            .lines()
            .filter(|line| line.starts_with("  ") && line.contains(": "))
            .collect();
        let value_starts: Vec<usize> = detail_lines
            .iter()
            .map(|line| line.find(": ").unwrap() + 2)
            .collect();
        assert!(value_starts.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn test_format_policy_block_includes_match_line() {
        let block = format_policy_block(
            &PolicyStatus {
                configured: true,
                config_path: Some("/tmp/c.json".into()),
                preferred_device_name: Some("HDMI".into()),
                preferred_device_label: Some("HDMI".into()),
                preferred_device_uid: Some("hdmi-1".into()),
                active_device_label: Some("HDMI".into()),
                active_device_uid: Some("hdmi-1".into()),
                matches_preferred: Some(true),
                preferred_present: Some(true),
                preferred_alive: Some(true),
                auto_switch: Some(true),
                config_volume: None,
                message: "ok".into(),
            },
            None,
        );
        assert!(block.contains("matches"));
        assert!(block.contains("preferred"));
        assert!(block.contains("HDMI (hdmi-1)"));
    }

    #[test]
    fn test_format_scalar_webapi_block_includes_model_and_mac_output_label() {
        let block = format_scalar_webapi_block(&ScalarWebApiStatus {
            enabled: true,
            host: Some("192.168.86.18".into()),
            model: Some("The Lair".into()),
            mac_output_uid: Some("BuiltInHeadphoneOutputDevice".into()),
            mac_output_label: Some("External Headphones".into()),
            power_status: Some("active".into()),
        });
        assert!(block.contains("model"));
        assert!(block.contains("The Lair"));
        assert!(block.contains("External Headphones (BuiltInHeadphoneOutputDevice)"));
    }

    #[test]
    fn test_format_activity_block_shows_poll_snapshot() {
        let block = format_activity_block(&ActivitySnapshot {
            sampled_at_unix_seconds: 1_714_000_000,
            idle_seconds: 12.4,
            threshold_seconds: 60.0,
            is_idle: false,
            console_user: Some("hcma".into()),
            daemon_user: "hcma".into(),
            triggers: vec!["keyboard".into(), "mouse".into()],
            last_became_active_at_unix_seconds: Some(1_713_999_000),
            last_became_active_console_user: Some("hcma".into()),
            last_became_active_daemon_user: Some("hcma".into()),
        });
        assert!(block.contains("Activity"));
        assert!(block.contains("12.4s (threshold 60.0s)"));
        assert!(block.contains("console user"));
        assert!(block.contains("hcma"));
        assert!(block.contains("keyboard, mouse"));
    }

    #[test]
    fn test_format_daemon_block_shows_status_flags() {
        fn has_row(block: &str, label: &str, value: &str) -> bool {
            block.lines().any(|line| {
                line.trim_start()
                    .split_once(':')
                    .is_some_and(|(actual_label, actual_value)| {
                        actual_label.trim() == label && actual_value.trim() == value
                    })
            })
        }

        let logs = DaemonLogPaths {
            file: "/tmp/rusty-jack.log".into(),
        };

        let running = format_daemon_block(
            &DaemonStatus::Running {
                label: crate::launchd::LAUNCH_AGENT_LABEL.into(),
                plist_path: "/tmp/test.plist".into(),
                service: "gui/501/com.example.rusty-jack".into(),
                pid: Some(123),
            },
            Some(&logs),
        );
        assert!(has_row(&running, "installed", "yes"));
        assert!(has_row(&running, "running", "yes"));
        assert!(has_row(&running, "paused", "no"));
        assert!(has_row(&running, "log", "/tmp/rusty-jack.log"));

        let paused = format_daemon_block(
            &DaemonStatus::Paused {
                label: crate::launchd::LAUNCH_AGENT_LABEL.into(),
                plist_path: "/tmp/test.plist".into(),
                service: "gui/501/com.example.rusty-jack".into(),
                pause_reason: Some(crate::launchd::DaemonPauseReason::picker_override(
                    "builtin".into(),
                    "Built-in Output".into(),
                    Some("hdmi-1".into()),
                )),
            },
            Some(&logs),
        );
        assert!(has_row(&paused, "installed", "yes"));
        assert!(has_row(&paused, "running", "no"));
        assert!(has_row(&paused, "paused", "yes"));
        assert!(has_row(&paused, "reason", "picker override"));
        assert!(paused.contains("daemon is paused until `rusty-jack resume`"));

        let not_installed = format_daemon_block(
            &DaemonStatus::NotInstalled {
                plist_path: "/tmp/test.plist".into(),
            },
            Some(&logs),
        );
        assert!(has_row(&not_installed, "installed", "no"));
        assert!(has_row(&not_installed, "running", "no"));
        assert!(has_row(&not_installed, "paused", "no"));
    }

    #[test]
    fn test_format_driver_recommended_explains_selected_builtin() {
        let value = format_driver_recommended(&HdmiDisplayPortVolumeControlStatus {
            connected_output_present: true,
            native_driver_installed: false,
            native_driver_recommended: false,
            native_driver_recommendation_reason: Some(
                "selected output is not HDMI/DisplayPort".into(),
            ),
            native_driver_install_path: None,
            native_driver_scope: None,
            native_driver_stage: None,
            native_driver_warning: None,
            native_driver: None,
            eqmac_installed: false,
            eqmac_app_path: None,
            eqmac_hal_driver_path: None,
            orphaned_eqmac_hal_driver_path: None,
            eqmac_driver_backup: None,
            recommendation: None,
        });
        assert_eq!(value, "no (selected output is not HDMI/DisplayPort)");
    }

    #[test]
    fn test_format_volume_control_block_includes_driver_details() {
        let block =
            format_hdmi_displayport_volume_control_block(&HdmiDisplayPortVolumeControlStatus {
                connected_output_present: true,
                native_driver_installed: true,
                native_driver_recommended: false,
                native_driver_recommendation_reason: Some(
                    "native driver is already installed".into(),
                ),
                native_driver_install_path: Some(
                    "/Users/example/Library/Audio/Plug-Ins/HAL/RustyJack.driver".into(),
                ),
                native_driver_scope: Some("user".into()),
                native_driver_stage: Some("virtual-output-null".into()),
                native_driver_warning: Some("Rusty Jack is currently a null output.".into()),
                native_driver: Some(HalDriverInfo {
                    name: "Rusty Jack".into(),
                    bundle_id: "com.the-hcma.rusty-jack.driver".into(),
                    version: Some("0.1.1".into()),
                    stage: Some("virtual-output-null".into()),
                    install_path: "/Users/example/Library/Audio/Plug-Ins/HAL/RustyJack.driver"
                        .into(),
                }),
                eqmac_installed: false,
                eqmac_app_path: None,
                eqmac_hal_driver_path: None,
                orphaned_eqmac_hal_driver_path: None,
                eqmac_driver_backup: None,
                recommendation: None,
            });

        assert!(block.contains("driver scope"));
        assert!(block.contains("user"));
        assert!(block.contains("driver version"));
        assert!(block.contains("0.1.1"));
        assert!(block.contains("virtual-output-null"));
        assert!(block.contains("null output"));
    }

    #[test]
    fn test_format_volume_control_block_includes_eqmac_backup() {
        let block =
            format_hdmi_displayport_volume_control_block(&HdmiDisplayPortVolumeControlStatus {
                connected_output_present: true,
                native_driver_installed: true,
                native_driver_recommended: false,
                native_driver_recommendation_reason: None,
                native_driver_install_path: None,
                native_driver_scope: None,
                native_driver_stage: None,
                native_driver_warning: None,
                native_driver: None,
                eqmac_installed: true,
                eqmac_app_path: Some("/Applications/eqMac.app".into()),
                eqmac_hal_driver_path: None,
                orphaned_eqmac_hal_driver_path: None,
                eqmac_driver_backup: Some(EqMacDriverBackupInfo {
                    original_path: "/Library/Audio/Plug-Ins/HAL/eqMac.driver".into(),
                    backup_path: "/Users/example/.config/rusty-jack/driver-backups/eqMac.driver"
                        .into(),
                    version: Some("1.0.0".into()),
                    backed_up_at_unix: Some(1_800_000_000),
                }),
                recommendation: None,
            });

        assert!(block.contains("eqMac backup"));
        assert!(block.contains("driver-backups/eqMac.driver"));
        assert!(block.contains("rusty-jack driver swap-out"));
    }

    #[test]
    fn test_print_json_serializes_policy_fields() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
                scalar_webapi_mac_output: None,
            },
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"active_device_uid\""));
        assert!(json.contains("\"policy\""));
    }

    #[test]
    fn test_print_json_includes_volume_percent() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
                scalar_webapi_mac_output: None,
            },
            None,
            None,
            Some(13),
            Some(DaemonStatus::Running {
                label: crate::launchd::LAUNCH_AGENT_LABEL.into(),
                plist_path: "/tmp/test.plist".into(),
                service: "gui/501/com.example.rusty-jack".into(),
                pid: Some(123),
            }),
            Some(crate::launchd::DaemonLogPaths {
                file: "/tmp/rusty-jack.log".into(),
            }),
            None,
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"volume_percent\":13"));
        assert!(json.contains("\"daemon\""));
        assert!(json.contains("\"daemon_logs\""));
        assert!(json.contains("\"state\":\"running\""));
    }
}
