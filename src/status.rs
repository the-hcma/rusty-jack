//! Build and format `rusty-jack status` output.

use crate::config::Config;
use crate::hdmi_displayport_volume_control::{
    hdmi_displayport_volume_control_status_for_target, HdmiDisplayPortVolumeControlStatus,
};
use crate::launchd::{DaemonLogPaths, DaemonStatus, DaemonVersionCheck};
use crate::list_fmt::{self, format_detail_rows};
use crate::policy::evaluate_policy;
use crate::scalar_webapi_device::{self, ScalarDiscoveryFeedback, ScalarWebApiMacOutputLink};
use crate::state::ActivitySnapshot;
use crate::system_default::DeviceList;
use crate::version::BinaryVersion;
use anyhow::Result;
use chrono::{Local, TimeZone};
use dialoguer::console::style;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};
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

/// Daemon-related fields collected for [`build_status`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusDaemonContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon: Option<DaemonStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_version: Option<DaemonVersionCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_logs: Option<DaemonLogPaths>,
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
    pub daemon_version: Option<DaemonVersionCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_logs: Option<DaemonLogPaths>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<ActivitySnapshot>,
    pub binary_version: BinaryVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScalarWebApiStatus {
    pub enabled: bool,
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_model: Option<String>,
    pub mac_output_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_output_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_input_uses_default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_speaker_input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_input_matches: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_input_error: Option<String>,
}

/// Build a status snapshot from a device list and optional config.
#[must_use]
pub fn build_status(
    list: DeviceList,
    config: Option<&Config>,
    config_path: Option<&Path>,
    volume_percent: Option<u8>,
    daemon_context: StatusDaemonContext,
    activity: Option<ActivitySnapshot>,
    scalar_probing_feedback: ScalarDiscoveryFeedback,
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
    let scalar_webapi = build_scalar_webapi_status(
        config,
        scalar_webapi_mac_output.as_ref(),
        scalar_probing_feedback,
    );

    StatusSnapshot {
        devices: list.devices,
        system_default: list.system_default,
        policy,
        volume_percent,
        hdmi_displayport_volume_control,
        scalar_webapi,
        scalar_webapi_mac_output,
        daemon: daemon_context.daemon,
        daemon_version: daemon_context.daemon_version,
        daemon_logs: daemon_context.daemon_logs,
        activity,
        binary_version: BinaryVersion::current(),
    }
}

