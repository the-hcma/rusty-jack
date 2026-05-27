//! First-run setup helpers for config creation and cleanup.

use crate::config::{default_config_path, is_placeholder_uid, render_lexicographic_json};
use crate::coreaudio::AudioHal;
use crate::output_device::OutputDevice;
use crate::scalar_webapi_device::{
    append_scalar_webapi_to_config_json, maybe_prompt_scalar_webapi_wake_triggers,
    prompt_add_scalar_webapi_device,
};
use crate::RustyJackError;
use dialoguer::{Confirm, Select};
use serde::Serialize;
use serde_json::Value;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

const IMPLICIT_BUILTIN_FALLBACK_LABEL: &str = "built-in output (automatic when available)";

/// Result of creating or preserving the default user config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConfigSetupResult {
    Created {
        config_path: String,
        preferred_uid: String,
        preferred_label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback_uid: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback_label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        volume: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        scalar_webapi_triggers: Option<Vec<String>>,
    },
    Kept {
        config_path: String,
    },
    Updated {
        config_path: String,
        changes: Vec<String>,
    },
}

/// Result of optionally removing the default user config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConfigRemovalResult {
    Removed { config_path: String },
    Kept { config_path: String },
    NotFound { config_path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigRemovalMode {
    Prompt,
    Remove,
    Keep,
}

/// Ensure the default config exists, prompting for devices when possible.
pub fn ensure_default_config(
    hal: &dyn AudioHal,
    interactive: bool,
) -> Result<ConfigSetupResult, RustyJackError> {
    let path = default_config_path_or_err()?;
    let list = hal.list_outputs()?;
    if path.exists() {
        return update_existing_config(&path, &list.devices, interactive);
    }

    let preferred_index = if interactive {
        prompt_for_preferred_device(&list.devices)?
    } else {
        default_preferred_device_index(&list.devices)
            .ok_or_else(|| RustyJackError::Config("no selectable output device found".into()))?
    };
    let preferred = &list.devices[preferred_index];
    let fallback_index = if interactive {
        prompt_for_fallback_device(&list.devices, &preferred.uid)?
    } else {
        default_fallback_device_index(&list.devices, &preferred.uid)
    };
    let fallback = fallback_index.map(|index| &list.devices[index]);
    let volume = hal.output_volume_percent(&preferred.uid);
    let scalar_webapi = if interactive {
        prompt_add_scalar_webapi_device(&list.devices, preferred)?
    } else {
        None
    };
    let mut value = render_config_value(preferred, fallback, volume)?;
    if let Some(selection) = scalar_webapi.as_ref() {
        append_scalar_webapi_to_config_json(&mut value, selection);
    }
    let config = render_lexicographic_json(&value)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    std::fs::write(&path, config).map_err(RustyJackError::Io)?;

    Ok(ConfigSetupResult::Created {
        config_path: path_display(&path)?,
        preferred_uid: preferred.uid.clone(),
        preferred_label: preferred.friendly_label(),
        fallback_uid: fallback.map(|device| device.uid.clone()),
        fallback_label: fallback.map(OutputDevice::friendly_label),
        volume,
        scalar_webapi_triggers: scalar_webapi.map(|selection| selection.triggers),
    })
}

/// Remove or preserve the default config according to uninstall options.
pub fn maybe_remove_default_config(
    mode: ConfigRemovalMode,
    interactive: bool,
) -> Result<ConfigRemovalResult, RustyJackError> {
    let path = default_config_path_or_err()?;
    if !path.exists() {
        return Ok(ConfigRemovalResult::NotFound {
            config_path: path_display(&path)?,
        });
    }

    let remove = match mode {
        ConfigRemovalMode::Remove => true,
        ConfigRemovalMode::Keep => false,
        ConfigRemovalMode::Prompt => {
            interactive
                && Confirm::new()
                    .with_prompt(format!("Remove config file {}?", path.display()))
                    .default(false)
                    .interact()
                    .map_err(|err| RustyJackError::Config(format!("config prompt failed: {err}")))?
        }
    };

    if remove {
        std::fs::remove_file(&path).map_err(RustyJackError::Io)?;
        Ok(ConfigRemovalResult::Removed {
            config_path: path_display(&path)?,
        })
    } else {
        Ok(ConfigRemovalResult::Kept {
            config_path: path_display(&path)?,
        })
    }
}

pub fn print_config_setup_result(result: &ConfigSetupResult) {
    match result {
        ConfigSetupResult::Created {
            config_path,
            preferred_label,
            fallback_label,
            volume,
            scalar_webapi_triggers,
            ..
        } => {
            println!("Created config");
            println!("  path:      {config_path}");
            println!("  preferred: {preferred_label}");
            println!(
                "  fallback:  {}",
                fallback_label
                    .as_deref()
                    .unwrap_or(IMPLICIT_BUILTIN_FALLBACK_LABEL)
            );
            if let Some(volume) = volume {
                println!("  volume:    {volume}%");
            }
            if let Some(triggers) = scalar_webapi_triggers {
                println!("  ScalarWebAPI triggers: {}", triggers.join(", "));
            }
        }
        ConfigSetupResult::Kept { config_path } => {
            println!("Config already exists");
            println!("  path: {config_path}");
        }
        ConfigSetupResult::Updated {
            config_path,
            changes,
        } => {
            println!("Updated config");
            println!("  path: {config_path}");
            for change in changes {
                println!("  changed: {change}");
            }
        }
    }
}

pub fn print_config_removal_result(result: &ConfigRemovalResult) {
    match result {
        ConfigRemovalResult::Removed { config_path } => {
            println!("Removed config");
            println!("  path: {config_path}");
        }
        ConfigRemovalResult::Kept { config_path } => {
            println!("Kept config");
            println!("  path: {config_path}");
        }
        ConfigRemovalResult::NotFound { config_path } => {
            println!("Config not found");
            println!("  expected: {config_path}");
        }
    }
}

#[must_use]
pub fn terminal_is_interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn prompt_for_preferred_device(devices: &[OutputDevice]) -> Result<usize, RustyJackError> {
    let candidates = selectable_alive_indices(devices);
    if candidates.is_empty() {
        return Err(RustyJackError::Config(
            "no selectable output device found".into(),
        ));
    }
    let default = default_preferred_device_index(devices)
        .and_then(|index| candidates.iter().position(|candidate| *candidate == index))
        .unwrap_or(0);
    let labels = candidates
        .iter()
        .map(|index| setup_device_label(&devices[*index]))
        .collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt("Pick the preferred output device")
        .items(&labels)
        .default(default)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("preferred device prompt failed: {err}")))?;
    Ok(candidates[selection])
}

