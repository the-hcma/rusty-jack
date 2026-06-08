//! First-run setup helpers for config creation and cleanup.

use crate::config::{default_config_path, is_placeholder_uid, render_lexicographic_json};
use crate::coreaudio::AudioHal;
use crate::output_device::OutputDevice;
use crate::scalar_webapi_device::{
    append_scalar_webapi_to_config_json, format_scalar_webapi_triggers_for_display,
    maybe_prompt_scalar_webapi_wake_triggers, prompt_add_scalar_webapi_device,
    prompt_scalar_webapi_host_selection,
};
use crate::RustyJackError;
use dialoguer::console::style;
use dialoguer::{Confirm, MultiSelect, Select};
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
        #[serde(skip_serializing_if = "Option::is_none")]
        scalar_webapi_mac_output_label: Option<String>,
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
    ensure_config_at_path(&path, hal, interactive)
}

/// Ensure a config exists at the given path, prompting for devices when possible.
pub fn ensure_config_at_path(
    path: &Path,
    hal: &dyn AudioHal,
    interactive: bool,
) -> Result<ConfigSetupResult, RustyJackError> {
    let list = hal.list_outputs()?;
    if path.exists() {
        return reconfigure_or_update_existing_config(path, hal, &list.devices, interactive);
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
    std::fs::write(path, config).map_err(RustyJackError::Io)?;

    let scalar_webapi_mac_output_label = scalar_webapi.as_ref().and_then(|selection| {
        list.devices
            .iter()
            .find(|device| device.uid == selection.mac_output_uid)
            .map(OutputDevice::friendly_label)
            .or_else(|| Some(selection.mac_output_name.clone()))
    });
    Ok(ConfigSetupResult::Created {
        config_path: path_display(path)?,
        preferred_uid: preferred.uid.clone(),
        preferred_label: preferred.friendly_label(),
        fallback_uid: fallback.map(|device| device.uid.clone()),
        fallback_label: fallback.map(OutputDevice::friendly_label),
        volume,
        scalar_webapi_triggers: scalar_webapi.map(|selection| selection.triggers),
        scalar_webapi_mac_output_label,
    })
}

fn reconfigure_or_update_existing_config(
    path: &Path,
    hal: &dyn AudioHal,
    devices: &[OutputDevice],
    interactive: bool,
) -> Result<ConfigSetupResult, RustyJackError> {
    let raw = std::fs::read_to_string(path).map_err(RustyJackError::Io)?;
    let mut value = serde_json::from_str::<Value>(&raw)
        .map_err(|err| RustyJackError::Config(format!("{}: {err}", path.display())))?;

    // Keep existing migration behavior (refresh names, offer ScalarWebAPI trigger upgrade, etc.)
    let migrated = update_existing_config_value(&mut value, devices, interactive)?;

    if interactive {
        println!("{}", style("Current config").cyan());
        println!(
            "  {} {}",
            style("path:").dim(),
            style(path_display(path)?).green()
        );
        print_existing_config_summary(&value, devices);

        if Confirm::new()
            .with_prompt(q(concat!(
                "Reconfigure this existing config?\n",
                "If you say yes, Rusty Jack will save a backup, then re-ask the key choices and default to the current values."
            )))
            .default(false)
            .interact()
            .map_err(|err| RustyJackError::Config(format!("reconfigure prompt failed: {err}")))?
        {
            let backup_path = backup_config_for_reconfigure(path, &raw)?;
            println!(
                "  {} {}",
                style("backup:").dim(),
                style(path_display(&backup_path)?).green()
            );
            return reconfigure_existing_config(path, hal, devices, value);
        }
    }

    // If migration produced changes, persist them.
    if let Some(migrated) = migrated {
        std::fs::write(path, render_lexicographic_json(&value)?).map_err(RustyJackError::Io)?;
        return Ok(ConfigSetupResult::Updated {
            config_path: path_display(path)?,
            changes: migrated,
        });
    }

    // Otherwise canonicalize whitespace/key order without reporting it as an update.
    let canonical = render_lexicographic_json(&value)?;
    if canonical != raw {
        std::fs::write(path, canonical).map_err(RustyJackError::Io)?;
    }
    Ok(ConfigSetupResult::Kept {
        config_path: path_display(path)?,
    })
}

fn reconfigure_existing_config(
    path: &Path,
    hal: &dyn AudioHal,
    devices: &[OutputDevice],
    value: Value,
) -> Result<ConfigSetupResult, RustyJackError> {
    loop {
        let mut updated = value.clone();

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum ReconfigureSection {
            Preferred,
            Volume,
            Fallback,
            ScalarWebApi,
        }

        let sections = [
            (ReconfigureSection::Preferred, "preferred output"),
            (ReconfigureSection::Volume, "volume"),
            (ReconfigureSection::Fallback, "fallback output"),
            (
                ReconfigureSection::ScalarWebApi,
                "ScalarWebAPI speaker wake",
            ),
        ];
        let defaults = [false, false, false, true];

        let selected = MultiSelect::new()
            .with_prompt(q(concat!(
                "What would you like to reconfigure?\n",
                "Select one or more sections (space to toggle, enter to confirm)."
            )))
            .items(&sections.iter().map(|(_, label)| *label).collect::<Vec<_>>())
            .defaults(&defaults)
            .interact()
            .map_err(|err| {
                RustyJackError::Config(format!("reconfigure sections prompt failed: {err}"))
            })?;

        let selected = selected
            .into_iter()
            .filter_map(|idx| sections.get(idx).map(|(section, _)| *section))
            .collect::<Vec<_>>();

        let reconfigure_preferred = selected.contains(&ReconfigureSection::Preferred);
        let reconfigure_volume = selected.contains(&ReconfigureSection::Volume);
        let reconfigure_fallback = selected.contains(&ReconfigureSection::Fallback);
        let reconfigure_scalar = selected.contains(&ReconfigureSection::ScalarWebApi);

        // Resolve current preferred device (may be needed as a default for ScalarWebAPI prompts).
        let default_preferred_uid = preferred_uid_from_value(&updated);
        let preferred_index = if reconfigure_preferred || default_preferred_uid.is_none() {
            prompt_for_preferred_device_with_default_uid(devices, default_preferred_uid.as_deref())?
        } else {
            devices
                .iter()
                .position(|device| Some(device.uid.as_str()) == default_preferred_uid.as_deref())
                .unwrap_or_else(|| prompt_for_preferred_device(devices).unwrap_or(0))
        };
        let preferred = &devices[preferred_index];

        let mut changes = Vec::new();

        // Preferred device.
        if reconfigure_preferred || default_preferred_uid.is_none() {
            // We only write preferred_device when we're explicitly reconfiguring it (or it's missing).
            set_device_selector(
                &mut updated,
                "preferred_device",
                preferred,
                "preferred device",
                &mut changes,
            );
        }

        // Volume (keep existing if present, otherwise use current device volume if readable).
        if reconfigure_volume {
            let volume = updated
                .get("volume")
                .and_then(Value::as_u64)
                .and_then(|v| u8::try_from(v).ok())
                .or_else(|| hal.output_volume_percent(&preferred.uid));
            if let Some(volume) = volume {
                if updated.get("volume").and_then(Value::as_u64) != Some(volume as u64) {
                    updated["volume"] = serde_json::json!(volume);
                    changes.push(format!("set volume to {volume}%"));
                }
            }
        }

        // Fallback device.
        if reconfigure_fallback {
            let default_fallback_uid = updated
                .get("fallback_uids")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .map(str::to_string);
            let fallback_index = prompt_for_fallback_device_with_default_uid(
                devices,
                &preferred.uid,
                default_fallback_uid.as_deref(),
            )?;
            match fallback_index {
                Some(index) => {
                    let uid = devices[index].uid.clone();
                    updated["fallback_uids"] = serde_json::json!([uid.clone()]);
                    changes.push(format!(
                        "set fallback device to `{}`",
                        devices[index].friendly_label()
                    ));
                }
                None => {
                    // Explicitly clear fallback_uids to allow implicit builtin fallback behavior.
                    if !fallback_uids_empty(&updated) {
                        updated["fallback_uids"] = serde_json::json!([]);
                        changes.push("cleared explicit fallback".into());
                    }
                }
            }
        }

        // ScalarWebAPI configuration.
        let scalar_webapi_enabled = updated
            .pointer("/scalar_webapi_device/enabled")
            .and_then(Value::as_bool)
            == Some(true);
        if reconfigure_scalar
            && scalar_webapi_enabled
            && Confirm::new()
                .with_prompt(q(
                    "Reconfigure ScalarWebAPI settings (host, Mac output, triggers)?",
                ))
                .default(false)
                .interact()
                .map_err(|err| {
                    RustyJackError::Config(format!("ScalarWebAPI reconfigure prompt failed: {err}"))
                })?
        {
            let current_host = updated
                .pointer("/scalar_webapi_device/host")
                .and_then(Value::as_str)
                .map(str::to_string);
            let current_model = updated
                .pointer("/scalar_webapi_device/model")
                .and_then(Value::as_str)
                .map(str::to_string);
            let Some((host, model)) = prompt_scalar_webapi_host_selection(
                current_host.as_deref(),
                current_model.as_deref(),
                false,
            )?
            else {
                continue;
            };
            if current_host.as_deref() != Some(host.as_str()) {
                updated["scalar_webapi_device"]["host"] = serde_json::json!(host);
                changes.push("updated ScalarWebAPI host".into());
            }
            if current_model.as_deref() != Some(model.as_str()) {
                updated["scalar_webapi_device"]["model"] = serde_json::json!(model);
                changes.push("updated ScalarWebAPI model".into());
            }

            // Mac output selector (avoid asking twice when it's identical to preferred).
            let current_mac_uid = updated
                .pointer("/scalar_webapi_device/mac_output/uid")
                .and_then(Value::as_str)
                .map(str::to_string);
            let mac_uid_matches_preferred =
                current_mac_uid.as_deref() == Some(preferred.uid.as_str());
            let use_preferred_for_scalar = mac_uid_matches_preferred
                || Confirm::new()
                    .with_prompt(q(concat!(
                        "Use the preferred output as the ScalarWebAPI Mac output?\n",
                        "This is usually correct when the speaker is connected to the preferred output."
                    )))
                    .default(true)
                    .interact()
                    .map_err(|err| {
                        RustyJackError::Config(format!(
                            "ScalarWebAPI Mac output prompt failed: {err}"
                        ))
                    })?;

            let mac_output_uid = if use_preferred_for_scalar {
                preferred.uid.clone()
            } else {
                let mac_output_index = prompt_for_preferred_device_with_default_uid(
                    devices,
                    current_mac_uid.as_deref(),
                )?;
                devices[mac_output_index].uid.clone()
            };
            ensure_nested_device_selector(
                &mut updated,
                &["scalar_webapi_device", "mac_output"],
                &mac_output_uid,
                devices,
                "ScalarWebAPI Mac output",
                &mut changes,
            );

            // Triggers: always ask in reconfigure flow.
            let current_triggers = updated
                .pointer("/scalar_webapi_device/triggers")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let triggers =
                crate::scalar_webapi_device::prompt_scalar_webapi_wake_triggers(&current_triggers)?;
            updated["scalar_webapi_device"]["triggers"] = serde_json::json!(triggers);
            changes.push("updated ScalarWebAPI wake triggers".into());
        } else if !scalar_webapi_enabled
            && reconfigure_scalar
            && Confirm::new()
                .with_prompt(q(
                    "Configure ScalarWebAPI speaker wake for this Mac output?",
                ))
                .default(false)
                .interact()
                .map_err(|err| {
                    RustyJackError::Config(format!("ScalarWebAPI add prompt failed: {err}"))
                })?
        {
            let selection = prompt_add_scalar_webapi_device(devices, preferred)?;
            if let Some(selection) = selection {
                append_scalar_webapi_to_config_json(&mut updated, &selection);
                changes.push("added ScalarWebAPI configuration".into());
            }
        }

        let diff = summarize_config_diff(&value, &updated, devices);
        if diff.is_empty() {
            println!("{}", style("No changes.").dim());
            return Ok(ConfigSetupResult::Kept {
                config_path: path_display(path)?,
            });
        }

        println!();
        println!("{}", style("Proposed config changes").cyan());
        for line in &diff {
            println!("  {}", style(line).green());
        }
        println!();

        let apply = Confirm::new()
            .with_prompt(q("Apply these changes to the config file?"))
            .default(true)
            .interact()
            .map_err(|err| {
                RustyJackError::Config(format!("confirm config changes failed: {err}"))
            })?;
        if apply {
            std::fs::write(path, render_lexicographic_json(&updated)?)
                .map_err(RustyJackError::Io)?;
            return Ok(ConfigSetupResult::Updated {
                config_path: path_display(path)?,
                changes: diff,
            });
        }

        let abandon = Confirm::new()
            .with_prompt(q("Abandon changes and keep the current config?"))
            .default(true)
            .interact()
            .map_err(|err| RustyJackError::Config(format!("abandon prompt failed: {err}")))?;
        if abandon {
            return Ok(ConfigSetupResult::Kept {
                config_path: path_display(path)?,
            });
        }

        // Otherwise loop and go over again.
        println!();
        println!("{}", style("OK. Let's go over the options again.").cyan());
    }
}

fn summarize_config_diff(before: &Value, after: &Value, devices: &[OutputDevice]) -> Vec<String> {
    let mut lines = Vec::new();

    let label_for_uid = |uid: Option<&str>| -> String {
        let Some(uid) = uid else {
            return "(none)".into();
        };
        devices
            .iter()
            .find(|device| device.uid == uid)
            .map(|device| device.friendly_label())
            .unwrap_or_else(|| uid.to_string())
    };

    let before_preferred = before
        .pointer("/preferred_device/uid")
        .and_then(Value::as_str);
    let after_preferred = after
        .pointer("/preferred_device/uid")
        .and_then(Value::as_str);
    if before_preferred != after_preferred {
        lines.push(format!(
            "preferred: {} -> {}",
            label_for_uid(before_preferred),
            label_for_uid(after_preferred)
        ));
    }

    let before_vol = before.get("volume").and_then(Value::as_u64);
    let after_vol = after.get("volume").and_then(Value::as_u64);
    if before_vol != after_vol {
        lines.push(format!(
            "volume: {} -> {}",
            before_vol
                .map(|v| format!("{v}%"))
                .unwrap_or("(unset)".into()),
            after_vol
                .map(|v| format!("{v}%"))
                .unwrap_or("(unset)".into())
        ));
    }

    let before_fb = before
        .get("fallback_uids")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str);
    let after_fb = after
        .get("fallback_uids")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str);
    if before_fb != after_fb {
        lines.push(format!(
            "fallback: {} -> {}",
            if before_fb.is_some() {
                label_for_uid(before_fb)
            } else {
                "(implicit builtin)".into()
            },
            if after_fb.is_some() {
                label_for_uid(after_fb)
            } else {
                "(implicit builtin)".into()
            }
        ));
    }

    let before_scalar_enabled = before
        .pointer("/scalar_webapi_device/enabled")
        .and_then(Value::as_bool)
        == Some(true);
    let after_scalar_enabled = after
        .pointer("/scalar_webapi_device/enabled")
        .and_then(Value::as_bool)
        == Some(true);
    if before_scalar_enabled != after_scalar_enabled {
        lines.push(format!(
            "ScalarWebAPI: {} -> {}",
            if before_scalar_enabled {
                "enabled"
            } else {
                "disabled"
            },
            if after_scalar_enabled {
                "enabled"
            } else {
                "disabled"
            }
        ));
    }

    let before_host = before
        .pointer("/scalar_webapi_device/host")
        .and_then(Value::as_str);
    let after_host = after
        .pointer("/scalar_webapi_device/host")
        .and_then(Value::as_str);
    if before_host != after_host {
        lines.push(format!(
            "ScalarWebAPI host: {} -> {}",
            before_host.unwrap_or("(unset)"),
            after_host.unwrap_or("(unset)")
        ));
    }

    let before_triggers = scalar_webapi_triggers_from_value(before);
    let after_triggers = scalar_webapi_triggers_from_value(after);
    if before_triggers != after_triggers {
        lines.push(format!(
            "ScalarWebAPI triggers: {} -> {}",
            format_scalar_webapi_triggers_for_display(
                &before_triggers,
                scalar_webapi_mac_output_label(before, devices).as_deref(),
            ),
            format_scalar_webapi_triggers_for_display(
                &after_triggers,
                scalar_webapi_mac_output_label(after, devices).as_deref(),
            ),
        ));
    }

    lines
}

