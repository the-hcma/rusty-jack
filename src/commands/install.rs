//! `rusty-jack install` — install the per-user launchd LaunchAgent.

use crate::coreaudio::AudioHal;
use crate::launchd::{install_daemon, print_install_result};
use crate::setup::{ensure_default_config, print_config_setup_result, terminal_is_interactive};
use anyhow::Result;

/// Install and start the per-user LaunchAgent.
pub fn run(hal: &dyn AudioHal, json: bool) -> Result<()> {
    let config = ensure_default_config(hal, !json && terminal_is_interactive())
        .map_err(anyhow::Error::new)?;
    let result = install_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "config": config,
            "daemon": result,
        }))?;
        println!("{value}");
    } else {
        print_config_setup_result(&config);
        print_install_result(&result);
    }

    Ok(())
}
