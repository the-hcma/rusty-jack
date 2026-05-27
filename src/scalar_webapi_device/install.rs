//! Interactive ScalarWebAPI device setup during `rusty-jack install`.

use crate::config::ScalarWebApiDeviceConfig;
use crate::output_device::OutputDevice;
use crate::scalar_webapi_device::{
    has_all_default_wake_triggers, DEFAULT_WAKE_TRIGGERS, KEYBOARD_TRIGGER, MOUSE_TRIGGER,
    OUTPUT_SELECTED_TRIGGER,
};
use crate::RustyJackError;
use dialoguer::console::style;
use dialoguer::{Confirm, Input, MultiSelect, Select};
use serde_json::Value;

const TRIGGER_LABELS: &[(&str, &str)] = &[
    (
        KEYBOARD_TRIGGER,
        "keyboard — wake on unlock / keyboard activity",
    ),
    (MOUSE_TRIGGER, "mouse — wake on unlock / pointer activity"),
    (
        OUTPUT_SELECTED_TRIGGER,
        "output_selected — wake when the Mac output is selected",
    ),
];

/// ScalarWebAPI block written during install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarWebApiInstallSelection {
    pub host: String,
    pub mac_output_uid: String,
    pub mac_output_name: String,
    pub triggers: Vec<String>,
}

