//! Interactive and scripted output device selection.

use crate::apply::{switch_output, ApplyResult, SwitchOptions};
use crate::config::Config;
use crate::coreaudio::AudioHal;
use crate::eqmac::{ensure_eqmac_for_target, format_ensure_messages};
use crate::list_fmt::{self, ANSI_CYAN, ANSI_DIM, ANSI_GREEN, ANSI_RESET};
use crate::output_device::OutputDevice;
use crate::policy::{RoutingTarget, RoutingTargetSource};
use crate::RustyJackError;
use dialoguer::{theme::ColorfulTheme, Select};
use serde::Serialize;
use std::io::{self, IsTerminal};

/// Outcome of an interactive or scripted device pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickSelection {
    Selected(usize),
    Cancelled,
}

/// JSON result when the user cancels the interactive picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PickerCancelled {
    status: &'static str,
}

impl Default for PickerCancelled {
    fn default() -> Self {
        Self::new()
    }
}

impl PickerCancelled {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            status: "cancelled",
        }
    }
}

const PICKER_PROMPT: &str =
    "Select output device (↑↓, Enter, Esc to cancel)  (> active, * preferred, dim = not routable)";

/// Resolve the configured preferred device UID against live outputs.
#[must_use]
pub fn preferred_uid_from_config(
    config: Option<&Config>,
    devices: &[OutputDevice],
) -> Option<String> {
    let config = config?;
    crate::device_select::resolve_device_selector(&config.preferred_selector(), devices).ok()
}

fn picker_prefix(active: bool, preferred: bool) -> &'static str {
    match (active, preferred) {
        (true, true) => ">* ",
        (true, false) => ">  ",
        (false, true) => "*  ",
        (false, false) => "   ",
    }
}

fn colorize_picker_label(
    text: &str,
    device: &OutputDevice,
    preferred: bool,
    use_color: bool,
) -> String {
    if !use_color {
        return text.to_string();
    }
    if !device.is_selectable() {
        return format!("{ANSI_DIM}{text}{ANSI_RESET}");
    }
    if device.is_active {
        format!("{ANSI_GREEN}{text}{ANSI_RESET}")
    } else if preferred {
        format!("{ANSI_CYAN}{text}{ANSI_RESET}")
    } else {
        text.to_string()
    }
}

/// Format a device row for the interactive picker.
#[must_use]
pub fn format_picker_label(device: &OutputDevice) -> String {
    format_picker_label_with_options(device, None, false)
}

/// Format a picker row with optional config-preferred marker and terminal color.
#[must_use]
pub fn format_picker_label_with_options(
    device: &OutputDevice,
    preferred_uid: Option<&str>,
    use_color: bool,
) -> String {
    let preferred = preferred_uid == Some(device.uid.as_str());
    let prefix = picker_prefix(device.is_active, preferred);
    let suffix = device
        .non_selectable_reason()
        .map(|reason| format!(" — {reason}"))
        .unwrap_or_default();
    let body = format!(
        "{prefix}{} — {}{suffix}",
        device.friendly_label(),
        device.transport
    );
    colorize_picker_label(&body, device, preferred, use_color)
}

fn ensure_selectable(device: &OutputDevice) -> Result<(), RustyJackError> {
    if device.is_selectable() {
        return Ok(());
    }
    let reason = device
        .non_selectable_reason()
        .unwrap_or("not a routable output");
    Err(RustyJackError::Config(format!(
        "{} — {reason}",
        device.friendly_label()
    )))
}

fn default_picker_index(devices: &[OutputDevice]) -> usize {
    devices
        .iter()
        .position(|d| d.is_active && d.is_selectable())
        .or_else(|| devices.iter().position(|d| d.is_selectable()))
        .unwrap_or(0)
}

/// Pick a device index interactively, or use `index` when provided.
pub fn pick_device_index(
    devices: &[OutputDevice],
    index: Option<usize>,
    preferred_uid: Option<&str>,
) -> Result<PickSelection, RustyJackError> {
    pick_device_index_with_notes(devices, index, preferred_uid, &[])
}

