//! `rusty-jack uninstall` — remove LaunchAgent and optionally config.

use crate::launchd::{print_disable_result, uninstall_daemon};
use crate::setup::{
    maybe_remove_default_config, print_config_removal_result, terminal_is_interactive,
    ConfigRemovalMode,
};
use anyhow::Result;

/// Stop the daemon, remove the LaunchAgent plist, and optionally remove config.
pub fn run(json: bool, remove_config: bool, keep_config: bool) -> Result<()> {
    let daemon = uninstall_daemon().map_err(anyhow::Error::new)?;
    let mode = if remove_config {
        ConfigRemovalMode::Remove
    } else if keep_config || json {
        ConfigRemovalMode::Keep
    } else {
        ConfigRemovalMode::Prompt
    };
    let config = maybe_remove_default_config(mode, !json && terminal_is_interactive())
        .map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "daemon": daemon,
            "config": config,
        }))?;
        println!("{value}");
    } else {
        print_disable_result(&daemon);
        print_config_removal_result(&config);
    }

    Ok(())
}
