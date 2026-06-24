//! Hardware smoke helpers for the native HAL driver (macOS only).
//!
//! Set `RUSTY_JACK_HAL_DRIVER_SMOKE=1` and run:
//! `cargo test --test native_driver_hal_smoke -- --ignored --nocapture`

use crate::config::{Config, DeviceSelectorConfig, LoggingConfig};
use crate::coreaudio::AudioHal;
use crate::eqmac::EQMAC_HAL_DRIVER_PATH;
use crate::hdmi_displayport_volume_control::{native_driver_info, RUSTY_JACK_VIRTUAL_OUTPUT_UID};
use crate::native_driver::{
    remove_native_driver_if_installed, restore_eqmac_hal_driver, swap_in_for_testing,
    DriverSwapInResult, NativeDriverInstallResult,
};
use crate::output_device::OutputDevice;
use crate::passthrough::{plan_passthrough, PassthroughRing, PASSTHROUGH_RING_PATH};
use crate::transport::TransportKind;
use crate::RustyJackError;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// Environment variable that opts into mutating HAL plugins without prompts.
pub const HAL_DRIVER_SMOKE_ENV: &str = "RUSTY_JACK_HAL_DRIVER_SMOKE";

const VIRTUAL_OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const VIRTUAL_OUTPUT_DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// True when `RUSTY_JACK_HAL_DRIVER_SMOKE=1` (or `true` / `yes`).
#[must_use]
pub fn hal_driver_smoke_enabled() -> bool {
    std::env::var(HAL_DRIVER_SMOKE_ENV)
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

/// Interactive CLI or HAL smoke env allows sudo HAL moves without a dialog.
#[must_use]
pub fn system_driver_moves_allowed(interactive: bool) -> bool {
    interactive || hal_driver_smoke_enabled()
}

/// Quit the eqMac app so it does not compete for the HDMI virtual route during smoke tests.
pub fn quit_eqmac_app() -> Result<(), RustyJackError> {
    let _ = Command::new("osascript")
        .args([
            "-e",
            r#"try
  tell application "eqMac" to quit
end try"#,
        ])
        .status()
        .map_err(RustyJackError::Io)?;
    let _ = Command::new("killall")
        .arg("eqMac")
        .status()
        .map_err(RustyJackError::Io)?;
    thread::sleep(Duration::from_millis(500));
    Ok(())
}

/// Install Rusty Jack to the system HAL folder and move eqMac's driver aside when needed.
pub fn swap_in_hal_smoke() -> Result<DriverSwapInResult, RustyJackError> {
    quit_eqmac_app()?;
    // Always non-interactive; `RUSTY_JACK_HAL_DRIVER_SMOKE=1` enables sudo moves inside swap_in.
    swap_in_for_testing(false)
}

/// Restore eqMac and remove Rusty Jack from system/user HAL paths (smoke teardown).
pub fn swap_out_hal_smoke() -> Result<(), RustyJackError> {
    let allow = system_driver_moves_allowed(false);
    restore_eqmac_hal_driver(allow)?;
    remove_native_driver_if_installed()?;
    Ok(())
}

/// Whether CoreAudio currently lists the Rusty Jack virtual output UID.
#[must_use]
pub fn virtual_output_listed(devices: &[OutputDevice]) -> bool {
    devices
        .iter()
        .any(|device| device.uid == RUSTY_JACK_VIRTUAL_OUTPUT_UID)
}

fn missing_virtual_output_hint() -> String {
    const HELPER_NEEDLE: &str = "Core Audio Driver (RustyJack.driver)";
    let helper_running = crate::process_detect::any_process_cmdline_contains(HELPER_NEEDLE);
    let mut parts: Vec<String> = vec![
        "Check Console for `amfid: ... RustyJack ... signature not valid: -67050` (adhoc-signed drivers are often rejected)."
            .into(),
        "Production builds need a Developer ID–signed RustyJack.driver.".into(),
    ];
    if helper_running {
        parts.push(
            "CoreAudio spawned `Core Audio Driver (RustyJack.driver)` but did not publish the virtual device — signing or driver registration is the likely blocker.".into(),
        );
    } else {
        parts.push(
            "No `Core Audio Driver (RustyJack.driver)` helper process — coreaudiod may not have loaded the bundle.".into(),
        );
    }
    parts.join(" ")
}

/// Poll until the virtual output appears or the timeout elapses.
pub fn wait_for_virtual_output(
    hal: &dyn AudioHal,
    timeout: Duration,
) -> Result<Vec<OutputDevice>, RustyJackError> {
    let deadline = Instant::now() + timeout;
    loop {
        let list = hal.list_outputs()?;
        if virtual_output_listed(&list.devices) {
            return Ok(list.devices);
        }
        if Instant::now() >= deadline {
            return Err(RustyJackError::Config(format!(
                "timed out after {}s waiting for CoreAudio virtual output {RUSTY_JACK_VIRTUAL_OUTPUT_UID}. {}",
                timeout.as_secs(),
                missing_virtual_output_hint()
            )));
        }
        thread::sleep(VIRTUAL_OUTPUT_POLL_INTERVAL);
    }
}

/// Minimal config targeting the first alive HDMI/DisplayPort output.
pub fn smoke_config_for_hdmi(devices: &[OutputDevice]) -> Result<Config, RustyJackError> {
    let hdmi = devices
        .iter()
        .find(|device| {
            device.is_alive
                && device.is_selectable()
                && matches!(
                    device.transport,
                    TransportKind::Hdmi | TransportKind::DisplayPort
                )
        })
        .ok_or_else(|| {
            RustyJackError::Config(
                "no alive HDMI/DisplayPort output for HAL driver smoke test".into(),
            )
        })?;
    Ok(Config {
        version: 1,
        auto_switch: true,
        poll_interval_ms: 2_000,
        switch_delay_ms: 500,
        activity_idle_threshold_ms: 60_000,
        activity_poll_interval_ms: 1_000,
        activity_monitor: "idle".into(),
        activity_active_confirm_ms: 5_000,
        activity_event_tap_include_mouse_move: false,
        preferred_device: DeviceSelectorConfig {
            name: Some(hdmi.friendly_label()),
            uid: Some(hdmi.uid.clone()),
        },
        preferred_device_uid: None,
        fallback_uids: vec![],
        also_set_system_output: true,
        volume: Some(25),
        scalar_webapi_device: None,
        logging: LoggingConfig::default(),
    })
}

/// Snapshot used by smoke tests and agents to see what still blocks passthrough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HalSmokeSnapshot {
    pub bundled_driver_available: bool,
    pub system_rusty_jack_installed: bool,
    pub eqmac_hal_present: bool,
    pub virtual_output_listed: bool,
    pub passthrough_ring_path: String,
    pub passthrough_ring_open_ok: bool,
    pub passthrough_plan_ready: bool,
}