fn update_existing_config(
    path: &Path,
    devices: &[OutputDevice],
    interactive: bool,
) -> Result<ConfigSetupResult, RustyJackError> {
    let raw = std::fs::read_to_string(path).map_err(RustyJackError::Io)?;
    let mut value = serde_json::from_str::<Value>(&raw)
        .map_err(|err| RustyJackError::Config(format!("{}: {err}", path.display())))?;
    let mut changes = Vec::new();

    let preferred_uid = preferred_uid_from_value(&value);
    if let Some(uid) = preferred_uid
        .as_deref()
        .filter(|uid| !is_placeholder_uid(uid))
        .map(str::to_string)
    {
        ensure_device_selector(&mut value, "preferred_device", &uid, devices, &mut changes);
    } else if interactive {
        let preferred_index = prompt_for_preferred_device(devices)?;
        let preferred = &devices[preferred_index];
        set_device_selector(
            &mut value,
            "preferred_device",
            preferred,
            "preferred device",
            &mut changes,
        );
    } else {
        return Err(RustyJackError::Config(
            "existing config is missing preferred_device.uid; rerun install interactively".into(),
        ));
    }

    if let Some(uid) = value
        .pointer("/scalar_webapi_device/mac_output/uid")
        .and_then(Value::as_str)
        .filter(|uid| !is_placeholder_uid(uid))
        .map(str::to_string)
    {
        ensure_nested_device_selector(
            &mut value,
            &["scalar_webapi_device", "mac_output"],
            &uid,
            devices,
            "ScalarWebAPI Mac output",
            &mut changes,
        );
    }

    if interactive {
        if let Some(triggers) = maybe_prompt_scalar_webapi_wake_triggers(&value)? {
            value["scalar_webapi_device"]["triggers"] = serde_json::json!(triggers);
            changes.push("updated ScalarWebAPI wake triggers".into());
        }
    }

    if interactive && fallback_uids_empty(&value) && prompt_add_fallback()? {
        let preferred_uid = preferred_uid_from_value(&value);
        if let Some(preferred_uid) = preferred_uid.as_deref() {
            if let Some(index) = prompt_for_fallback_device(devices, preferred_uid)? {
                value["fallback_uids"] = serde_json::json!([devices[index].uid]);
                changes.push(format!(
                    "added fallback device `{}`",
                    devices[index].friendly_label()
                ));
            }
        }
    }

    if changes.is_empty() {
        let canonical = render_lexicographic_json(&value)?;
        if canonical != raw {
            std::fs::write(path, canonical).map_err(RustyJackError::Io)?;
        }
        return Ok(ConfigSetupResult::Kept {
            config_path: path_display(path)?,
        });
    }

    std::fs::write(path, render_lexicographic_json(&value)?).map_err(RustyJackError::Io)?;
    Ok(ConfigSetupResult::Updated {
        config_path: path_display(path)?,
        changes,
    })
}