/// Ask whether to configure ScalarWebAPI speaker wake during install.
pub fn prompt_add_scalar_webapi_device(
    devices: &[OutputDevice],
    default_mac_output: &OutputDevice,
) -> Result<Option<ScalarWebApiInstallSelection>, RustyJackError> {
    if !Confirm::new()
        .with_prompt(style(concat!(
            "Configure ScalarWebAPI speaker wake (for example, a Sony speaker on your LAN).\n",
            "This lets Rusty Jack wake the speaker when you unlock your Mac and/or select the output.\n",
            "Configure it now?"
        ))
        .cyan()
        .to_string())
        .default(true)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("ScalarWebAPI prompt failed: {err}")))?
    {
        return Ok(None);
    }

    println!();
    println!("{}", style("ScalarWebAPI").cyan());
    println!(
        "{}",
        style("Enter the device host (IP address or hostname). Example: 192.168.1.42").dim()
    );
    let host: String = Input::new()
        .with_prompt(style("Device host").cyan().to_string())
        .validate_with(|input: &String| {
            if input.trim().is_empty() {
                Err("host is required")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(|err| RustyJackError::Config(format!("ScalarWebAPI host prompt failed: {err}")))?;

    let mac_output = prompt_mac_output(devices, default_mac_output)?;
    let triggers = prompt_wake_triggers()?;

    Ok(Some(ScalarWebApiInstallSelection {
        host: host.trim().to_string(),
        mac_output_uid: mac_output.uid.clone(),
        mac_output_name: mac_output.name.clone(),
        triggers,
    }))
}

/// Offer to add missing recommended wake triggers on an existing config.
pub fn maybe_prompt_scalar_webapi_wake_triggers(
    value: &Value,
) -> Result<Option<Vec<String>>, RustyJackError> {
    let Some(triggers) = value
        .pointer("/scalar_webapi_device/triggers")
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    let triggers = triggers
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if value
        .pointer("/scalar_webapi_device/enabled")
        .and_then(Value::as_bool)
        != Some(true)
        || has_all_default_wake_triggers(&triggers)
    {
        return Ok(None);
    }

    if Confirm::new()
        .with_prompt(
            style(concat!(
                "ScalarWebAPI wake is missing unlock/activity triggers.\n",
                "Enable all recommended triggers (keyboard, mouse, and output selection)?"
            ))
            .cyan()
            .to_string(),
        )
        .default(true)
        .interact()
        .map_err(|err| {
            RustyJackError::Config(format!("ScalarWebAPI trigger prompt failed: {err}"))
        })?
    {
        return Ok(Some(default_wake_triggers()));
    }

    Ok(Some(prompt_wake_triggers_with_defaults(&triggers)?))
}

/// Prompt for ScalarWebAPI wake triggers using `current` as the defaults.
///
/// This is used by the install reconfigure flow to explicitly revisit triggers.
pub fn prompt_scalar_webapi_wake_triggers(
    current: &[String],
) -> Result<Vec<String>, RustyJackError> {
    prompt_wake_triggers_with_defaults(current)
}

pub fn scalar_webapi_install_to_config(
    selection: &ScalarWebApiInstallSelection,
) -> ScalarWebApiDeviceConfig {
    ScalarWebApiDeviceConfig {
        enabled: true,
        model: "ScalarWebAPI device".into(),
        host: Some(selection.host.clone()),
        port: 10_000,
        path: concat!("so", "ny").to_string(),
        mac_output: crate::config::DeviceSelectorConfig {
            name: Some(selection.mac_output_name.clone()),
            uid: Some(selection.mac_output_uid.clone()),
        },
        triggers: selection.triggers.clone(),
        wake_debounce_ms: 30_000,
        request_timeout_ms: 3_000,
        require_quick_start: true,
    }
}

pub fn append_scalar_webapi_to_config_json(
    value: &mut Value,
    selection: &ScalarWebApiInstallSelection,
) {
    let api = scalar_webapi_install_to_config(selection);
    value["scalar_webapi_device"] = serde_json::json!({
        "enabled": api.enabled,
        "model": api.model,
        "host": api.host,
        "mac_output": {
            "name": api.mac_output.name,
            "uid": api.mac_output.uid,
        },
        "triggers": api.triggers,
        "wake_debounce_ms": api.wake_debounce_ms,
        "request_timeout_ms": api.request_timeout_ms,
        "require_quick_start": api.require_quick_start,
    });
}

fn prompt_mac_output(
    devices: &[OutputDevice],
    default_mac_output: &OutputDevice,
) -> Result<OutputDevice, RustyJackError> {
    let candidates = devices
        .iter()
        .filter(|device| device.is_alive && device.is_selectable())
        .collect::<Vec<_>>();
    if candidates.len() <= 1 {
        return Ok(default_mac_output.clone());
    }

    let default_index = candidates
        .iter()
        .position(|device| device.uid == default_mac_output.uid)
        .unwrap_or(0);
    let labels = candidates
        .iter()
        .map(|device| crate::setup::setup_device_label(device))
        .collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt(
            style(concat!(
                "Which Mac output is connected to the ScalarWebAPI speaker?\n",
                "Pick the Mac output that should trigger wake."
            ))
            .cyan()
            .to_string(),
        )
        .items(&labels)
        .default(default_index)
        .interact()
        .map_err(|err| {
            RustyJackError::Config(format!("ScalarWebAPI Mac output prompt failed: {err}"))
        })?;
    Ok(candidates[selection].clone())
}

fn prompt_wake_triggers() -> Result<Vec<String>, RustyJackError> {
    if Confirm::new()
        .with_prompt(
            style(concat!(
                "Use all recommended wake triggers.\n",
                "This includes unlock/activity (keyboard + mouse) and output selection."
            ))
            .cyan()
            .to_string(),
        )
        .default(true)
        .interact()
        .map_err(|err| {
            RustyJackError::Config(format!("ScalarWebAPI trigger confirm failed: {err}"))
        })?
    {
        return Ok(default_wake_triggers());
    }

    prompt_wake_triggers_with_defaults(&default_wake_triggers())
}

fn prompt_wake_triggers_with_defaults(current: &[String]) -> Result<Vec<String>, RustyJackError> {
    let labels = TRIGGER_LABELS
        .iter()
        .map(|(trigger, label)| format_toggle_label(label, current_contains(current, trigger)))
        .collect::<Vec<_>>();

    let defaults = TRIGGER_LABELS
        .iter()
        .map(|(trigger, _)| current_contains(current, trigger))
        .collect::<Vec<_>>();

    let selection = MultiSelect::new()
        .with_prompt(
            style(concat!(
                "Toggle wake triggers.\n",
                "Items marked with `*` are currently enabled."
            ))
            .cyan()
            .to_string(),
        )
        .items(&labels)
        .defaults(&defaults)
        .interact()
        .map_err(|err| {
            RustyJackError::Config(format!("ScalarWebAPI trigger selection failed: {err}"))
        })?;

    let triggers = selection
        .iter()
        .filter_map(|&index| {
            TRIGGER_LABELS
                .get(index)
                .map(|(trigger, _)| (*trigger).to_string())
        })
        .collect::<Vec<_>>();
    if triggers.is_empty() {
        return Err(RustyJackError::Config(
            "select at least one ScalarWebAPI wake trigger".into(),
        ));
    }
    Ok(triggers)
}

fn format_toggle_label(label: &str, enabled: bool) -> String {
    if enabled {
        format!("* {label} (currently enabled)")
    } else {
        format!("  {label}")
    }
}

fn current_contains(current: &[String], trigger: &str) -> bool {
    current
        .iter()
        .any(|value| value.eq_ignore_ascii_case(trigger))
}

fn default_wake_triggers() -> Vec<String> {
    DEFAULT_WAKE_TRIGGERS
        .iter()
        .map(|trigger| (*trigger).to_string())
        .collect()
}
