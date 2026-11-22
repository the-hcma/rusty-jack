//! `rusty-jack upgrade` — refresh the per-user LaunchAgent when needed.

use crate::config::default_config_path;
use crate::doctor::{print_report, privacy_report_after_restart, LogCursor, POST_RESTART_WAIT};
use crate::launchd::{print_upgrade_result, upgrade_daemon};
use crate::native_driver::{
    print_upgrade_result as print_driver_upgrade_result, upgrade_if_materially_changed,
};
use crate::privacy_permissions::ensure_privacy_permissions_for_setup;
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
    let log_cursor = LogCursor::capture_default();
    let result = upgrade_daemon(force).map_err(anyhow::Error::new)?;
    // Wait for the restarted daemon to state its own Accessibility verdict, so the output
    // says definitively whether keyboard/mouse wake works instead of pointing at a log.
    let privacy_report = privacy_report_after_restart(&privacy, &log_cursor, POST_RESTART_WAIT);

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "daemon": result,
            "native_driver": native_driver,
            "privacy_permissions": privacy,
            "privacy_report": privacy_report,
        }))?;
        println!("{value}");
    } else {
        print_driver_upgrade_result(&native_driver);
        print_upgrade_result(&result);
        if let Some(report) = &privacy_report {
            print_report(report);
        }
    }

    Ok(())
}
