//! `rusty-jack uninstall` — remove LaunchAgent, driver, and optionally config.

use crate::launchd::{print_disable_result, uninstall_daemon};
use crate::native_driver::{
    print_uninstall_result as print_driver_uninstall_result, uninstall_if_installed,
};
use crate::setup::{
    maybe_remove_default_config, print_config_removal_result, terminal_is_interactive,
    ConfigRemovalMode,
};
use anyhow::Result;

/// Stop the daemon, remove the LaunchAgent plist, and optionally remove driver/config.
pub fn run(json: bool, remove_config: bool, keep_config: bool, only_driver: bool) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    if only_driver {
        let native_driver = uninstall_if_installed(interactive).map_err(anyhow::Error::new)?;
        if json {
            let value = serde_json::to_string_pretty(&serde_json::json!({
                "native_driver": native_driver,
            }))?;
            println!("{value}");
        } else {
            print_driver_uninstall_result(&native_driver);
        }
        return Ok(());
    }

    let daemon = uninstall_daemon().map_err(anyhow::Error::new)?;
    let native_driver = uninstall_if_installed(interactive).map_err(anyhow::Error::new)?;
    let mode = if remove_config {
        ConfigRemovalMode::Remove
    } else if keep_config || json {
        ConfigRemovalMode::Keep
    } else {
        ConfigRemovalMode::Prompt
    };
    let config = maybe_remove_default_config(mode, interactive).map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "daemon": daemon,
            "native_driver": native_driver,
            "config": config,
        }))?;
        println!("{value}");
    } else {
        print_disable_result(&daemon);
        print_driver_uninstall_result(&native_driver);
        print_config_removal_result(&config);
    }

    Ok(())
}