fn preferred_uid_from_value(value: &Value) -> Option<String> {
    value
        .pointer("/preferred_device/uid")
        .and_then(Value::as_str)
        .filter(|uid| !is_placeholder_uid(uid))
        .or_else(|| {
            value
                .get("preferred_device_uid")
                .and_then(Value::as_str)
                .filter(|uid| !is_placeholder_uid(uid))
        })
        .map(str::to_string)
}

fn fallback_uids_empty(value: &Value) -> bool {
    value
        .get("fallback_uids")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
}

fn prompt_add_fallback() -> Result<bool, RustyJackError> {
    Confirm::new()
        .with_prompt("Add an explicit fallback output to the existing config?")
        .default(false)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("fallback prompt failed: {err}")))
}

fn ensure_device_selector(
    value: &mut Value,
    key: &str,
    uid: &str,
    devices: &[OutputDevice],
    changes: &mut Vec<String>,
) {
    if let Some(device) = devices.iter().find(|device| device.uid == uid) {
        set_selector_fields(&mut value[key], device, "preferred device", changes);
    }
}

fn ensure_nested_device_selector(
    value: &mut Value,
    path: &[&str],
    uid: &str,
    devices: &[OutputDevice],
    label: &str,
    changes: &mut Vec<String>,
) {
    if let Some(device) = devices.iter().find(|device| device.uid == uid) {
        let mut current = value;
        for segment in path {
            current = &mut current[*segment];
        }
        set_selector_fields(current, device, label, changes);
    }
}

fn set_device_selector(
    value: &mut Value,
    key: &str,
    device: &OutputDevice,
    label: &str,
    changes: &mut Vec<String>,
) {
    set_selector_fields(&mut value[key], device, label, changes);
}

fn set_selector_fields(
    selector: &mut Value,
    device: &OutputDevice,
    label: &str,
    changes: &mut Vec<String>,
) {
    if !selector.is_object() {
        *selector = serde_json::json!({});
    }
    if selector.get("uid").and_then(Value::as_str) != Some(device.uid.as_str()) {
        selector["uid"] = Value::String(device.uid.clone());
        changes.push(format!("set {label} uid to `{}`", device.uid));
    }
    if selector.get("name").and_then(Value::as_str) != Some(device.name.as_str()) {
        selector["name"] = Value::String(device.name.clone());
        changes.push(format!("set {label} name to `{}`", device.name));
    }
}

