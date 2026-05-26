//! `rusty-jack picker` — interactive output device selection.

use crate::apply::print_text;
use crate::config::{load_config_optional, resolve_config_path, Config};
use crate::coreaudio::AudioHal;
use crate::launchd::{
    daemon_status, pause_daemon_with_reason, DaemonPauseReason, DaemonStatus, PauseResult,
};
use crate::list_fmt::{terminal_supports_color, ANSI_CYAN, ANSI_RESET};
use crate::output_device::OutputDevice;
use crate::picker::{
    pick_and_switch, pick_device_index_with_notes, pick_device_index_with_refreshed_notes,
    preferred_uid_from_config, selection_overrides_preferred, volume_for_preferred_pick,
    PickSelection, PickerCancelled,
};
use crate::volume_memory::{remember_active_non_preferred, remembered_volume};
use anyhow::Result;
use dialoguer::Confirm;
use std::path::Path;

/// Show devices and switch to the user's choice.
pub fn run(
    hal: &dyn AudioHal,
    json: bool,
    index: Option<usize>,
    config_path: Option<&Path>,
) -> Result<()> {
    let list = hal.list_outputs().map_err(anyhow::Error::new)?;
    let config = load_picker_config(config_path)?;

    let preferred_uid = preferred_uid_from_config(config.as_ref(), &list.devices);
    let picker_notes = if index.is_none() && !json {
        config
            .as_ref()
            .map(|config| crate::scalar_webapi::picker_power_notes(config, &list.devices))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let selection = if index.is_none() && !json {
        pick_device_index_with_refreshed_notes(
            &list.devices,
            index,
            preferred_uid.as_deref(),
            &picker_notes,
            || {
                config
                    .as_ref()
                    .map(|config| crate::scalar_webapi::picker_power_notes(config, &list.devices))
                    .unwrap_or_default()
            },
        )
    } else {
        pick_device_index_with_notes(
            &list.devices,
            index,
            preferred_uid.as_deref(),
            &picker_notes,
        )
    }
    .map_err(anyhow::Error::new)?;

    match selection {
        PickSelection::Cancelled => {
            if json {
                let value = serde_json::to_string_pretty(&PickerCancelled::new())?;
                println!("{value}");
            } else {
                println!("Cancelled.");
            }
            return Ok(());
        }
        PickSelection::Selected(selection) => {
            let device = &list.devices[selection];
            let also_set_system_output = config
                .as_ref()
                .map(|c| c.also_set_system_output)
                .unwrap_or(true);
            let volume = volume_for_preferred_pick(config.as_ref(), &list.devices, &device.uid)
                .or_else(|| remembered_volume(&device.uid))
                .or_else(|| volume_for_manual_pick(hal, &list.devices, &device.uid));
            let pause_reason =
                match maybe_pause_daemon_for_override(json, preferred_uid.as_deref(), device)? {
                    PickerOverridePause::Continue(reason) => reason,
                    PickerOverridePause::Cancelled => {
                        if json {
                            let value = serde_json::to_string_pretty(&PickerCancelled::new())?;
                            println!("{value}");
                        } else {
                            println!("Cancelled. Daemon is still running.");
                        }
                        return Ok(());
                    }
                };
            remember_active_non_preferred(
                hal,
                &list.devices,
                preferred_uid.as_deref(),
                &device.uid,
            )
            .map_err(anyhow::Error::new)?;

            let result = pick_and_switch(
                hal,
                &list.devices,
                selection,
                also_set_system_output,
                volume,
            )
            .map_err(anyhow::Error::new)?;
            if let Some(config) = config.as_ref() {
                crate::scalar_webapi::warn_on_output_selected(config, &list.devices, &device.uid);
            }

            if json {
                let value = serde_json::to_string_pretty(&result)?;
                println!("{value}");
            } else {
                print_text(&result, &list);
                if let Some(reason) = pause_reason {
                    println!();
                    println!("{}", reason.message());
                }
            }
        }
    }

    Ok(())
}

fn load_picker_config(config_path: Option<&Path>) -> Result<Option<Config>> {
    let Some(path) = resolve_config_path(config_path) else {
        return Ok(None);
    };
    let explicit = config_path.is_some();
    Ok(load_config_optional(&path, explicit)?)
}

fn volume_for_manual_pick(
    hal: &dyn AudioHal,
    devices: &[OutputDevice],
    picked_uid: &str,
) -> Option<u8> {
    let active_uid = devices
        .iter()
        .find(|device| device.is_active)
        .map(|device| device.uid.as_str())?;
    if active_uid == picked_uid {
        return None;
    }

    hal.output_volume_percent(active_uid)
}

enum PickerOverridePause {
    Continue(Option<DaemonPauseReason>),
    Cancelled,
}

fn maybe_pause_daemon_for_override(
    json: bool,
    preferred_uid: Option<&str>,
    device: &OutputDevice,
) -> Result<PickerOverridePause> {
    if !selection_overrides_preferred(preferred_uid, &device.uid) {
        return Ok(PickerOverridePause::Continue(None));
    }

    if !matches!(
        daemon_status().map_err(anyhow::Error::new)?,
        DaemonStatus::Running { .. }
    ) {
        return Ok(PickerOverridePause::Continue(None));
    }

    if json || !crate::setup::terminal_is_interactive() {
        anyhow::bail!(
            "picker selected `{}` while the daemon is running; rerun interactively to confirm pausing auto-routing, or run `rusty-jack pause` first",
            device.friendly_label()
        );
    }

    let label = device.friendly_label();
    println!(
        "The daemon is running. Picking {label} instead of the configured preferred output will pause auto-routing until you run `rusty-jack resume`."
    );
    let prompt = if terminal_supports_color() {
        format!("{ANSI_CYAN}Continue{ANSI_RESET}")
    } else {
        "Continue".into()
    };
    let confirmed = Confirm::new()
        .with_prompt(prompt)
        .default(true)
        .interact()?;

    if !confirmed {
        return Ok(PickerOverridePause::Cancelled);
    }

    let reason = DaemonPauseReason::picker_override(
        device.uid.clone(),
        label,
        preferred_uid.map(str::to_string),
    );
    match pause_daemon_with_reason(Some(reason.clone())).map_err(anyhow::Error::new)? {
        PauseResult::Paused { .. } => Ok(PickerOverridePause::Continue(Some(reason))),
        PauseResult::NotInstalled { .. } => Ok(PickerOverridePause::Continue(None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn device(uid: &str, name: &str, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: name.into(),
            transport: TransportKind::BuiltIn,
            is_alive: true,
            is_default: false,
            is_active: active,
            monitor_name: None,
        }
    }

    #[test]
    fn test_run_picker_by_index() {
        let hal = MockHal::new(vec![
            device("builtin", "Built-in Output", true),
            OutputDevice {
                transport: TransportKind::Hdmi,
                monitor_name: Some("TV".into()),
                uid: "hdmi".into(),
                name: "HDMI".into(),
                ..device("hdmi", "HDMI", false)
            },
        ])
        .with_default("builtin");

        run(&hal, true, Some(1), None).unwrap();
        assert_eq!(hal.default_output_uid().unwrap().as_deref(), Some("hdmi"));
    }

    #[test]
    fn test_run_picker_preserves_volume_for_manual_pick() {
        let hal = MockHal::new(vec![
            device("builtin", "Built-in Output", true),
            OutputDevice {
                transport: TransportKind::Hdmi,
                monitor_name: Some("TV".into()),
                uid: "hdmi".into(),
                name: "HDMI".into(),
                ..device("hdmi", "HDMI", false)
            },
        ])
        .with_default("builtin")
        .with_output_volume(33);

        run(&hal, true, Some(1), None).unwrap();

        assert_eq!(
            hal.volume_calls(),
            vec![
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi".into(),
                    percent: 33,
                },
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi".into(),
                    percent: 33,
                }
            ]
        );
    }

    #[test]
    fn test_run_picker_no_change_json() {
        let hal =
            MockHal::new(vec![device("builtin", "Built-in Output", true)]).with_default("builtin");

        run(&hal, true, Some(0), None).unwrap();
        assert!(hal.set_calls().is_empty());
    }

    #[test]
    fn test_run_picker_applies_volume_for_preferred() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "hdmi" }},
  "volume": 25
}}"#
        )
        .unwrap();

        let hal = MockHal::new(vec![
            device("builtin", "Built-in Output", true),
            OutputDevice {
                transport: TransportKind::Hdmi,
                monitor_name: Some("TV".into()),
                uid: "hdmi".into(),
                name: "HDMI".into(),
                ..device("hdmi", "HDMI", false)
            },
        ])
        .with_default("builtin");

        run(&hal, true, Some(1), Some(file.path())).unwrap();
        assert_eq!(
            hal.volume_calls(),
            vec![
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi".into(),
                    percent: 25,
                },
                crate::coreaudio::mock::SetVolumeCall {
                    uid: "hdmi".into(),
                    percent: 25,
                }
            ]
        );
    }

    #[test]
    fn test_load_picker_config_explicit_missing_errors() {
        assert!(load_picker_config(Some(Path::new("/no/such/rusty-jack-config.json"))).is_err());
    }
}
