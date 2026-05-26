//! `rusty-jack upgrade` — refresh and restart the per-user LaunchAgent.

use crate::launchd::{print_upgrade_result, upgrade_daemon};
use crate::native_driver::{
    print_upgrade_result as print_driver_upgrade_result, upgrade_if_materially_changed,
};
use crate::setup::terminal_is_interactive;
use anyhow::Result;

/// Refresh the plist to the current binary path and restart the LaunchAgent.
pub fn run(json: bool) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    let native_driver = upgrade_if_materially_changed(interactive).map_err(anyhow::Error::new)?;
    let result = upgrade_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "daemon": result,
            "native_driver": native_driver,
        }))?;
        println!("{value}");
    } else {
        print_driver_upgrade_result(&native_driver);
        print_upgrade_result(&result);
    }

    Ok(())
}
