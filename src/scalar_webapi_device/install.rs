//! Interactive ScalarWebAPI device setup during `rusty-jack install`.

use crate::config::ScalarWebApiDeviceConfig;
use crate::output_device::OutputDevice;
use crate::scalar_webapi_device::{
    discover_scalar_webapi_devices_on_lan, has_all_default_wake_triggers,
    DiscoveredScalarWebApiDevice, DEFAULT_WAKE_TRIGGERS, KEYBOARD_TRIGGER, MOUSE_TRIGGER,
    OUTPUT_SELECTED_TRIGGER,
};
use crate::transport::TransportKind;
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

const INSTALL_DISCOVERY_TIMEOUT_MS: u64 = 3_000;

/// ScalarWebAPI block written during install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarWebApiInstallSelection {
    pub host: String,
    pub model: String,
    pub mac_output_uid: String,
    pub mac_output_name: String,
    pub triggers: Vec<String>,
}

/// Ask whether to configure ScalarWebAPI speaker wake during install.
pub fn prompt_add_scalar_webapi_device(
    devices: &[OutputDevice],
    default_mac_output: &OutputDevice,
) -> Result<Option<ScalarWebApiInstallSelection>, RustyJackError> {
    let Some((host, model)) = prompt_scalar_webapi_host_selection(None, None, true)? else {
        return Ok(None);
    };

    let mac_output = prompt_mac_output_for_scalar_webapi(devices, default_mac_output)?;
    let triggers = prompt_wake_triggers()?;

    Ok(Some(ScalarWebApiInstallSelection {
        host,
        model,
        mac_output_uid: mac_output.uid.clone(),
        mac_output_name: mac_output.name.clone(),
        triggers,
    }))
}

/// Scan the LAN and pick a ScalarWebAPI host, optionally confirming a configured device.
pub fn prompt_scalar_webapi_host_selection(
    current_host: Option<&str>,
    current_model: Option<&str>,
    allow_skip: bool,
) -> Result<Option<(String, String)>, RustyJackError> {
    println!();
    println!("{}", style("ScalarWebAPI").cyan());
    let discovered = discover_scalar_webapi_devices_for_install()?;
    print_configured_scalar_webapi_host_status(current_host, &discovered);
    prompt_scalar_webapi_host_from_discovery(&discovered, current_host, current_model, allow_skip)
}

