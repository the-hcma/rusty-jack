//! `rusty-jack install` — install the per-user launchd LaunchAgent.

use crate::coreaudio::AudioHal;
use crate::hdmi_displayport_volume_control::hdmi_displayport_volume_control_status;
use crate::launchd::{install_daemon, print_install_result};
use crate::native_driver::{
    install_for_connected_hdmi_displayport, print_install_result as print_driver_install_result,
};
use crate::setup::{ensure_default_config, print_config_setup_result, terminal_is_interactive};
use anyhow::Result;

/// Install and start the per-user LaunchAgent.
pub fn run(hal: &dyn AudioHal, json: bool) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    let config = ensure_default_config(hal, interactive).map_err(anyhow::Error::new)?;
    let list = hal.list_outputs().ok();
    let hdmi_displayport_volume_control = list
        .as_ref()
        .map(|list| hdmi_displayport_volume_control_status(&list.devices));
    let native_driver = if let Some(list) = &list {
        Some(
            install_for_connected_hdmi_displayport(&list.devices, interactive)
                .map_err(anyhow::Error::new)?,
        )
    } else {
        None
    };
    let result = install_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "config": config,
            "daemon": result,
            "hdmi_displayport_volume_control": hdmi_displayport_volume_control,
            "native_driver": native_driver,
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
        if let Some(native_driver) = &native_driver {
            print_driver_install_result(native_driver);
        }
        print_install_result(&result);
    }

    Ok(())
}