/// Inspect post-install state without mutating audio routes.
pub fn hal_smoke_snapshot(
    hal: &dyn AudioHal,
    config: &Config,
) -> Result<HalSmokeSnapshot, RustyJackError> {
    let list = hal.list_outputs()?;
    let bundled_driver_available = crate::native_driver::bundled_native_driver_path().is_some();
    let system_path = Path::new("/Library/Audio/Plug-Ins/HAL/RustyJack.driver");
    let passthrough_ring_open_ok = PassthroughRing::open().is_ok();
    let passthrough_plan_ready =
        native_driver_info().is_some() && plan_passthrough(config, &list.devices).is_some();
    Ok(HalSmokeSnapshot {
        bundled_driver_available,
        system_rusty_jack_installed: system_path.is_dir(),
        eqmac_hal_present: Path::new(EQMAC_HAL_DRIVER_PATH).is_dir(),
        virtual_output_listed: virtual_output_listed(&list.devices),
        passthrough_ring_path: PASSTHROUGH_RING_PATH.into(),
        passthrough_ring_open_ok,
        passthrough_plan_ready,
    })
}

/// End-to-end HAL smoke: swap-in, wait for virtual device, verify ring + passthrough plan.
pub fn run_hal_driver_smoke(hal: &dyn AudioHal) -> Result<HalSmokeSnapshot, RustyJackError> {
    if !hal_driver_smoke_enabled() {
        return Err(RustyJackError::Config(format!(
            "set {HAL_DRIVER_SMOKE_ENV}=1 before running HAL driver smoke tests"
        )));
    }
    if crate::native_driver::bundled_native_driver_path().is_none() {
        return Err(RustyJackError::Config(
            "bundled RustyJack.driver not found; run `make driver-bundle` or `make install` first"
                .into(),
        ));
    }

    let swap = swap_in_hal_smoke()?;
    match &swap {
        DriverSwapInResult::SwappedIn { native_driver, .. } => match native_driver {
            NativeDriverInstallResult::Installed { .. }
            | NativeDriverInstallResult::AlreadyInstalled { .. } => {}
            other => {
                return Err(RustyJackError::Config(format!(
                    "driver swap-in did not install native driver: {other:?}"
                )));
            }
        },
        DriverSwapInResult::Skipped { reason, .. } => {
            return Err(RustyJackError::Config(format!(
                "driver swap-in skipped: {reason}"
            )));
        }
    }

    let devices = wait_for_virtual_output(hal, VIRTUAL_OUTPUT_DEFAULT_TIMEOUT)?;
    let config = smoke_config_for_hdmi(&devices)?;
    let snapshot = hal_smoke_snapshot(hal, &config)?;
    if !snapshot.virtual_output_listed {
        return Err(RustyJackError::Config(
            "Rusty Jack HAL bundle is installed but CoreAudio did not publish the virtual output"
                .into(),
        ));
    }
    if !snapshot.passthrough_ring_open_ok {
        return Err(RustyJackError::Config(format!(
            "could not open passthrough ring at {}",
            snapshot.passthrough_ring_path
        )));
    }
    if !snapshot.passthrough_plan_ready {
        return Err(RustyJackError::Config(
            "passthrough plan could not be built; check preferred HDMI/DP target and driver stage"
                .into(),
        ));
    }
    Ok(snapshot)
}

/// RAII helper: restores eqMac on drop when smoke mode is enabled.
pub struct HalSmokeGuard {
    active: bool,
}

impl HalSmokeGuard {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for HalSmokeGuard {
    fn default() -> Self {
        Self {
            active: hal_driver_smoke_enabled(),
        }
    }
}

impl Drop for HalSmokeGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Err(err) = swap_out_hal_smoke() {
            eprintln!("warning: HAL smoke teardown (eqMac restore) failed: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_output_listed_detects_uid() {
        let devices = vec![OutputDevice {
            id: 1,
            uid: RUSTY_JACK_VIRTUAL_OUTPUT_UID.into(),
            name: "Rusty Jack".into(),
            transport: TransportKind::Virtual,
            is_alive: true,
            is_default: false,
            is_active: false,
        }];
        assert!(virtual_output_listed(&devices));
    }

    #[test]
    fn test_system_driver_moves_allowed_requires_smoke_or_interactive() {
        std::env::remove_var(HAL_DRIVER_SMOKE_ENV);
        assert!(!system_driver_moves_allowed(false));
        assert!(system_driver_moves_allowed(true));
    }
}
