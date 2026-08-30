//! `rusty-jack upgrade` — refresh the per-user LaunchAgent when needed.

use crate::launchd::{print_upgrade_result, upgrade_daemon};
use crate::native_driver::{
    print_upgrade_result as print_driver_upgrade_result, upgrade_if_materially_changed,
};
use crate::privacy_permissions::{
    ensure_privacy_permissions_for_setup, print_privacy_permission_status,
};
use crate::setup::terminal_is_interactive;
use anyhow::Result;
use std::path::Path;

/// Refresh the plist to the current binary path and restart the LaunchAgent when needed.
pub fn run(json: bool, force_reload: bool, config_path: Option<&Path>) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    // Honor global `--config` so privacy checks match the daemon's config path.
    let privacy = ensure_privacy_permissions_for_setup(interactive, config_path)
        .map_err(anyhow::Error::new)?;
    let native_driver = upgrade_if_materially_changed(interactive).map_err(anyhow::Error::new)?;
    let force = force_reload || privacy.force_daemon_restart;
    let result = upgrade_daemon(force).map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "daemon": result,
            "native_driver": native_driver,
            "privacy_permissions": privacy,
        }))?;
        println!("{value}");
    } else {
        print_driver_upgrade_result(&native_driver);
        print_upgrade_result(&result);
        print_privacy_permission_status(&privacy);
    }

    Ok(())
}
