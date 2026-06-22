//! `rusty-jack list` — enumerate output devices.

use crate::config::{load_config_optional, resolve_config_path};
use crate::coreaudio::AudioHal;
use crate::list_fmt;
use crate::output_device::filter_hdmi_devices;
use crate::scalar_webapi_device::{
    attach_scalar_webapi_mac_output, discover_scalar_webapi_devices_on_lan_with_feedback,
    refresh_scalar_webapi_discovery_cache, should_show_distinct_speaker_model,
    DiscoveredScalarWebApiDevice, ScalarDiscoveryFeedback,
};
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

/// List output devices, optionally filtered to HDMI-class transports.
pub fn run(
    hal: &dyn AudioHal,
    hdmi_only: bool,
    json: bool,
    discover: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    let resolved = resolve_config_path(config_path);
    let explicit = config_path.is_some();
    let config = if let Some(path) = resolved.as_deref() {
        load_config_optional(path, explicit)?
    } else {
        None
    };

    let mut list = hal.list_outputs()?;
    list = attach_scalar_webapi_mac_output(list, config.as_ref());

    if hdmi_only {
        list.devices = filter_hdmi_devices(&list.devices);
    }

    let mut scalar_webapi_discovered: Option<Vec<DiscoveredScalarWebApiDevice>> = None;
    let mut configured_discovery: Option<DiscoveredScalarWebApiDevice> = None;
    let mut configured_model: Option<String> = None;
    let mut configured_host: Option<String> = None;
    if discover {
        eprintln!("Discovering ScalarWebAPI speakers on your local network...");
        let timeout_ms = config
            .as_ref()
            .and_then(|cfg| cfg.scalar_webapi_device.as_ref())
            .map(|api| api.request_timeout_ms)
            .unwrap_or(3_000);
        let feedback = if json {
            ScalarDiscoveryFeedback::Silent
        } else {
            ScalarDiscoveryFeedback::Interactive
        };
        let mut discovered =
            discover_scalar_webapi_devices_on_lan_with_feedback(timeout_ms, feedback)?;
        if let Some(api) = config
            .as_ref()
            .and_then(|cfg| cfg.scalar_webapi_device.as_ref())
            .filter(|api| api.enabled)
        {
            configured_model = Some(api.model.clone());
            configured_host = api.host.clone();
            configured_discovery = refresh_scalar_webapi_discovery_cache(api)?;
            if let Some(configured_hit) = configured_discovery.as_ref() {
                if !discovered.iter().any(|hit| hit.host == configured_hit.host) {
                    discovered.push(configured_hit.clone());
                }
            }
        }
        scalar_webapi_discovered = Some(discovered);
    }

    if json {
        if let Some(discovered) = scalar_webapi_discovered {
            #[derive(Serialize)]
            struct ListJsonOutput {
                #[serde(flatten)]
                list: crate::system_default::DeviceList,
                scalar_webapi_discovered: Vec<DiscoveredScalarWebApiDevice>,
            }
            let value = ListJsonOutput {
                list,
                scalar_webapi_discovered: discovered,
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            list_fmt::print_json(&list)?;
        }
    } else {
        list_fmt::print_table(&list, hdmi_only)?;
        if let Some(discovered) = scalar_webapi_discovered.as_ref() {
            let configured_hardware_model = configured_discovery.as_ref().and_then(|hit| {
                let hardware = hit.model.as_deref()?;
                let show = configured_model.as_deref().is_none_or(|config_model| {
                    should_show_distinct_speaker_model(config_model, hardware)
                });
                show.then_some(hardware)
            });
            println!();
            println!(
                "{}",
                list_fmt::format_scalar_webapi_discovery_footer(
                    discovered.len(),
                    configured_host.as_deref(),
                    configured_hardware_model,
                )
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coreaudio::mock::MockHal;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn fixture_devices() -> Vec<OutputDevice> {
        vec![
            OutputDevice {
                id: 1,
                uid: "builtin".into(),
                name: "Speakers".into(),
                transport: TransportKind::BuiltIn,
                is_alive: true,
                is_default: true,
                is_active: true,
            },
            OutputDevice {
                id: 2,
                uid: "hdmi-uid".into(),
                name: "Monitor".into(),
                transport: TransportKind::Hdmi,
                is_alive: true,
                is_default: false,
                is_active: false,
            },
        ]
    }

    #[test]
    fn test_list_hdmi_filter_via_mock() {
        let hal = MockHal::new(fixture_devices());
        let all = hal.list_outputs().unwrap().devices;
        let hdmi = filter_hdmi_devices(&all);
        assert_eq!(hdmi.len(), 1);
        assert_eq!(hdmi[0].uid, "hdmi-uid");
    }

    #[test]
    fn test_run_json_does_not_panic() {
        let hal = MockHal::new(fixture_devices());
        run(&hal, false, true, false, None).unwrap();
    }
}
