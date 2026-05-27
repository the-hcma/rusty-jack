//! `rusty-jack daemon` — background supervisor loop.

use crate::activity::PlatformActivityMonitor;
use crate::config::{load_config, resolve_config_path};
use crate::coreaudio::AudioHal;
use crate::device_select::resolve_device_selector;
use anyhow::{Context, Result};
use std::path::Path;

/// Run the background policy supervisor.
pub fn run(hal: &dyn AudioHal, config_path: Option<&Path>) -> Result<()> {
    let path = resolve_config_path(config_path)
        .context("no config path — use --config or ~/.config/rusty-jack/config.json")?;
    let config = load_config(&path).map_err(anyhow::Error::new)?;

    // Detect common config mismatches early so they show up on daemon restarts.
    if let Ok(list) = hal.list_outputs() {
        let preferred_selector = config.preferred_selector();
        if let Ok(uid) = resolve_device_selector(&preferred_selector, &list.devices) {
            if let Some(device) = list.devices.iter().find(|d| d.uid == uid) {
                let resolved_label = device.friendly_label();
                let stored = config.preferred_device.name.as_deref();
                if stored.is_some_and(|name| name != resolved_label) {
                    eprintln!(
                        "warning: config preferred_device.name is `{}` but connected device is `{}` ({})",
                        stored.unwrap_or("(missing)"),
                        resolved_label,
                        uid
                    );
                    eprintln!(
                        "warning: consider re-running `rusty-jack install` to refresh stored device names"
                    );
                }
            }
        }

        if let Some(api) = config.scalar_webapi_device.as_ref().filter(|api| api.enabled) {
            let selector = api.mac_output.clone().into();
            if let Ok(uid) = resolve_device_selector(&selector, &list.devices) {
                if let Some(device) = list.devices.iter().find(|d| d.uid == uid) {
                    let resolved_label = device.friendly_label();
                    let stored = api.mac_output.name.as_deref();
                    if stored.is_some_and(|name| name != resolved_label) {
                        eprintln!(
                            "warning: config scalar_webapi_device.mac_output.name is `{}` but connected device is `{}` ({})",
                            stored.unwrap_or("(missing)"),
                            resolved_label,
                            uid
                        );
                        eprintln!(
                            "warning: consider re-running `rusty-jack install` to refresh stored device names"
                        );
                    }
                }
            }
        }
    }

    println!(
        "rusty-jack daemon running (poll={}ms, activity_poll={}ms)",
        config.poll_interval_ms, config.activity_poll_interval_ms
    );

    let activity = PlatformActivityMonitor;
    crate::daemon::run_forever(hal, &path, &activity).map_err(anyhow::Error::new)
}