fn scalar_webapi_triggers_from_value(value: &Value) -> Vec<String> {
    value
        .pointer("/scalar_webapi_device/triggers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn scalar_webapi_mac_output_label(value: &Value, devices: &[OutputDevice]) -> Option<String> {
    if let Some(name) = value
        .pointer("/scalar_webapi_device/mac_output/name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    {
        let uid = value
            .pointer("/scalar_webapi_device/mac_output/uid")
            .and_then(Value::as_str);
        if let Some(uid) = uid {
            if let Some(device) = devices.iter().find(|device| device.uid == uid) {
                return Some(device.friendly_label());
            }
        }
        return Some(name.to_string());
    }
    value
        .pointer("/scalar_webapi_device/mac_output/uid")
        .and_then(Value::as_str)
        .and_then(|uid| {
            devices
                .iter()
                .find(|device| device.uid == uid)
                .map(OutputDevice::friendly_label)
        })
}

fn print_existing_config_summary(value: &Value, devices: &[OutputDevice]) {
    // Preferred device.
    let preferred_uid = preferred_uid_from_value(value);
    let preferred_label = preferred_uid
        .as_deref()
        .and_then(|uid| devices.iter().find(|d| d.uid == uid))
        .map(|d| d.friendly_label())
        .or_else(|| preferred_uid.as_deref().map(|uid| uid.to_string()))
        .unwrap_or_else(|| "(not set)".into());
    println!(
        "  {} {}",
        style("preferred:").dim(),
        style(preferred_label).green()
    );

    // Volume.
    let volume = value.get("volume").and_then(Value::as_u64);
    if let Some(volume) = volume {
        println!(
            "  {} {}",
            style("volume:").dim(),
            style(format!("{volume}%")).green()
        );
    }

    // ScalarWebAPI.
    let scalar_enabled = value
        .pointer("/scalar_webapi_device/enabled")
        .and_then(Value::as_bool)
        == Some(true);
    if scalar_enabled {
        let mac_output_uid = value
            .pointer("/scalar_webapi_device/mac_output/uid")
            .and_then(Value::as_str);
        let mac_output_label = mac_output_uid
            .and_then(|uid| {
                devices
                    .iter()
                    .find(|d| d.uid == uid)
                    .map(|d| d.friendly_label())
            })
            .or_else(|| mac_output_uid.map(|uid| uid.to_string()))
            .unwrap_or_else(|| "(not set)".into());
        let host = value
            .pointer("/scalar_webapi_device/host")
            .and_then(Value::as_str)
            .unwrap_or("(missing host)");
        let triggers = format_scalar_webapi_triggers_for_display(
            &scalar_webapi_triggers_from_value(value),
            scalar_webapi_mac_output_label(value, devices).as_deref(),
        );
        println!(
            "  {} {}",
            style("ScalarWebAPI Mac output:").dim(),
            style(mac_output_label).green()
        );
        println!(
            "  {}",
            style("This Mac output should be physically connected to the ScalarWebAPI speaker.")
                .dim()
        );
        println!(
            "  {} {}",
            style("ScalarWebAPI host:").dim(),
            style(host).green()
        );
        println!(
            "  {} {}",
            style("ScalarWebAPI triggers:").dim(),
            style(triggers).green()
        );
    }
}

fn update_existing_config_value(
    value: &mut Value,
    devices: &[OutputDevice],
    interactive: bool,
) -> Result<Option<Vec<String>>, RustyJackError> {
    let mut changes = Vec::new();

    let preferred_uid = preferred_uid_from_value(value);
    if let Some(uid) = preferred_uid
        .as_deref()
        .filter(|uid| !is_placeholder_uid(uid))
        .map(str::to_string)
    {
        ensure_device_selector(value, "preferred_device", &uid, devices, &mut changes);
    } else if interactive {
        let preferred_index = prompt_for_preferred_device(devices)?;
        let preferred = &devices[preferred_index];
        set_device_selector(
            value,
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
            value,
            &["scalar_webapi_device", "mac_output"],
            &uid,
            devices,
            "ScalarWebAPI Mac output",
            &mut changes,
        );
    }

    if interactive {
        if let Some(triggers) = maybe_prompt_scalar_webapi_wake_triggers(value)? {
            value["scalar_webapi_device"]["triggers"] = serde_json::json!(triggers);
            changes.push("updated ScalarWebAPI wake triggers".into());
        }
    }

    Ok((!changes.is_empty()).then_some(changes))
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
            scalar_webapi_mac_output_label,
            ..
        } => {
            println!("{}", style("Created config").cyan());
            println!("  {} {}", style("path:").dim(), style(config_path).green());
            println!(
                "  {} {}",
                style("preferred:").dim(),
                style(preferred_label).green()
            );
            println!(
                "  {} {}",
                style("fallback:").dim(),
                style(
                    fallback_label
                        .as_deref()
                        .unwrap_or(IMPLICIT_BUILTIN_FALLBACK_LABEL)
                )
                .green()
            );
            if let Some(volume) = volume {
                println!(
                    "  {} {}",
                    style("volume:").dim(),
                    style(format!("{volume}%")).green()
                );
            }
            if let Some(triggers) = scalar_webapi_triggers {
                println!(
                    "  {} {}",
                    style("ScalarWebAPI triggers:").dim(),
                    style(format_scalar_webapi_triggers_for_display(
                        triggers,
                        scalar_webapi_mac_output_label.as_deref(),
                    ))
                    .green()
                );
            }
        }
        ConfigSetupResult::Kept { config_path } => {
            println!("{}", style("Config already exists").cyan());
            println!("  {} {}", style("path:").dim(), style(config_path).green());
        }
        ConfigSetupResult::Updated {
            config_path,
            changes,
        } => {
            println!("{}", style("Updated config").cyan());
            println!("  {} {}", style("path:").dim(), style(config_path).green());
            for change in changes {
                println!("  {} {}", style("changed:").dim(), style(change).green());
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

fn q(prompt: impl AsRef<str>) -> String {
    style(prompt.as_ref()).cyan().to_string()
}

fn prompt_for_preferred_device_with_default_uid(
    devices: &[OutputDevice],
    default_uid: Option<&str>,
) -> Result<usize, RustyJackError> {
    let candidates = selectable_alive_indices(devices);
    if candidates.is_empty() {
        return Err(RustyJackError::Config(
            "no selectable output device found".into(),
        ));
    }
    let default = default_uid
        .and_then(|uid| {
            candidates
                .iter()
                .position(|index| devices[*index].uid == uid)
        })
        .unwrap_or_else(|| {
            default_preferred_device_index(devices)
                .and_then(|index| candidates.iter().position(|candidate| *candidate == index))
                .unwrap_or(0)
        });
    let labels = candidates
        .iter()
        .map(|index| setup_device_label(&devices[*index]))
        .collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt(q("Pick the preferred output device"))
        .items(&labels)
        .default(default)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("preferred device prompt failed: {err}")))?;
    Ok(candidates[selection])
}

fn prompt_for_fallback_device_with_default_uid(
    devices: &[OutputDevice],
    preferred_uid: &str,
    default_uid: Option<&str>,
) -> Result<Option<usize>, RustyJackError> {
    let mut choices = selectable_alive_indices(devices)
        .into_iter()
        .filter(|index| devices[*index].uid != preferred_uid)
        .map(Some)
        .collect::<Vec<_>>();
    choices.push(None);

    let default = default_uid
        .and_then(|uid| {
            choices
                .iter()
                .position(|choice| choice.map(|index| devices[index].uid.as_str()) == Some(uid))
        })
        .unwrap_or_else(|| {
            default_fallback_device_index(devices, preferred_uid)
                .and_then(|index| choices.iter().position(|choice| *choice == Some(index)))
                .unwrap_or(choices.len() - 1)
        });

    let labels = choices
        .iter()
        .map(|choice| match choice {
            Some(index) => setup_device_label(&devices[*index]),
            None => format!("Use {IMPLICIT_BUILTIN_FALLBACK_LABEL}"),
        })
        .collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt(q("Pick a fallback output device"))
        .items(&labels)
        .default(default)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("fallback device prompt failed: {err}")))?;
    Ok(choices[selection])
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
        .with_prompt(q("Pick the preferred output device"))
        .items(&labels)
        .default(default)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("preferred device prompt failed: {err}")))?;
    Ok(candidates[selection])
}

