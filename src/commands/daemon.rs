//! `rusty-jack daemon` — background supervisor loop.

use crate::activity::PlatformActivityMonitor;
use crate::config::{load_config, resolve_config_path};
use crate::coreaudio::AudioHal;
use crate::device_select::resolve_device_selector;
use crate::logging::{init_daemon, DaemonLoggingOptions};
use anyhow::{Context, Result};
use std::path::Path;

/// Run the background policy supervisor.
pub fn run(hal: &dyn AudioHal, config_path: Option<&Path>) -> Result<()> {
    let path = resolve_config_path(config_path)
        .context("no config path — use --config or ~/.config/rusty-jack/config.json")?;
    let config = load_config(&path).map_err(anyhow::Error::new)?;

    let logging_options = DaemonLoggingOptions::from(&config.logging);
    let log_path =
        crate::logging::resolve_log_file_path(&logging_options.file).map_err(anyhow::Error::new)?;
    eprintln!("rusty-jack daemon: writing logs to {}", log_path.display());
    init_daemon(&logging_options).map_err(anyhow::Error::new)?;

    // Detect common config mismatches early so they show up on daemon restarts.
    if let Ok(list) = hal.list_outputs() {
        let preferred_selector = config.preferred_selector();
        if let Ok(uid) = resolve_device_selector(&preferred_selector, &list.devices) {
            if let Some(device) = list.devices.iter().find(|d| d.uid == uid) {
                let resolved_label = device.friendly_label();
                let stored = config.preferred_device.name.as_deref();
                if stored.is_some_and(|name| name != resolved_label) {
                    tracing::warn!(
                        target: "daemon",
                        "[config] preferred_device.name={stored:?} connected={resolved_label} uid={uid}; consider re-running `rusty-jack install` to refresh stored device names"
                    );
                }
            }
        }

        if let Some(api) = config
            .scalar_webapi_device
            .as_ref()
            .filter(|api| api.enabled)
        {
            let selector = api.mac_output.clone().into();
            if let Ok(uid) = resolve_device_selector(&selector, &list.devices) {
                if let Some(device) = list.devices.iter().find(|d| d.uid == uid) {
                    let resolved_label = device.friendly_label();
                    let stored = api.mac_output.name.as_deref();
                    if stored.is_some_and(|name| name != resolved_label) {
                        tracing::warn!(
                            target: "daemon",
                            "[config] scalar_webapi_device.mac_output.name={stored:?} connected={resolved_label} uid={uid}; consider re-running `rusty-jack install` to refresh stored device names"
                        );
                    }
                }
            }
        }
    }

    tracing::info!(
        target: "daemon",
        "[daemon] started poll={}ms activity_poll={}ms",
        config.poll_interval_ms,
        config.activity_poll_interval_ms
    );

    let activity = PlatformActivityMonitor;
    crate::daemon::run_forever(hal, &path, &activity).map_err(anyhow::Error::new)
}
