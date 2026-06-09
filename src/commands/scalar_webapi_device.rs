//! `rusty-jack scalar-webapi-device` — ScalarWebAPI speaker helpers.

use crate::config::{
    load_config, load_config_optional, render_lexicographic_json, resolve_config_path,
    ScalarWebApiDeviceConfig,
};
use crate::list_fmt;
use crate::scalar_webapi_device::{
    discover_scalar_webapi_devices_on_lan, enrich_scalar_webapi_mac_address,
    fetch_system_mac_address_for_api, refresh_scalar_webapi_discovery_cache,
    DiscoveredScalarWebApiDevice,
};
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

const DEFAULT_DISCOVER_TIMEOUT_MS: u64 = 3_000;

/// Scan the LAN for ScalarWebAPI-compatible speakers.
pub fn discover(
    json: bool,
    timeout_ms: Option<u64>,
    update_config: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    let resolved = resolve_config_path(config_path);
    let explicit = config_path.is_some();
    let config_path = resolved.as_deref();
    let config = if let Some(path) = config_path {
        load_config_optional(path, explicit)?
    } else {
        None
    };

    let timeout_ms = timeout_ms.unwrap_or_else(|| {
        config
            .as_ref()
            .and_then(|cfg| cfg.scalar_webapi_device.as_ref())
            .map(|api| api.request_timeout_ms)
            .unwrap_or(DEFAULT_DISCOVER_TIMEOUT_MS)
    });

    if !json {
        eprintln!("Discovering ScalarWebAPI speakers on your local network...");
    }

    let mut discovered = discover_scalar_webapi_devices_on_lan(timeout_ms)?;
    let mut configured_host = None;
    let mut configured_model = None;
    let mut configured_discovery = None;
    let mut configured_mac_address = config
        .as_ref()
        .and_then(|cfg| cfg.scalar_webapi_device.as_ref())
        .and_then(|api| api.mac_address.clone());

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
        if configured_mac_address.is_none() {
            configured_mac_address = fetch_system_mac_address_for_api(api).ok().flatten();
        }
    }

    if update_config {
        let path = config_path.context(
            "no config path — use --config or set RUSTY_JACK_CONFIG to update mac_address",
        )?;
        let mut config = load_config(path).map_err(anyhow::Error::new)?;
        let Some(api) = config.scalar_webapi_device.as_mut() else {
            anyhow::bail!("config has no scalar_webapi_device block to update");
        };
        if enrich_scalar_webapi_mac_address(api).map_err(anyhow::Error::new)? {
            write_scalar_webapi_mac_address(path, api)?;
            if !json {
                println!(
                    "Updated config mac_address: {}",
                    api.mac_address.as_deref().unwrap_or("(none)")
                );
            }
            configured_mac_address = api.mac_address.clone();
        } else if !json {
            println!("No mac_address update (already set or device unreachable).");
        }
    }

    if json {
        #[derive(Serialize)]
        struct DiscoverJsonOutput {
            scalar_webapi_discovered: Vec<DiscoveredScalarWebApiDevice>,
            #[serde(skip_serializing_if = "Option::is_none")]
            configured_host: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            configured_mac_address: Option<String>,
        }
        let value = DiscoverJsonOutput {
            scalar_webapi_discovered: discovered,
            configured_host,
            configured_mac_address,
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        if discovered.is_empty() {
            println!("No ScalarWebAPI speakers discovered.");
        } else {
            println!("Discovered ScalarWebAPI speakers:");
            for device in &discovered {
                let label = device
                    .model
                    .as_deref()
                    .filter(|model| !model.is_empty())
                    .unwrap_or("ScalarWebAPI speaker");
                println!("  {label} at {}", device.host);
            }
        }
        if let Some(mac) = configured_mac_address.as_deref() {
            println!();
            println!("Configured device MAC (Wake-on-LAN): {mac}");
        }
        let configured_hardware_model = configured_discovery
            .as_ref()
            .and_then(|hit| hit.model.as_deref().or(configured_model.as_deref()));
        if !discovered.is_empty() || configured_host.is_some() {
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

fn write_scalar_webapi_mac_address(
    path: &Path,
    api: &ScalarWebApiDeviceConfig,
) -> Result<(), crate::RustyJackError> {
    let raw = std::fs::read_to_string(path).map_err(crate::RustyJackError::Io)?;
    let mut value: Value = serde_json::from_str(&raw)
        .map_err(|err| crate::RustyJackError::Config(format!("{}: {err}", path.display())))?;
    value["scalar_webapi_device"]["mac_address"] = serde_json::json!(api.mac_address);
    let canonical = render_lexicographic_json(&value)?;
    crate::config::atomic_write_config(path, &canonical)
}