// Legacy helper (kept only for tests that validate config migrations).
#[cfg(test)]
fn update_existing_config(
    path: &Path,
    devices: &[OutputDevice],
    interactive: bool,
) -> Result<ConfigSetupResult, RustyJackError> {
    let raw = std::fs::read_to_string(path).map_err(RustyJackError::Io)?;
    let mut value = serde_json::from_str::<Value>(&raw)
        .map_err(|err| RustyJackError::Config(format!("{}: {err}", path.display())))?;
    let changes = update_existing_config_value(&mut value, devices, interactive)?;

    if let Some(changes) = changes {
        std::fs::write(path, render_lexicographic_json(&value)?).map_err(RustyJackError::Io)?;
        return Ok(ConfigSetupResult::Updated {
            config_path: path_display(path)?,
            changes,
        });
    }

    let canonical = render_lexicographic_json(&value)?;
    if canonical != raw {
        std::fs::write(path, canonical).map_err(RustyJackError::Io)?;
    }

    Ok(ConfigSetupResult::Kept {
        config_path: path_display(path)?,
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

#[allow(dead_code)]
fn prompt_add_fallback() -> Result<bool, RustyJackError> {
    Confirm::new()
        .with_prompt(q("Add an explicit fallback output to the existing config?"))
        .default(false)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("fallback prompt failed: {err}")))
}

// NOTE: fallback prompting moved into the interactive reconfigure flow.

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
        .with_prompt(q("Pick a fallback output device"))
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

fn backup_config_for_reconfigure(path: &Path, raw: &str) -> Result<PathBuf, RustyJackError> {
    let parent = path.parent().ok_or_else(|| {
        RustyJackError::Config(format!("config path has no parent: {}", path.display()))
    })?;
    let backup_dir = parent.join("config-backups");
    std::fs::create_dir_all(&backup_dir).map_err(RustyJackError::Io)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| RustyJackError::Config(format!("system clock before UNIX epoch: {err}")))?;
    let backup_path = backup_dir.join(format!(
        "config-{}.{:09}.json",
        timestamp.as_secs(),
        timestamp.subsec_nanos()
    ));
    std::fs::write(&backup_path, raw).map_err(RustyJackError::Io)?;
    Ok(backup_path)
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
    fn test_backup_config_for_reconfigure_writes_timestamped_copy() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(&config_path, r#"{"version":1}"#).unwrap();

        let backup_path = backup_config_for_reconfigure(&config_path, r#"{"version":1}"#).unwrap();

        assert!(backup_path.starts_with(dir.path().join("config-backups")));
        assert!(backup_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("config-") && name.ends_with(".json")));
        assert_eq!(
            std::fs::read_to_string(&backup_path).unwrap(),
            r#"{"version":1}"#
        );
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