/// Pick a device index with optional per-device notes shown in the interactive menu.
pub fn pick_device_index_with_notes(
    devices: &[OutputDevice],
    index: Option<usize>,
    preferred_uid: Option<&str>,
    notes: &[(String, String)],
) -> Result<PickSelection, RustyJackError> {
    if devices.is_empty() {
        return Err(RustyJackError::Config(
            "no output devices available to pick".into(),
        ));
    }

    if let Some(index) = index {
        if index >= devices.len() {
            return Err(RustyJackError::Config(format!(
                "device index {index} out of range (0–{})",
                devices.len() - 1
            )));
        }
        ensure_selectable(&devices[index])?;
        return Ok(PickSelection::Selected(index));
    }

    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        return Err(RustyJackError::Config(
            "interactive picker requires a TTY — pass --index or redirect from a terminal".into(),
        ));
    }

    let use_color = list_fmt::terminal_supports_color();

    loop {
        let labels: Vec<String> = devices
            .iter()
            .map(|device| {
                let label = format_picker_label_with_options(device, preferred_uid, use_color);
                append_picker_note(label, note_for_uid(notes, &device.uid))
            })
            .collect();
        let default = default_picker_index(devices);

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt(PICKER_PROMPT)
            .items(&labels)
            .default(default)
            .interact_opt()
            .map_err(|err| RustyJackError::Config(format!("picker failed: {err}")))?;

        match selection {
            None => return Ok(PickSelection::Cancelled),
            Some(index) => {
                let device = &devices[index];
                if device.is_selectable() {
                    return Ok(PickSelection::Selected(index));
                }
                let reason = device
                    .non_selectable_reason()
                    .unwrap_or("not a routable output");
                eprintln!(
                    "Cannot switch to {} — {reason}. Pick a speaker, monitor, or dock output.",
                    device.friendly_label()
                );
            }
        }
    }
}

fn note_for_uid<'a>(notes: &'a [(String, String)], uid: &str) -> Option<&'a str> {
    notes
        .iter()
        .find(|(note_uid, _)| note_uid == uid)
        .map(|(_, note)| note.as_str())
}

fn append_picker_note(mut label: String, note: Option<&str>) -> String {
    let Some(note) = note.filter(|value| !value.trim().is_empty()) else {
        return label;
    };
    label.push_str(" — ");
    label.push_str(note);
    label
}

/// When the picked device is the configured preferred device, return config volume.
#[must_use]
pub fn volume_for_preferred_pick(
    config: Option<&Config>,
    devices: &[OutputDevice],
    picked_uid: &str,
) -> Option<u8> {
    let config = config?;
    let volume = config.volume?;
    let preferred_uid = preferred_uid_from_config(Some(config), devices)?;
    (preferred_uid == picked_uid).then_some(volume)
}

