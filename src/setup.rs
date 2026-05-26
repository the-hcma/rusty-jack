//! First-run setup helpers for config creation and cleanup.

use crate::config::{default_config_path, render_lexicographic_json};
use crate::coreaudio::AudioHal;
use crate::output_device::OutputDevice;
use crate::RustyJackError;
use dialoguer::{Confirm, Select};
use serde::Serialize;
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
    },
    Kept {
        config_path: String,
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
    if path.exists() && !should_replace_existing_config(&path, interactive)? {
        return Ok(ConfigSetupResult::Kept {
            config_path: path_display(&path)?,
        });
    }

    let list = hal.list_outputs()?;
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
    let config = render_config_json(preferred, fallback, volume)?;

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
        }
        ConfigSetupResult::Kept { config_path } => {
            println!("Config already exists");
            println!("  path: {config_path}");
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

fn should_replace_existing_config(path: &Path, interactive: bool) -> Result<bool, RustyJackError> {
    if !path.exists() {
        return Ok(true);
    }
    if !interactive {
        return Ok(false);
    }
    Confirm::new()
        .with_prompt(format!(
            "Config already exists at {}. Replace it?",
            path.display()
        ))
        .default(false)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("config prompt failed: {err}")))
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

fn setup_device_label(device: &OutputDevice) -> String {
    let active = if device.is_active { "active, " } else { "" };
    format!(
        "{} — {}{} — {}",
        device.friendly_label(),
        active,
        device.transport,
        device.uid
    )
}

pub(crate) fn render_config_json(
    preferred: &OutputDevice,
    fallback: Option<&OutputDevice>,
    volume: Option<u8>,
) -> Result<String, RustyJackError> {
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
            "uid": preferred.uid,
        },
        "fallback_uids": fallback_uids,
        "also_set_system_output": true,
    });
    if let Some(volume) = volume {
        value["volume"] = serde_json::json!(volume);
    }
    render_lexicographic_json(&value)
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

    fn device(uid: &str, name: &str, transport: TransportKind, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: name.into(),
            transport,
            is_alive: true,
            is_default: active,
            is_active: active,
            monitor_name: None,
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
}
