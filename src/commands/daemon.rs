//! `rusty-jack daemon` — background supervisor loop.

use crate::activity::PlatformActivityMonitor;
use crate::config::{load_config, resolve_config_path};
use crate::coreaudio::AudioHal;
use anyhow::{Context, Result};
use std::path::Path;

/// Run the background policy supervisor.
pub fn run(hal: &dyn AudioHal, config_path: Option<&Path>) -> Result<()> {
    let path = resolve_config_path(config_path)
        .context("no config path — use --config or ~/.config/rusty-jack/config.json")?;
    let config = load_config(&path).map_err(anyhow::Error::new)?;

    println!(
        "rusty-jack daemon running (poll={}ms, activity_poll={}ms)",
        config.poll_interval_ms, config.activity_poll_interval_ms
    );

    let activity = PlatformActivityMonitor;
    crate::daemon::run_forever(hal, &path, &activity).map_err(anyhow::Error::new)
}