fn discover_scalar_webapi_devices_for_install(
) -> Result<Vec<DiscoveredScalarWebApiDevice>, RustyJackError> {
    println!(
        "{}",
        style("Searching the local network for ScalarWebAPI devices...").dim()
    );
    let discovered = discover_scalar_webapi_devices_on_lan(INSTALL_DISCOVERY_TIMEOUT_MS)?;
    match discovered.len() {
        0 => println!(
            "{}",
            style("No ScalarWebAPI devices found on the local network.").dim()
        ),
        1 => {
            let device = &discovered[0];
            println!(
                "  {} {}",
                style("found:").dim(),
                style(format_discovered_device(device)).green()
            );
        }
        count => {
            println!(
                "  {} {}",
                style("found:").dim(),
                style(format!("{count} ScalarWebAPI devices")).green()
            );
            for device in &discovered {
                println!("    {}", style(format_discovered_device(device)).green());
            }
        }
    }
    Ok(discovered)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarHostSelection {
    KeepConfigured,
    Discovered(usize),
    Manual,
    Skip,
}

fn print_configured_scalar_webapi_host_status(
    current_host: Option<&str>,
    discovered: &[DiscoveredScalarWebApiDevice],
) {
    let Some(host) = current_host.map(str::trim).filter(|host| !host.is_empty()) else {
        return;
    };
    if let Some(device) = discovered
        .iter()
        .find(|device| hosts_match(&device.host, host))
    {
        println!(
            "  {} {}",
            style("configured:").dim(),
            style(format!(
                "{} (found on LAN)",
                format_discovered_device(device)
            ))
            .green()
        );
        return;
    }
    println!(
        "  {} {}",
        style("configured:").dim(),
        style(format!("{host} (not seen in LAN scan)")).yellow()
    );
}

fn prompt_scalar_webapi_host_from_discovery(
    discovered: &[DiscoveredScalarWebApiDevice],
    current_host: Option<&str>,
    current_model: Option<&str>,
    allow_skip: bool,
) -> Result<Option<(String, String)>, RustyJackError> {
    let current = current_host.map(str::trim).filter(|host| !host.is_empty());
    if current.is_none() {
        return match discovered.len() {
            0 => prompt_manual_scalar_webapi_host(),
            1 => prompt_single_discovered_scalar_webapi_host(&discovered[0]),
            _ => prompt_multiple_discovered_scalar_webapi_hosts(discovered, allow_skip),
        };
    }

    let current = current.expect("current host checked above");
    let current_model = current_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or("ScalarWebAPI device")
        .to_string();
    let mut options: Vec<(ScalarHostSelection, String)> = Vec::new();
    let configured_found = discovered
        .iter()
        .find(|device| hosts_match(&device.host, current));
    if let Some(device) = configured_found {
        options.push((
            ScalarHostSelection::KeepConfigured,
            format!(
                "Keep configured: {} (found on LAN)",
                format_discovered_device(device)
            ),
        ));
    } else {
        options.push((
            ScalarHostSelection::KeepConfigured,
            format!("Keep configured: {current} (not seen in LAN scan)"),
        ));
    }
    for (index, device) in discovered
        .iter()
        .enumerate()
        .filter(|(_, device)| !hosts_match(&device.host, current))
    {
        options.push((
            ScalarHostSelection::Discovered(index),
            format_discovered_device(device),
        ));
    }
    options.push((ScalarHostSelection::Manual, "Enter host manually".into()));
    if allow_skip {
        options.push((ScalarHostSelection::Skip, "Skip ScalarWebAPI setup".into()));
    }

    if options.len() == 1 {
        return Ok(Some((current.to_string(), current_model)));
    }

    let default = 0;
    let labels = options
        .iter()
        .map(|(_, label)| label.as_str())
        .collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt(
            style(concat!(
                "Which ScalarWebAPI device should Rusty Jack wake?\n",
                "Pick the configured host, a discovered device, or enter a host manually."
            ))
            .cyan()
            .to_string(),
        )
        .items(&labels)
        .default(default)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("ScalarWebAPI prompt failed: {err}")))?;

    match options[selection].0 {
        ScalarHostSelection::KeepConfigured => Ok(Some((
            current.to_string(),
            configured_found
                .map(discovered_model_label)
                .unwrap_or(current_model),
        ))),
        ScalarHostSelection::Discovered(index) => Ok(Some((
            discovered[index].host.clone(),
            discovered_model_label(&discovered[index]),
        ))),
        ScalarHostSelection::Manual => {
            let (host, model) = prompt_manual_scalar_webapi_host_entry()?;
            Ok(Some((host, model)))
        }
        ScalarHostSelection::Skip => Ok(None),
    }
}

fn hosts_match(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn prompt_single_discovered_scalar_webapi_host(
    device: &DiscoveredScalarWebApiDevice,
) -> Result<Option<(String, String)>, RustyJackError> {
    if !Confirm::new()
        .with_prompt(style(format!(
            "Configure speaker wake for {}?\n\
             Rusty Jack can wake this device when you unlock your Mac and/or select the connected output.",
            format_discovered_device(device)
        ))
        .cyan()
        .to_string())
        .default(true)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("ScalarWebAPI prompt failed: {err}")))?
    {
        return Ok(None);
    }

    Ok(Some((device.host.clone(), discovered_model_label(device))))
}

fn prompt_multiple_discovered_scalar_webapi_hosts(
    discovered: &[DiscoveredScalarWebApiDevice],
    allow_skip: bool,
) -> Result<Option<(String, String)>, RustyJackError> {
    let mut labels = discovered
        .iter()
        .map(format_discovered_device)
        .collect::<Vec<_>>();
    labels.push("Enter host manually".into());
    if allow_skip {
        labels.push("Skip ScalarWebAPI setup".into());
    }

    let selection = Select::new()
        .with_prompt(
            style(if allow_skip {
                concat!(
                    "Which ScalarWebAPI device should Rusty Jack wake?\n",
                    "Pick a discovered device, enter a host manually, or skip setup."
                )
            } else {
                concat!(
                    "Which ScalarWebAPI device should Rusty Jack wake?\n",
                    "Pick a discovered device or enter a host manually."
                )
            })
            .cyan()
            .to_string(),
        )
        .items(&labels)
        .default(0)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("ScalarWebAPI prompt failed: {err}")))?;

    if selection < discovered.len() {
        let device = &discovered[selection];
        return Ok(Some((device.host.clone(), discovered_model_label(device))));
    }
    if selection == discovered.len() {
        return prompt_manual_scalar_webapi_host_entry().map(Some);
    }
    if allow_skip && selection == discovered.len() + 1 {
        return Ok(None);
    }
    Ok(None)
}

