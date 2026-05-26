//! `rusty-jack status` — show active/default output and policy state.

use crate::config::{load_config_optional, resolve_config_path};
use crate::coreaudio::AudioHal;
use crate::status::{build_status, print_json, print_text};
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
    let snapshot = build_status(
        list,
        config.as_ref(),
        resolved.as_deref(),
        volume_percent,
        daemon,
    );

    if json {
        print_json(&snapshot)?;
    } else {
        print_text(&snapshot)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;
    use std::io::Write;
    use tempfile::NamedTempFile;

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

        run(&hal, true, None).unwrap();
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
        run(&hal, true, None).unwrap();
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

        run(&hal, true, Some(file.path())).unwrap();
    }
}
