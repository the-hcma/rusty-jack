//! `rusty-jack install` — install the per-user launchd LaunchAgent.

use crate::coreaudio::AudioHal;
use crate::hdmi_displayport_volume_control::hdmi_displayport_volume_control_status;
use crate::launchd::{install_daemon, print_install_result};
use crate::native_driver::{
    install_for_connected_hdmi_displayport, print_install_result as print_driver_install_result,
};
use crate::privacy_permissions::{
    ensure_privacy_permissions_for_setup, print_privacy_permission_status,
};
use crate::setup::{ensure_default_config, print_config_setup_result, terminal_is_interactive};
use crate::RustyJackError;
use anyhow::Result;
use dialoguer::console::style;
use dialoguer::Confirm;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum OrphanedEqMacDriverCleanupResult {
    NotFound,
    Recommended { path: String, command: String },
    Removed { path: String },
    Skipped { path: String, reason: String },
}

/// Install and start the per-user LaunchAgent.
pub fn run(hal: &dyn AudioHal, json: bool) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    let config = ensure_default_config(hal, interactive).map_err(anyhow::Error::new)?;
    let list = hal.list_outputs().ok();
    let mut hdmi_displayport_volume_control = list
        .as_ref()
        .map(|list| hdmi_displayport_volume_control_status(&list.devices));
    let eqmac_cleanup =
        cleanup_orphaned_eqmac_driver(hdmi_displayport_volume_control.as_ref(), interactive)
            .map_err(anyhow::Error::new)?;
    if matches!(
        eqmac_cleanup,
        OrphanedEqMacDriverCleanupResult::Removed { .. }
    ) {
        hdmi_displayport_volume_control = list
            .as_ref()
            .map(|list| hdmi_displayport_volume_control_status(&list.devices));
    }
    let native_driver = if let Some(list) = &list {
        Some(
            install_for_connected_hdmi_displayport(&list.devices, interactive)
                .map_err(anyhow::Error::new)?,
        )
    } else {
        None
    };
    // LaunchAgent runs `daemon` without `--config` and reads the default path
    // (same file `ensure_default_config` wrote). Do not use CLI `--config`/env here.
    let privacy = ensure_privacy_permissions_for_setup(
        interactive,
        crate::config::default_config_path().as_deref(),
    )
    .map_err(anyhow::Error::new)?;
    let result = install_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "config": config,
            "daemon": result,
            "eqmac_cleanup": eqmac_cleanup,
            "hdmi_displayport_volume_control": hdmi_displayport_volume_control,
            "native_driver": native_driver,
            "privacy_permissions": privacy,
        }))?;
        println!("{value}");
    } else {
        print_config_setup_result(&config);
        if let Some(recommendation) = hdmi_displayport_volume_control
            .as_ref()
            .and_then(|status| status.recommendation.as_ref())
        {
            println!();
            println!("HDMI/DisplayPort volume control");
            println!("  note: {recommendation}");
        }
        print_eqmac_cleanup_result(&eqmac_cleanup);
        if let Some(native_driver) = &native_driver {
            print_driver_install_result(native_driver);
        }
        print_install_result(&result);
        print_privacy_permission_status(&privacy);
    }

    Ok(())
}

fn cleanup_orphaned_eqmac_driver(
    status: Option<&crate::hdmi_displayport_volume_control::HdmiDisplayPortVolumeControlStatus>,
    interactive: bool,
) -> Result<OrphanedEqMacDriverCleanupResult, RustyJackError> {
    let Some(path) = status.and_then(|status| status.orphaned_eqmac_hal_driver_path.clone()) else {
        return Ok(OrphanedEqMacDriverCleanupResult::NotFound);
    };
    let command = format!("sudo rm -rf {path}");

    if !interactive {
        return Ok(OrphanedEqMacDriverCleanupResult::Recommended { path, command });
    }

    if !Confirm::new()
        .with_prompt(format!(
            "{}",
            style(format!(
                "Remove orphaned eqMac HAL driver at {path}?\nThis uses sudo."
            ))
            .cyan()
        ))
        .default(false)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("eqMac cleanup prompt failed: {err}")))?
    {
        return Ok(OrphanedEqMacDriverCleanupResult::Skipped {
            path,
            reason: "user declined orphaned eqMac driver removal".into(),
        });
    }

    let status = std::process::Command::new("sudo")
        .args(["rm", "-rf", path.as_str()])
        .status()
        .map_err(RustyJackError::Io)?;
    if !status.success() {
        return Err(RustyJackError::Config(format!(
            "failed to remove orphaned eqMac driver with `{command}`"
        )));
    }

    Ok(OrphanedEqMacDriverCleanupResult::Removed { path })
}

fn print_eqmac_cleanup_result(result: &OrphanedEqMacDriverCleanupResult) {
    match result {
        OrphanedEqMacDriverCleanupResult::NotFound => {}
        OrphanedEqMacDriverCleanupResult::Recommended { path, command } => {
            println!();
            println!("Orphaned eqMac driver");
            println!("  path:    {path}");
            println!("  cleanup: {command}");
        }
        OrphanedEqMacDriverCleanupResult::Removed { path } => {
            println!();
            println!("Removed orphaned eqMac driver");
            println!("  path: {path}");
        }
        OrphanedEqMacDriverCleanupResult::Skipped { path, reason } => {
            println!();
            println!("Kept orphaned eqMac driver");
            println!("  path:   {path}");
            println!("  reason: {reason}");
        }
    }
}
