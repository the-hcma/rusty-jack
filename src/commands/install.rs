//! `rusty-jack install` — install the per-user launchd LaunchAgent.

use crate::launchd::{install_daemon, print_install_result};
use anyhow::Result;

/// Install and start the per-user LaunchAgent.
pub fn run(json: bool) -> Result<()> {
    let result = install_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&result)?;
        println!("{value}");
    } else {
        print_install_result(&result);
    }

    Ok(())
}
