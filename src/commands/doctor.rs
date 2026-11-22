//! `rusty-jack doctor` — diagnose why Rusty Jack may not be working.
//!
//! Checks the macOS privacy permissions the config needs, then the daemon (installed, running,
//! paused, stale), eqMac/volume-control readiness, audio routing, and recent daemon log
//! problems. Everything reuses the data `status` already computes.

use crate::config::{load_config_optional_readonly, resolve_config_path, Config};
use crate::coreaudio::AudioHal;
use crate::doctor::{build_privacy_report, daemon_log_path, read_log_tail};
use crate::doctor_health::{health_checks, summarize_daemon_log, HealthInputs};
use crate::hdmi_displayport_volume_control::{
    hdmi_displayport_volume_control_status_for_target, route_needs_hdmi_displayport_volume_control,
};
use crate::policy::evaluate_policy;
use anyhow::Result;
use std::path::Path;

/// Gather the daemon, routing, volume-control and log facts and run the health checks.
fn collect_health_checks(
    hal: Option<&dyn AudioHal>,
    config: Option<&Config>,
    config_path: Option<&Path>,
) -> Vec<crate::doctor::DoctorCheck> {
    let daemon = crate::launchd::daemon_status().ok();
    let running_pid = daemon.as_ref().and_then(|status| match status {
        crate::launchd::DaemonStatus::Running { pid, .. } => *pid,
        _ => None,
    });
    let version = crate::launchd::daemon_version_check(running_pid).ok();

    let list = hal.and_then(|hal| hal.list_outputs().ok());
    let policy = list
        .as_ref()
        .map(|list| evaluate_policy(list, config, config_path));
    let selected_uid = policy.as_ref().and_then(|policy| {
        policy
            .preferred_device_uid
            .as_deref()
            .or(policy.active_device_uid.as_deref())
    });
    let volume = list
        .as_ref()
        .map(|list| hdmi_displayport_volume_control_status_for_target(&list.devices, selected_uid));
    let volume_needed = match (&list, selected_uid) {
        (Some(list), Some(uid)) => route_needs_hdmi_displayport_volume_control(&list.devices, uid),
        _ => false,
    };

    let log = daemon_log_path(config)
        .map(|path| summarize_daemon_log(&read_log_tail(&path)))
        .unwrap_or_default();

    health_checks(&HealthInputs {
        config_loaded: config.is_some(),
        auto_switch: config.is_some_and(|config| config.auto_switch),
        daemon: daemon.as_ref(),
        version: version.as_ref(),
        policy: policy.as_ref(),
        volume: volume.as_ref(),
        volume_needed,
        eqmac_running: crate::eqmac::eqmac_is_running(),
        eqmac_virtual_default: crate::eqmac::eqmac_virtual_default_is_active(),
        log: &log,
    })
}

/// Run the diagnostics; exits non-zero when any check fails.
pub fn run(json: bool, config_path: Option<&Path>) -> Result<()> {
    let resolved = resolve_config_path(config_path);
    let config = match resolved.as_deref() {
        Some(path) => load_config_optional_readonly(path, config_path.is_some())?,
        None => None,
    };
    let mut report = build_privacy_report(config.as_ref(), true)?;
    let hal = crate::coreaudio::platform_hal().ok();
    report.checks.extend(collect_health_checks(
        hal.as_deref(),
        config.as_ref(),
        resolved.as_deref(),
    ));

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        match (&config, resolved.as_deref()) {
            (Some(_), Some(path)) => println!("Config: {}", path.display()),
            _ => println!(
                "Config: none loaded (permissions depend on config; run `rusty-jack config init`)"
            ),
        }
        println!();
        println!("{}", report.format("Diagnosis"));
        println!();
        println!("{}", report.conclusion(config.is_some()));
    }

    if report.has_failures() {
        anyhow::bail!("rusty-jack doctor found problems");
    }
    Ok(())
}
