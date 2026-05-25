//! `rusty-jack upgrade` — refresh and restart the per-user LaunchAgent.

use crate::launchd::{print_upgrade_result, upgrade_daemon};
use anyhow::Result;

/// Refresh the plist to the current binary path and restart the LaunchAgent.
pub fn run(json: bool) -> Result<()> {
    let result = upgrade_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&result)?;
        println!("{value}");
    } else {
        print_upgrade_result(&result);
    }

    Ok(())
}