fn build_scalar_webapi_status(
    config: Option<&Config>,
    link: Option<&ScalarWebApiMacOutputLink>,
    feedback: ScalarDiscoveryFeedback,
) -> Option<ScalarWebApiStatus> {
    let api = config
        .and_then(|c| c.scalar_webapi_device.as_ref())
        .filter(|api| api.enabled)?;
    scalar_webapi_device::with_scalar_probing_feedback(
        feedback,
        "  probing ScalarWebAPI speaker",
        || {
            Some(ScalarWebApiStatus {
                enabled: api.enabled,
                host: api.host.clone(),
                model: Some(api.model.clone()),
                speaker_model: scalar_webapi_device::cached_speaker_model_for_display(api)
                    .and_then(|hardware| {
                        scalar_webapi_device::should_show_distinct_speaker_model(
                            &api.model, &hardware,
                        )
                        .then_some(hardware)
                    }),
                mac_output_uid: link
                    .map(|link| link.mac_output_uid.clone())
                    .or_else(|| api.mac_output.uid.clone()),
                mac_output_label: link.and_then(|link| link.mac_output_label.clone()),
                power_status: scalar_webapi_device::current_power_status_for_display(api),
                speaker_input: scalar_webapi_device::configured_speaker_input_label(api),
                speaker_input_uses_default: Some(scalar_webapi_device::speaker_input_uses_default(
                    api,
                )),
                active_speaker_input: scalar_webapi_device::current_scalar_webapi_speaker_input(
                    api,
                ),
                speaker_input_matches: scalar_webapi_device::speaker_input_matches_config(api),
                speaker_input_error:
                    scalar_webapi_device::configured_speaker_input_validation_error(api),
            })
        },
    )
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
    if let Some(speaker_model) = &status.speaker_model {
        rows.push(("speaker model", speaker_model.clone()));
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

    if let Some(input) = &status.speaker_input {
        let label = if status.speaker_input_uses_default == Some(true) {
            format!("{input} (default)")
        } else {
            input.clone()
        };
        rows.push(("configured input", label));
    }
    if let Some(active_input) = &status.active_speaker_input {
        rows.push(("active input", active_input.clone()));
    }
    if let Some(error) = &status.speaker_input_error {
        rows.push(("input error", error.clone()));
    }
    if let Some(matches) = status.speaker_input_matches {
        rows.push((
            "input matches",
            if matches { "yes".into() } else { "no".into() },
        ));
    }

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

fn format_daemon_log_follow_command(log_path: &str) -> String {
    format!("tail -F {log_path}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonDisplayRow {
    error: bool,
    label: String,
    value: String,
}

fn daemon_display_rows(
    daemon: &DaemonStatus,
    daemon_version: Option<&DaemonVersionCheck>,
    daemon_logs: Option<&DaemonLogPaths>,
) -> Vec<DaemonDisplayRow> {
    let mut rows: Vec<DaemonDisplayRow> = Vec::new();
    let supervisor_error = crate::launchd::daemon_supervisor_error_message(daemon);
    match daemon {
        DaemonStatus::Running {
            label,
            plist_path,
            service,
            pid,
            launch_job_state,
            last_exit_code,
        } => {
            let process_running = pid.is_some();
            rows.push(DaemonDisplayRow {
                error: !process_running,
                label: "state".into(),
                value: if process_running {
                    "running".into()
                } else {
                    "not running".into()
                },
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "installed".into(),
                value: "yes".into(),
            });
            rows.push(DaemonDisplayRow {
                error: !process_running,
                label: "running".into(),
                value: if process_running { "yes" } else { "no" }.into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "paused".into(),
                value: "no".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "label".into(),
                value: label.clone(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "service".into(),
                value: service.clone(),
            });
            if let Some(pid) = pid {
                rows.push(DaemonDisplayRow {
                    error: false,
                    label: "pid".into(),
                    value: pid.to_string(),
                });
            }
            if let Some(state) = launch_job_state {
                rows.push(DaemonDisplayRow {
                    error: !process_running,
                    label: "launchd state".into(),
                    value: state.clone(),
                });
            }
            if let Some(code) = last_exit_code
                .as_ref()
                .filter(|code| *code != "(never exited)")
            {
                rows.push(DaemonDisplayRow {
                    error: true,
                    label: "last exit code".into(),
                    value: code.clone(),
                });
            }
            rows.push(DaemonDisplayRow {
                error: false,
                label: "plist".into(),
                value: plist_path.clone(),
            });
        }
        DaemonStatus::Paused {
            label,
            plist_path,
            service,
            pause_reason,
        } => {
            rows.push(DaemonDisplayRow {
                error: true,
                label: "state".into(),
                value: "paused".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "installed".into(),
                value: "yes".into(),
            });
            rows.push(DaemonDisplayRow {
                error: true,
                label: "running".into(),
                value: "no".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "paused".into(),
                value: "yes".into(),
            });
            if let Some(reason) = pause_reason {
                rows.push(DaemonDisplayRow {
                    error: false,
                    label: "reason".into(),
                    value: reason.label().into(),
                });
                rows.push(DaemonDisplayRow {
                    error: false,
                    label: "note".into(),
                    value: reason.message(),
                });
            }
            rows.push(DaemonDisplayRow {
                error: false,
                label: "label".into(),
                value: label.clone(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "service".into(),
                value: service.clone(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "plist".into(),
                value: plist_path.clone(),
            });
        }
        DaemonStatus::NotInstalled { plist_path } => {
            rows.push(DaemonDisplayRow {
                error: true,
                label: "state".into(),
                value: "not installed".into(),
            });
            rows.push(DaemonDisplayRow {
                error: true,
                label: "installed".into(),
                value: "no".into(),
            });
            rows.push(DaemonDisplayRow {
                error: true,
                label: "running".into(),
                value: "no".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "paused".into(),
                value: "no".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "expected plist".into(),
                value: plist_path.clone(),
            });
        }
        DaemonStatus::Unknown {
            label,
            plist_path,
            message,
        } => {
            rows.push(DaemonDisplayRow {
                error: true,
                label: "state".into(),
                value: "unknown".into(),
            });
            rows.push(DaemonDisplayRow {
                error: true,
                label: "installed".into(),
                value: "unknown".into(),
            });
            rows.push(DaemonDisplayRow {
                error: true,
                label: "running".into(),
                value: "unknown".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "paused".into(),
                value: "unknown".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "label".into(),
                value: label.clone(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "plist".into(),
                value: plist_path.clone(),
            });
            rows.push(DaemonDisplayRow {
                error: true,
                label: "note".into(),
                value: message.clone(),
            });
        }
    }

    if let Some(message) = supervisor_error {
        rows.push(DaemonDisplayRow {
            error: true,
            label: "error".into(),
            value: message,
        });
    }

    if let Some(logs) = daemon_logs {
        rows.push(DaemonDisplayRow {
            error: false,
            label: "log".into(),
            value: logs.file.clone(),
        });
        rows.push(DaemonDisplayRow {
            error: false,
            label: "log follow".into(),
            value: format_daemon_log_follow_command(&logs.file),
        });
    }

    if let Some(check) = daemon_version {
        if let Some(version) = check
            .running_binary_version
            .as_ref()
            .or(check.plist_binary_version.as_ref())
        {
            rows.push(DaemonDisplayRow {
                error: false,
                label: "daemon binary".into(),
                value: version.display(),
            });
        }
        if check.stale {
            rows.push(DaemonDisplayRow {
                error: false,
                label: "daemon stale".into(),
                value: "yes".into(),
            });
            rows.push(DaemonDisplayRow {
                error: false,
                label: "note".into(),
                value: daemon_stale_note(check),
            });
        }
    }

    rows
}

#[cfg(test)]
fn format_daemon_block(
    daemon: &DaemonStatus,
    daemon_version: Option<&DaemonVersionCheck>,
    daemon_logs: Option<&DaemonLogPaths>,
) -> String {
    let rows = daemon_display_rows(daemon, daemon_version, daemon_logs);
    let borrowed: Vec<(&str, &str)> = rows
        .iter()
        .map(|row| (row.label.as_str(), row.value.as_str()))
        .collect();
    format_labeled_section("Daemon", "  ", &borrowed)
}

fn write_daemon_block(out: &mut impl Write, rows: &[DaemonDisplayRow]) -> Result<()> {
    writeln!(out, "Daemon")?;
    let use_color = io::stdout().is_terminal();
    let width = rows.iter().map(|row| row.label.len()).max().unwrap_or(0);
    for row in rows {
        let line = format!("  {:width$}: {}", row.label, row.value, width = width);
        if use_color && row.error {
            writeln!(out, "{}", style(line).red().bold())?;
        } else {
            writeln!(out, "{line}")?;
        }
    }
    Ok(())
}

fn format_labeled_section(title: &str, indent: &str, rows: &[(&str, &str)]) -> String {
    let mut lines = vec![title.to_string()];
    lines.extend(format_detail_rows(indent, rows));
    lines.join("\n")
}

fn daemon_stale_note(check: &DaemonVersionCheck) -> String {
    let current = check.cli_version.display();
    if check.needs_version_stamp_refresh {
        return format!(
            "LaunchAgent is missing stamped daemon version metadata; CLI is {}; run `{}`",
            current, check.refresh_command
        );
    }
    if let Some(running) = check
        .running_binary_version
        .as_ref()
        .filter(|version| !version.matches(&check.cli_version))
    {
        return format!(
            "running daemon is {} but CLI is {}; run `{}`",
            running.display(),
            current,
            check.refresh_command
        );
    }
    if check
        .plist_binary_path
        .as_deref()
        .is_some_and(|path| path != check.cli_binary_path)
    {
        return format!(
            "LaunchAgent points at a different binary than this CLI; run `{}`",
            check.refresh_command
        );
    }
    if let Some(plist_version) = check
        .plist_binary_version
        .as_ref()
        .filter(|version| !version.matches(&check.cli_version))
    {
        return format!(
            "LaunchAgent binary is {} but CLI is {}; run `{}`",
            plist_version.display(),
            current,
            check.refresh_command
        );
    }
    format!(
        "daemon is stale; CLI is {}; run `{}`",
        current, check.refresh_command
    )
}

fn format_daemon_logs_block(logs: &DaemonLogPaths) -> String {
    format_labeled_section(
        "Daemon",
        "  ",
        &[
            ("log", logs.file.as_str()),
            (
                "log follow",
                format_daemon_log_follow_command(&logs.file).as_str(),
            ),
        ],
    )
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
        let event = snapshot
            .last_became_active_event
            .as_deref()
            .map(|label| format!(" event={label}"))
            .unwrap_or_default();
        rows.push((
            "last became_active",
            format!("{} (console user {user}){event}", format_unix_local(at)),
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
        write_daemon_block(
            &mut out,
            &daemon_display_rows(
                daemon,
                snapshot.daemon_version.as_ref(),
                snapshot.daemon_logs.as_ref(),
            ),
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
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        format_binary_version_block(&snapshot.binary_version)
    )?;
    Ok(())
}

fn format_binary_version_block(version: &BinaryVersion) -> String {
    let rows = [
        ("version", version.version.as_str()),
        ("commit", version.commit.as_str()),
    ];
    format_labeled_section("Rusty Jack", "  ", &rows)
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn empty_daemon_context() -> StatusDaemonContext {
        StatusDaemonContext {
            daemon: None,
            daemon_version: None,
            daemon_logs: None,
        }
    }

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

    fn spawn_scalar_status_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let len = stream.read(&mut request).unwrap_or(0);
                let payload = String::from_utf8_lossy(&request[..len]);
                if payload.contains("Upgrade: websocket") {
                    let response =
                        "HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response_body = r#"{"result":[{"status":"standby"}],"id":1}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                }
            }
        });
        port
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
            empty_daemon_context(),
            None,
            ScalarDiscoveryFeedback::Silent,
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
            activity_monitor: "idle".into(),
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
            empty_daemon_context(),
            None,
            ScalarDiscoveryFeedback::Silent,
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
            empty_daemon_context(),
            None,
            ScalarDiscoveryFeedback::Silent,
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
    fn test_build_status_includes_binary_version() {
        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
                scalar_webapi_mac_output: None,
            },
            None,
            None,
            None,
            empty_daemon_context(),
            None,
            ScalarDiscoveryFeedback::Silent,
        );
        assert!(!snapshot.binary_version.version.is_empty());
        assert!(!snapshot.binary_version.commit.is_empty());
    }

    #[test]
    fn test_format_binary_version_block() {
        let block = format_binary_version_block(&BinaryVersion {
            version: "0.4.1".into(),
            commit: "9dc370e".into(),
        });
        assert!(block.contains("Rusty Jack"));
        assert!(block.contains("0.4.1"));
        assert!(block.contains("9dc370e"));
    }

    #[test]
    fn test_format_scalar_webapi_block_includes_model_and_mac_output_label() {
        let block = format_scalar_webapi_block(&ScalarWebApiStatus {
            enabled: true,
            host: Some("192.168.86.18".into()),
            model: Some("The Lair".into()),
            speaker_model: None,
            mac_output_uid: Some("BuiltInHeadphoneOutputDevice".into()),
            mac_output_label: Some("External Headphones".into()),
            power_status: Some("active".into()),
            speaker_input: Some("Audio in".into()),
            speaker_input_uses_default: Some(false),
            active_speaker_input: Some("Audio in".into()),
            speaker_input_matches: Some(true),
            speaker_input_error: None,
        });
        assert!(block.contains("model"));
        assert!(block.contains("The Lair"));
        assert!(block.contains("External Headphones (BuiltInHeadphoneOutputDevice)"));
    }

    #[test]
    fn test_build_status_uses_display_power_status_lookup() {
        let port = spawn_scalar_status_server();
        let config = Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            activity_monitor: "idle".into(),
            preferred_device: DeviceSelectorConfig {
                name: None,
                uid: Some("hdmi-1".into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            scalar_webapi_device: Some(crate::config::ScalarWebApiDeviceConfig {
                enabled: true,
                model: "The Lair".into(),
                host: Some("127.0.0.1".into()),
                port,
                path: concat!("so", "ny").into(),
                mac_output: DeviceSelectorConfig {
                    name: None,
                    uid: Some("hdmi-1".into()),
                },
                triggers: vec!["output_selected".into()],
                wake_debounce_ms: 30_000,
                request_timeout_ms: 3_000,
                require_quick_start: true,
                speaker_input: None,
            }),
            ..Default::default()
        };

        let snapshot = build_status(
            DeviceList {
                devices: vec![hdmi_device(true)],
                system_default: None,
                scalar_webapi_mac_output: None,
            },
            Some(&config),
            None,
            None,
            empty_daemon_context(),
            None,
            ScalarDiscoveryFeedback::Silent,
        );
        assert_eq!(
            snapshot
                .scalar_webapi
                .as_ref()
                .and_then(|status| status.power_status.as_deref()),
            Some("standby")
        );
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
            last_became_active_event: Some("KeyDown".into()),
        });
        assert!(block.contains("Activity"));
        assert!(block.contains("12.4s (threshold 60.0s)"));
        assert!(block.contains("console user"));
        assert!(block.contains("hcma"));
        assert!(block.contains("event=KeyDown"));
        assert!(block.contains("keyboard, mouse"));
    }

    #[test]
    fn test_format_daemon_block_flags_stale_daemon() {
        use crate::launchd::DaemonVersionCheck;

        let check = DaemonVersionCheck {
            cli_binary_path: "/opt/homebrew/bin/rusty-jack".into(),
            cli_version: BinaryVersion {
                version: "0.4.2".into(),
                commit: "newcommit".into(),
            },
            plist_binary_path: Some("/opt/homebrew/Cellar/rusty-jack/0.4.1/bin/rusty-jack".into()),
            plist_binary_version: Some(BinaryVersion {
                version: "0.4.1".into(),
                commit: "oldcommit".into(),
            }),
            running_binary_version: None,
            needs_version_stamp_refresh: false,
            stale: true,
            refresh_command: crate::launchd::DAEMON_REFRESH_COMMAND,
        };
        let block = format_daemon_block(
            &DaemonStatus::Running {
                label: crate::launchd::LAUNCH_AGENT_LABEL.into(),
                plist_path: "/tmp/test.plist".into(),
                service: "gui/501/com.example.rusty-jack".into(),
                pid: Some(123),
                launch_job_state: None,
                last_exit_code: None,
            },
            Some(&check),
            None,
        );
        assert!(block.contains("daemon stale"));
        assert!(block.contains("rusty-jack upgrade --force"));
        assert!(block.contains("0.4.1 (commit oldcommit)"));
    }

    #[test]
    fn test_format_daemon_block_flags_brew_upgrade_stale_running_env() {
        use crate::launchd::DaemonVersionCheck;

        let check = DaemonVersionCheck {
            cli_binary_path: "/opt/homebrew/bin/rusty-jack".into(),
            cli_version: BinaryVersion {
                version: "0.6.0".into(),
                commit: "f289256".into(),
            },
            plist_binary_path: Some("/opt/homebrew/bin/rusty-jack".into()),
            plist_binary_version: Some(BinaryVersion {
                version: "0.6.0".into(),
                commit: "f289256".into(),
            }),
            running_binary_version: Some(BinaryVersion {
                version: "0.5.0".into(),
                commit: "oldcommit".into(),
            }),
            needs_version_stamp_refresh: false,
            stale: true,
            refresh_command: crate::launchd::DAEMON_REFRESH_COMMAND,
        };
        let block = format_daemon_block(
            &DaemonStatus::Running {
                label: crate::launchd::LAUNCH_AGENT_LABEL.into(),
                plist_path: "/tmp/test.plist".into(),
                service: "gui/503/com.example.rusty-jack".into(),
                pid: Some(2952),
                launch_job_state: None,
                last_exit_code: None,
            },
            Some(&check),
            None,
        );
        assert!(block.contains("daemon stale"));
        assert!(block.contains("running daemon is 0.5.0 (commit oldcommit)"));
        assert!(block.contains("rusty-jack upgrade --force"));
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
                launch_job_state: None,
                last_exit_code: None,
            },
            None,
            Some(&logs),
        );
        assert!(has_row(&running, "installed", "yes"));
        assert!(has_row(&running, "running", "yes"));
        assert!(has_row(&running, "paused", "no"));
        assert!(has_row(&running, "log", "/tmp/rusty-jack.log"));
        assert!(has_row(
            &running,
            "log follow",
            "tail -F /tmp/rusty-jack.log"
        ));

        let loaded_not_running = format_daemon_block(
            &DaemonStatus::Running {
                label: crate::launchd::LAUNCH_AGENT_LABEL.into(),
                plist_path: "/tmp/test.plist".into(),
                service: "gui/501/com.example.rusty-jack".into(),
                pid: None,
                launch_job_state: Some("spawn scheduled".into()),
                last_exit_code: Some("78: EX_CONFIG".into()),
            },
            None,
            Some(&logs),
        );
        assert!(has_row(&loaded_not_running, "state", "not running"));
        assert!(has_row(&loaded_not_running, "running", "no"));
        assert!(has_row(
            &loaded_not_running,
            "launchd state",
            "spawn scheduled"
        ));
        assert!(has_row(
            &loaded_not_running,
            "last exit code",
            "78: EX_CONFIG"
        ));
        assert!(has_row(
            &loaded_not_running,
            "error",
            "daemon not running (launchd state=spawn scheduled) (last exit code=78: EX_CONFIG); run `rusty-jack upgrade --force` or `rusty-jack resume`"
        ));

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
            None,
            Some(&logs),
        );
        assert!(has_row(&paused, "installed", "yes"));
        assert!(has_row(&paused, "running", "no"));
        assert!(has_row(&paused, "paused", "yes"));
        assert!(has_row(&paused, "reason", "picker override"));
        assert!(paused.contains("daemon is paused until `rusty-jack resume`"));
        assert!(has_row(
            &paused,
            "error",
            "daemon paused; run `rusty-jack resume`"
        ));

        let not_installed = format_daemon_block(
            &DaemonStatus::NotInstalled {
                plist_path: "/tmp/test.plist".into(),
            },
            None,
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
            empty_daemon_context(),
            None,
            ScalarDiscoveryFeedback::Silent,
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
            StatusDaemonContext {
                daemon: Some(DaemonStatus::Running {
                    label: crate::launchd::LAUNCH_AGENT_LABEL.into(),
                    plist_path: "/tmp/test.plist".into(),
                    service: "gui/501/com.example.rusty-jack".into(),
                    pid: Some(123),
                    launch_job_state: None,
                    last_exit_code: None,
                }),
                daemon_version: None,
                daemon_logs: Some(crate::launchd::DaemonLogPaths {
                    file: "/tmp/rusty-jack.log".into(),
                }),
            },
            None,
            ScalarDiscoveryFeedback::Silent,
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"volume_percent\":13"));
        assert!(json.contains("\"daemon\""));
        assert!(json.contains("\"daemon_logs\""));
        assert!(json.contains("\"state\":\"running\""));
    }
}