fn prompt_for_fallback_device(
    devices: &[OutputDevice],
    preferred_uid: &str,
) -> Result<Option<usize>, RustyJackError> {
    let mut choices = selectable_alive_indices(devices)
        .into_iter()
        .filter(|index| devices[*index].uid != preferred_uid)
        .map(Some)
        .collect::<Vec<_>>();
    choices.push(None);

    let default_fallback = default_fallback_device_index(devices, preferred_uid);
    let default = default_fallback
        .and_then(|index| choices.iter().position(|choice| *choice == Some(index)))
        .unwrap_or(choices.len() - 1);
    let labels = choices
        .iter()
        .map(|choice| match choice {
            Some(index) => setup_device_label(&devices[*index]),
            None => format!("Use {IMPLICIT_BUILTIN_FALLBACK_LABEL}"),
        })
        .collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt("Pick a fallback output device")
        .items(&labels)
        .default(default)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("fallback device prompt failed: {err}")))?;
    Ok(choices[selection])
}

fn selectable_alive_indices(devices: &[OutputDevice]) -> Vec<usize> {
    devices
        .iter()
        .enumerate()
        .filter(|(_, device)| device.is_alive && device.is_selectable())
        .map(|(index, _)| index)
        .collect()
}

pub(crate) fn default_preferred_device_index(devices: &[OutputDevice]) -> Option<usize> {
    devices
        .iter()
        .position(|device| device.is_active && device.is_alive && device.is_selectable())
        .or_else(|| {
            devices
                .iter()
                .position(|device| device.is_alive && device.is_selectable())
        })
}

pub(crate) fn default_fallback_device_index(
    devices: &[OutputDevice],
    preferred_uid: &str,
) -> Option<usize> {
    devices
        .iter()
        .position(|device| device.uid != preferred_uid && device.is_internal_builtin_output())
        .or_else(|| {
            devices.iter().position(|device| {
                device.uid != preferred_uid && device.is_alive && device.is_selectable()
            })
        })
}

