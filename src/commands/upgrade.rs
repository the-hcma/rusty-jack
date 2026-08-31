//! `rusty-jack upgrade` — refresh the per-user LaunchAgent when needed.

use crate::config::default_config_path;
use crate::launchd::{print_upgrade_result, upgrade_daemon};
use crate::native_driver::{
    print_upgrade_result as print_driver_upgrade_result, upgrade_if_materially_changed,
};
use crate::privacy_permissions::{
    ensure_privacy_permissions_for_setup, print_privacy_permission_status,
};
use crate::setup::terminal_is_interactive;
use anyhow::Result;

/// Refresh the plist to the current binary path and restart the LaunchAgent when needed.
pub fn run(json: bool, force_reload: bool) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    // LaunchAgent runs `daemon` without `--config`; check the default path only
    // (ignore CLI `--config` / RUSTY_JACK_CONFIG so we match the installed daemon).
    let privacy =
        ensure_privacy_permissions_for_setup(interactive, default_config_path().as_deref())
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