fn prompt_manual_scalar_webapi_host() -> Result<Option<(String, String)>, RustyJackError> {
    if !Confirm::new()
        .with_prompt(style(concat!(
            "Configure ScalarWebAPI speaker wake manually?\n",
            "This lets Rusty Jack wake a ScalarWebAPI-compatible speaker on your LAN when you unlock your Mac and/or select the output."
        ))
        .cyan()
        .to_string())
        .default(false)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("ScalarWebAPI prompt failed: {err}")))?
    {
        return Ok(None);
    }

    prompt_manual_scalar_webapi_host_entry().map(Some)
}

fn prompt_manual_scalar_webapi_host_entry() -> Result<(String, String), RustyJackError> {
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

    Ok((host.trim().to_string(), "ScalarWebAPI device".into()))
}

fn format_discovered_device(device: &DiscoveredScalarWebApiDevice) -> String {
    let label = discovered_model_label(device);
    format!("{label} at {}", device.host)
}

fn discovered_model_label(device: &DiscoveredScalarWebApiDevice) -> String {
    device
        .model
        .as_deref()
        .filter(|model| !model.is_empty())
        .unwrap_or("ScalarWebAPI device")
        .to_string()
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
        model: selection.model.clone(),
        host: Some(selection.host.clone()),
        port: 10_000,
        path: concat!("so", "ny").to_string(),
        mac_output: crate::config::DeviceSelectorConfig {
            name: Some(selection.mac_output_name.clone()),
            uid: Some(selection.mac_output_uid.clone()),
        },
        triggers: selection.triggers.clone(),
        wake_debounce_ms: 5_000,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarMacConnectionKind {
    HdmiDisplayPort,
    AnalogPort,
    Usb,
    AllOutputs,
}

fn prompt_mac_output_for_scalar_webapi(
    devices: &[OutputDevice],
    default_mac_output: &OutputDevice,
) -> Result<OutputDevice, RustyJackError> {
    let hdmi_displayport = filter_hdmi_displayport_outputs(devices);
    let analog_ports = filter_analog_port_outputs(devices);
    let usb_outputs = filter_usb_outputs(devices);

    let connection_kind = prompt_scalar_mac_connection_kind(
        hdmi_displayport.len(),
        analog_ports.len(),
        usb_outputs.len(),
    )?;

    let candidates = match connection_kind {
        ScalarMacConnectionKind::HdmiDisplayPort => hdmi_displayport,
        ScalarMacConnectionKind::AnalogPort => analog_ports,
        ScalarMacConnectionKind::Usb => usb_outputs,
        ScalarMacConnectionKind::AllOutputs => selectable_outputs(devices),
    };

    if candidates.is_empty() {
        return Err(RustyJackError::Config(
            "no Mac output matches the selected ScalarWebAPI connection type".into(),
        ));
    }
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }

    let default_index = candidates
        .iter()
        .position(|device| device.uid == default_mac_output.uid)
        .unwrap_or(0);
    let labels = candidates
        .iter()
        .map(crate::setup::setup_device_label)
        .collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt(
            style(concat!(
                "Which Mac output is connected to the ScalarWebAPI speaker?\n",
                "Pick the output that should trigger wake."
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

fn prompt_scalar_mac_connection_kind(
    hdmi_displayport_count: usize,
    analog_port_count: usize,
    usb_count: usize,
) -> Result<ScalarMacConnectionKind, RustyJackError> {
    let mut options = Vec::new();
    if hdmi_displayport_count > 0 {
        options.push((
            ScalarMacConnectionKind::HdmiDisplayPort,
            "HDMI or DisplayPort output",
        ));
    }
    if analog_port_count > 0 {
        options.push((
            ScalarMacConnectionKind::AnalogPort,
            "Headphone / line-out port",
        ));
    }
    if usb_count > 0 {
        options.push((ScalarMacConnectionKind::Usb, "USB audio device"));
    }
    options.push((
        ScalarMacConnectionKind::AllOutputs,
        "Not sure — show all outputs",
    ));

    if options.len() == 1 {
        return Ok(options[0].0);
    }

    let default = options
        .iter()
        .position(|(kind, _)| *kind == ScalarMacConnectionKind::HdmiDisplayPort)
        .or_else(|| {
            options
                .iter()
                .position(|(kind, _)| *kind == ScalarMacConnectionKind::AnalogPort)
        })
        .unwrap_or(0);
    let labels = options.iter().map(|(_, label)| *label).collect::<Vec<_>>();
    let selection = Select::new()
        .with_prompt(
            style(concat!(
                "How is the ScalarWebAPI speaker connected to this Mac?\n",
                "Choose the physical connection type, then pick the matching output."
            ))
            .cyan()
            .to_string(),
        )
        .items(&labels)
        .default(default)
        .interact()
        .map_err(|err| {
            RustyJackError::Config(format!("ScalarWebAPI connection prompt failed: {err}"))
        })?;
    Ok(options[selection].0)
}

fn selectable_outputs(devices: &[OutputDevice]) -> Vec<OutputDevice> {
    devices
        .iter()
        .filter(|device| device.is_alive && device.is_selectable())
        .cloned()
        .collect()
}

fn filter_hdmi_displayport_outputs(devices: &[OutputDevice]) -> Vec<OutputDevice> {
    devices
        .iter()
        .filter(|device| {
            device.is_alive
                && device.is_selectable()
                && matches!(
                    device.transport,
                    TransportKind::Hdmi | TransportKind::DisplayPort
                )
        })
        .cloned()
        .collect()
}

fn filter_analog_port_outputs(devices: &[OutputDevice]) -> Vec<OutputDevice> {
    devices
        .iter()
        .filter(|device| {
            device.is_alive
                && device.is_selectable()
                && device.transport == TransportKind::BuiltIn
                && !device.is_internal_builtin_output()
        })
        .cloned()
        .collect()
}

fn filter_usb_outputs(devices: &[OutputDevice]) -> Vec<OutputDevice> {
    devices
        .iter()
        .filter(|device| {
            device.is_alive && device.is_selectable() && device.transport == TransportKind::Usb
        })
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn device(uid: &str, name: &str, transport: TransportKind) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: name.into(),
            transport,
            is_alive: true,
            is_default: false,
            is_active: false,
        }
    }

    #[test]
    fn test_filter_hdmi_displayport_outputs() {
        let devices = vec![
            device("hdmi", "HDMI", TransportKind::Hdmi),
            device("dp", "DisplayPort", TransportKind::DisplayPort),
            device("hp", "External Headphones", TransportKind::BuiltIn),
        ];
        let filtered = filter_hdmi_displayport_outputs(&devices);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|device| {
            matches!(
                device.transport,
                TransportKind::Hdmi | TransportKind::DisplayPort
            )
        }));
    }

    #[test]
    fn test_filter_analog_port_outputs_excludes_internal_speakers() {
        let devices = vec![
            device("speakers", "Mac mini Speakers", TransportKind::BuiltIn),
            device("hp", "External Headphones", TransportKind::BuiltIn),
        ];
        let filtered = filter_analog_port_outputs(&devices);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].uid, "hp");
    }

    #[test]
    fn test_hosts_match_is_case_insensitive() {
        assert!(hosts_match("192.168.86.18", "192.168.86.18"));
        assert!(hosts_match(" Speaker.Local ", "speaker.local"));
        assert!(!hosts_match("192.168.86.18", "192.168.86.19"));
    }

    #[test]
    fn test_format_discovered_device_includes_model_and_host() {
        let device = DiscoveredScalarWebApiDevice {
            host: "192.168.1.42".into(),
            model: Some("SRS-ZR5".into()),
        };
        assert_eq!(format_discovered_device(&device), "SRS-ZR5 at 192.168.1.42");
    }
}