pub(crate) fn setup_device_label(device: &OutputDevice) -> String {
    let active = if device.is_active { "active, " } else { "" };
    format!(
        "{} — {}{} — {}",
        device.friendly_label(),
        active,
        device.transport,
        device.uid
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_config_json(
    preferred: &OutputDevice,
    fallback: Option<&OutputDevice>,
    volume: Option<u8>,
) -> Result<String, RustyJackError> {
    render_lexicographic_json(&render_config_value(preferred, fallback, volume)?)
}

fn render_config_value(
    preferred: &OutputDevice,
    fallback: Option<&OutputDevice>,
    volume: Option<u8>,
) -> Result<Value, RustyJackError> {
    let fallback_uids = fallback
        .map(|device| vec![device.uid.clone()])
        .unwrap_or_default();
    let mut value = serde_json::json!({
        "version": 1,
        "auto_switch": true,
        "poll_interval_ms": 2000,
        "switch_delay_ms": 500,
        "activity_idle_threshold_ms": 60000,
        "activity_poll_interval_ms": 1000,
        "preferred_device": {
            "name": preferred.name,
            "uid": preferred.uid,
        },
        "fallback_uids": fallback_uids,
        "also_set_system_output": true,
    });
    if let Some(volume) = volume {
        value["volume"] = serde_json::json!(volume);
    }
    Ok(value)
}

fn default_config_path_or_err() -> Result<PathBuf, RustyJackError> {
    default_config_path()
        .ok_or_else(|| RustyJackError::Config("HOME is not set; cannot locate config".into()))
}

fn path_display(path: &Path) -> Result<String, RustyJackError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| RustyJackError::Config("config path is not valid UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportKind;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn device(uid: &str, name: &str, transport: TransportKind, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: name.into(),
            transport,
            is_alive: true,
            is_default: active,
            is_active: active,
        }
    }

    #[test]
    fn test_default_fallback_prefers_internal_speakers() {
        let devices = vec![
            device(
                "headphones",
                "External Headphones",
                TransportKind::BuiltIn,
                true,
            ),
            device(
                "speakers",
                "Mac mini Speakers",
                TransportKind::BuiltIn,
                false,
            ),
            device("hdmi", "HDMI", TransportKind::Hdmi, false),
        ];

        assert_eq!(
            default_fallback_device_index(&devices, "headphones"),
            Some(1)
        );
    }

    #[test]
    fn test_default_preferred_uses_active_selectable_output() {
        let devices = vec![
            device(
                "speakers",
                "Mac mini Speakers",
                TransportKind::BuiltIn,
                false,
            ),
            device("hdmi", "HDMI", TransportKind::Hdmi, true),
        ];

        assert_eq!(default_preferred_device_index(&devices), Some(1));
    }

    #[test]
    fn test_render_config_json_includes_preferred_and_fallback() {
        let preferred = device("hdmi", "HDMI", TransportKind::Hdmi, true);
        let fallback = device(
            "speakers",
            "Mac mini Speakers",
            TransportKind::BuiltIn,
            false,
        );

        let json = render_config_json(&preferred, Some(&fallback), Some(13)).unwrap();

        assert!(json.contains("\"name\": \"HDMI\""));
        assert!(json.contains("\"uid\": \"hdmi\""));
        assert!(json.contains("\"speakers\""));
        assert!(json.contains("\"volume\": 13"));
        assert!(json.starts_with("{\n  \"activity_idle_threshold_ms\""));
        assert!(json.ends_with('\n'));
    }

    #[test]
    fn test_render_config_json_omits_unreadable_volume() {
        let preferred = device("hdmi", "HDMI", TransportKind::Hdmi, true);

        let json = render_config_json(&preferred, None, None).unwrap();

        assert!(!json.contains("\"volume\""));
    }

    #[test]
    fn test_update_existing_config_adds_names_without_dropping_settings() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "headphones" }},
  "scalar_webapi_device": {{
    "enabled": true,
    "host": "speaker.local",
    "mac_output": {{ "uid": "headphones" }},
    "triggers": ["output_selected"]
  }},
  "volume": 80
}}"#
        )
        .unwrap();
        let devices = vec![device(
            "headphones",
            "External Headphones",
            TransportKind::BuiltIn,
            true,
        )];

        let result = update_existing_config(file.path(), &devices, false).unwrap();

        assert!(matches!(result, ConfigSetupResult::Updated { .. }));
        let updated = std::fs::read_to_string(file.path()).unwrap();
        assert!(updated.contains(r#""name": "External Headphones""#));
        assert!(updated.contains(r#""host": "speaker.local""#));
        assert!(updated.contains(r#""volume": 80"#));
    }

    #[test]
    fn test_default_scalar_webapi_wake_triggers() {
        assert!(!crate::scalar_webapi_device::has_all_default_wake_triggers(
            &["output_selected".into()]
        ));
        assert!(crate::scalar_webapi_device::has_all_default_wake_triggers(
            &["keyboard".into(), "mouse".into(), "output_selected".into()]
        ));
    }

    #[test]
    fn test_update_existing_config_keeps_current_when_no_changes() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "preferred_device": {{
    "name": "External Headphones",
    "uid": "headphones"
  }},
  "version": 1
}}
"#
        )
        .unwrap();
        let devices = vec![device(
            "headphones",
            "External Headphones",
            TransportKind::BuiltIn,
            true,
        )];

        let result = update_existing_config(file.path(), &devices, false).unwrap();

        assert!(matches!(result, ConfigSetupResult::Kept { .. }));
    }
}
