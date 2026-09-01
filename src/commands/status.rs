//! `rusty-jack status` — show active/default output and policy state.

use crate::config::{load_config_optional, resolve_config_path};
use crate::coreaudio::AudioHal;
use crate::launchd::daemon_supervisor_error_message;
use crate::scalar_webapi_device::ScalarDiscoveryFeedback;
use crate::status::{build_status, print_json, print_text, StatusDaemonContext};
use anyhow::Result;
use std::path::Path;

/// Show current default/active output and policy status.
pub fn run(hal: &dyn AudioHal, json: bool, config_path: Option<&Path>) -> Result<()> {
    let resolved = resolve_config_path(config_path);
    let explicit = config_path.is_some();
    let config = if let Some(path) = resolved.as_deref() {
        load_config_optional(path, explicit)?
    } else {
        None
    };

    let list = hal.list_outputs()?;
    let active_uid = list
        .devices
        .iter()
        .find(|d| d.is_active)
        .map(|d| d.uid.as_str());
    let volume_percent = active_uid.and_then(|uid| hal.output_volume_percent(uid));
    let daemon = crate::launchd::daemon_status().ok();
    let running_pid = daemon.as_ref().and_then(|status| match status {
        crate::launchd::DaemonStatus::Running { pid, .. } => *pid,
        _ => None,
    });
    let daemon_version = crate::launchd::daemon_version_check(running_pid).ok();
    let daemon_logs = crate::launchd::daemon_log_paths().ok();
    let activity = crate::state::load_activity_snapshot().ok().flatten();
    let scalar_probing_feedback = if json {
        ScalarDiscoveryFeedback::Silent
    } else {
        ScalarDiscoveryFeedback::Interactive
    };
    let snapshot = build_status(
        list,
        config.as_ref(),
        resolved.as_deref(),
        volume_percent,
        StatusDaemonContext {
            daemon,
            daemon_version,
            daemon_logs,
        },
        activity,
        scalar_probing_feedback,
    );

    if json {
        print_json(&snapshot)?;
    } else {
        print_text(&snapshot)?;
    }

    match &snapshot.daemon {
        None => anyhow::bail!("could not inspect LaunchAgent status"),
        Some(daemon) => {
            if let Some(message) = daemon_supervisor_error_message(daemon) {
                anyhow::bail!(message);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::launchd::{daemon_process_running, daemon_status};
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn assert_run_matches_daemon_health(hal: &MockHal, json: bool, config_path: Option<&Path>) {
        let result = run(hal, json, config_path);
        let daemon_healthy = daemon_status()
            .ok()
            .is_some_and(|status| daemon_process_running(&status));
        if daemon_healthy {
            result.unwrap();
        } else {
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("daemon"),
                "expected daemon health error, got: {err}"
            );
        }
    }

    #[test]
    fn test_run_json_includes_volume() {
        let hal = MockHal::new(vec![OutputDevice {
            id: 1,
            uid: "hdmi".into(),
            name: "Monitor".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: true,
            is_active: true,
        }])
        .with_output_volume(42);

        assert_run_matches_daemon_health(&hal, true, None);
    }

    #[test]
    fn test_run_json_does_not_panic() {
        let hal = MockHal::new(vec![OutputDevice {
            id: 1,
            uid: "hdmi".into(),
            name: "Monitor".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: true,
            is_active: true,
        }]);
        assert_run_matches_daemon_health(&hal, true, None);
    }

    #[test]
    fn test_run_with_config_file() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "auto_switch": true,
  "preferred_device": {{ "uid": "hdmi" }},
  "fallback_uids": []
}}"#
        )
        .unwrap();

        let hal = MockHal::new(vec![OutputDevice {
            id: 1,
            uid: "hdmi".into(),
            name: "Monitor".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: true,
            is_active: true,
        }]);

        assert_run_matches_daemon_health(&hal, true, Some(file.path()));
    }
}