/// Switch to the chosen list index.
pub fn pick_and_switch(
    hal: &dyn AudioHal,
    devices: &[OutputDevice],
    index: usize,
    also_set_system_output: bool,
    volume: Option<u8>,
) -> Result<ApplyResult, RustyJackError> {
    let device = devices
        .get(index)
        .ok_or_else(|| RustyJackError::Config(format!("device index {index} out of range")))?;

    let eqmac = ensure_eqmac_for_target(devices, &device.uid)?;
    for line in format_ensure_messages(eqmac) {
        eprintln!("{line}");
    }

    let target = RoutingTarget {
        uid: device.uid.clone(),
        name: device.name.clone(),
        monitor_name: device.monitor_name.clone(),
        source: RoutingTargetSource::Picker,
    };

    switch_output(
        hal,
        &target,
        &SwitchOptions {
            also_set_system_output,
            volume,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig};
    use crate::coreaudio::mock::MockHal;
    use crate::transport::TransportKind;

    fn hdmi_device(uid: &str, monitor: &str) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: Some(monitor.into()),
        }
    }

    fn device(name: &str, monitor: Option<&str>, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: "uid".into(),
            name: name.into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
            monitor_name: monitor.map(str::to_string),
        }
    }

    fn zoom_device() -> OutputDevice {
        OutputDevice {
            id: 9,
            uid: "zoom.us:0".into(),
            name: "ZoomAudioDevice".into(),
            transport: TransportKind::Virtual,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: None,
        }
    }

    #[test]
    fn test_format_picker_label_marks_zoom_dimmed() {
        let label = format_picker_label_with_options(&zoom_device(), None, true);
        assert!(label.contains("ZoomAudioDevice"));
        assert!(label.contains("app virtual"));
        assert!(label.starts_with(ANSI_DIM));
    }

    #[test]
    fn test_pick_device_index_rejects_zoom_by_index() {
        let devices = vec![device("HDMI", Some("TV"), true), zoom_device()];
        assert!(pick_device_index(&devices, Some(1), None).is_err());
    }

    #[test]
    fn test_format_picker_label_marks_active() {
        let active = format_picker_label(&device("HDMI", Some("DELL U3219Q"), true));
        assert!(active.starts_with(">  HDMI (DELL U3219Q)"));
        let idle = format_picker_label(&device("Built-in Output", None, false));
        assert!(idle.starts_with("   Built-in Output"));
    }

    #[test]
    fn test_format_picker_label_marks_preferred() {
        let preferred = format_picker_label_with_options(
            &device("HDMI", Some("DELL U3219Q"), false),
            Some("uid"),
            false,
        );
        assert!(preferred.starts_with("*  HDMI (DELL U3219Q)"));
    }

    #[test]
    fn test_format_picker_label_marks_active_and_preferred() {
        let both = format_picker_label_with_options(
            &device("HDMI", Some("DELL U3219Q"), true),
            Some("uid"),
            false,
        );
        assert!(both.starts_with(">* HDMI (DELL U3219Q)"));
    }

    #[test]
    fn test_format_picker_label_colors_preferred() {
        let preferred = format_picker_label_with_options(
            &device("HDMI", Some("DELL U3219Q"), false),
            Some("uid"),
            true,
        );
        assert!(preferred.starts_with(ANSI_CYAN));
        assert!(preferred.ends_with(ANSI_RESET));
    }

    #[test]
    fn test_format_picker_label_colors_active() {
        let active = format_picker_label_with_options(
            &device("HDMI", Some("DELL U3219Q"), true),
            None,
            true,
        );
        assert!(active.starts_with(ANSI_GREEN));
    }

    #[test]
    fn test_preferred_uid_from_config() {
        let devices = vec![hdmi_device("hdmi-1", "DELL U3219Q")];
        let config = Config {
            version: 1,
            auto_switch: true,
            preferred_device: DeviceSelectorConfig {
                uid: None,
                monitor_name: Some("DELL U3219Q".into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            sony_speaker: None,
        };
        assert_eq!(
            preferred_uid_from_config(Some(&config), &devices).as_deref(),
            Some("hdmi-1")
        );
    }

    #[test]
    fn test_pick_device_index_by_number() {
        let devices = vec![
            device("Built-in Output", None, false),
            device("HDMI", Some("TV"), true),
        ];
        assert_eq!(
            pick_device_index(&devices, Some(1), None).unwrap(),
            PickSelection::Selected(1)
        );
    }

    #[test]
    fn test_pick_device_index_out_of_range() {
        let devices = vec![device("HDMI", Some("TV"), true)];
        assert!(pick_device_index(&devices, Some(3), None).is_err());
    }

    #[test]
    fn test_pick_device_index_empty_list() {
        assert!(pick_device_index(&[], None, None).is_err());
    }

    #[test]
    fn test_append_picker_note() {
        let label = append_picker_note(
            "   External Headphones — built-in".into(),
            Some("Sony: standby"),
        );
        assert_eq!(label, "   External Headphones — built-in — Sony: standby");
    }

    #[test]
    fn test_volume_for_preferred_pick_matches() {
        let devices = vec![hdmi_device("hdmi-1", "DELL U3219Q")];
        let config = Config {
            version: 1,
            auto_switch: true,
            preferred_device: DeviceSelectorConfig {
                uid: None,
                monitor_name: Some("DELL U3219Q".into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: Some(13),
            sony_speaker: None,
        };
        assert_eq!(
            volume_for_preferred_pick(Some(&config), &devices, "hdmi-1"),
            Some(13)
        );
        assert_eq!(
            volume_for_preferred_pick(Some(&config), &devices, "builtin"),
            None
        );
    }

    #[test]
    fn test_pick_and_switch_sets_volume_for_preferred() {
        let hal = MockHal::new(vec![
            hdmi_device("builtin", "Built-in"),
            hdmi_device("hdmi-1", "DELL U3219Q"),
        ])
        .with_default("builtin");

        pick_and_switch(
            &hal,
            &hal.list_outputs().unwrap().devices,
            1,
            true,
            Some(42),
        )
        .unwrap();
        assert_eq!(
            hal.volume_calls(),
            vec![crate::coreaudio::mock::SetVolumeCall {
                uid: "hdmi-1".into(),
                percent: 42,
            }]
        );
    }
}
