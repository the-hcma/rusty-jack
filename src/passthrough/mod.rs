//! Native-driver passthrough pipeline (virtual HAL capture → gain → physical render).

mod gain;

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
mod engine;
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
mod ring;

use crate::config::Config;
use crate::coreaudio::AudioHal;
use crate::hdmi_displayport_volume_control::{
    native_driver_info, route_needs_hdmi_displayport_volume_control, RUSTY_JACK_DRIVER_NAME,
};
use crate::output_device::OutputDevice;
use crate::policy::{select_routing_target, RoutingTarget, RoutingTargetSource, SelectTargetError};
use crate::system_default::HalDriverInfo;
use crate::RustyJackError;
use serde::Serialize;

pub use gain::{apply_stereo_interleaved_gain, percent_to_scalar};

/// CoreAudio UID for the Rusty Jack virtual output published by the HAL driver.
pub const RUSTY_JACK_VIRTUAL_OUTPUT_UID: &str = "com.the-hcma.rusty-jack.driver.output";

/// Driver stage while the daemon plans passthrough without live audio I/O.
pub const PASSTHROUGH_SKELETON_DRIVER_STAGE: &str = "passthrough-skeleton";

/// Driver stage when capture/render is wired through the shared ring and daemon IO proc.
pub const PASSTHROUGH_ACTIVE_DRIVER_STAGE: &str = "passthrough-active";

/// Stereo format shared with `driver/RustyJack/RustyJackAudioServerPlugIn.c`.
pub const PASSTHROUGH_SAMPLE_RATE_HZ: u32 = 48_000;
pub const PASSTHROUGH_CHANNEL_COUNT: usize = 2;
pub const PASSTHROUGH_FRAMES_PER_CHUNK: usize = 512;

/// Whether the passthrough controller is armed for the current route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassthroughMode {
    /// Native driver absent or route does not need HDMI/DP software volume.
    Disabled,
    /// Driver installed and route qualifies; planning only (legacy / driver not yet active).
    Skeleton,
    /// Daemon is rendering captured audio to the physical HDMI/DP device.
    Active,
}

/// Target route for the passthrough renderer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PassthroughPlan {
    pub virtual_output_uid: String,
    pub physical_uid: String,
    pub physical_name: String,
    pub volume_percent: u8,
    pub volume_scalar: f32,
    pub driver_stage: Option<String>,
}

/// Tracks passthrough lifecycle inside the daemon loop.
#[derive(Default)]
pub struct PassthroughController {
    active: Option<PassthroughPlan>,
    #[cfg(target_os = "macos")]
    engine: Option<engine::PassthroughEngine>,
}

impl std::fmt::Debug for PassthroughController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassthroughController")
            .field("active", &self.active)
            .field("engine_running", &self.engine.is_some())
            .finish()
    }
}

impl PassthroughController {
    /// Reconcile passthrough state with the latest config and device list.
    pub fn sync(&mut self, hal: &dyn AudioHal, config: &Config) -> Result<(), RustyJackError> {
        let list = hal.list_outputs()?;
        let plan = plan_passthrough(config, &list.devices);
        self.apply_plan(plan);
        Ok(())
    }

    #[must_use]
    pub fn mode(&self) -> PassthroughMode {
        if self.active.is_none() {
            return PassthroughMode::Disabled;
        }
        #[cfg(target_os = "macos")]
        if self.engine.is_some() {
            return PassthroughMode::Active;
        }
        PassthroughMode::Skeleton
    }

    #[must_use]
    pub fn active_plan(&self) -> Option<&PassthroughPlan> {
        self.active.as_ref()
    }

    pub(crate) fn apply_plan(&mut self, plan: Option<PassthroughPlan>) {
        if self.active == plan {
            return;
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(engine) = self.engine.take() {
                drop(engine);
            }
        }

        match (&self.active, &plan) {
            (None, Some(next)) => {
                eprintln!(
                    "passthrough: armed for {} at {}% (virtual {} → physical {})",
                    next.physical_name,
                    next.volume_percent,
                    next.virtual_output_uid,
                    next.physical_uid
                );
            }
            (Some(previous), None) => {
                eprintln!(
                    "passthrough: disarmed (was {} at {}%)",
                    previous.physical_name, previous.volume_percent
                );
            }
            (Some(previous), Some(next)) if previous != next => {
                eprintln!(
                    "passthrough: retargeted {} → {} at {}%",
                    previous.physical_name, next.physical_name, next.volume_percent
                );
            }
            _ => {}
        }

        self.active = plan.clone();

        #[cfg(all(target_os = "macos", not(test)))]
        if let Some(next) = plan {
            match engine::PassthroughEngine::start(&next) {
                Ok(engine) => {
                    self.engine = Some(engine);
                }
                Err(err) => {
                    eprintln!("warning: passthrough engine failed to start: {err}");
                    self.active = None;
                }
            }
        }
    }
}

/// Policy-selected HDMI/DP target before virtual default substitution.
pub fn select_physical_routing_target(
    config: &Config,
    devices: &[OutputDevice],
) -> Result<RoutingTarget, SelectTargetError> {
    select_routing_target(config, devices)
}

/// Default output target: Rusty Jack virtual device when passthrough is active.
pub fn select_effective_routing_target(
    config: &Config,
    devices: &[OutputDevice],
) -> Result<RoutingTarget, SelectTargetError> {
    let physical = select_physical_routing_target(config, devices)?;
    let Some(driver) = native_driver_info() else {
        return Ok(physical);
    };
    select_effective_routing_target_with_native_driver(config, devices, &driver)
}

pub(crate) fn select_effective_routing_target_with_native_driver(
    config: &Config,
    devices: &[OutputDevice],
    driver: &HalDriverInfo,
) -> Result<RoutingTarget, SelectTargetError> {
    let physical = select_physical_routing_target(config, devices)?;
    let Some(plan) = plan_passthrough_with_native_driver(config, devices, driver) else {
        return Ok(physical);
    };
    if plan.physical_uid != physical.uid {
        return Ok(physical);
    }
    let Some(virtual_device) = devices
        .iter()
        .find(|device| device.uid == RUSTY_JACK_VIRTUAL_OUTPUT_UID)
    else {
        return Ok(physical);
    };
    Ok(RoutingTarget {
        uid: virtual_device.uid.clone(),
        name: virtual_device.friendly_label(),
        source: physical.source,
    })
}

/// Physical HDMI/DP UID used for volume-control checks while the virtual device is default.
#[must_use]
pub fn passthrough_physical_uid<'a>(
    config: &'a Config,
    devices: &'a [OutputDevice],
) -> Option<&'a str> {
    let driver = native_driver_info()?;
    let plan = plan_passthrough_with_native_driver(config, devices, &driver)?;
    devices
        .iter()
        .find(|device| device.uid == plan.physical_uid)
        .map(|device| device.uid.as_str())
}

/// Volume level to apply when switching to the effective (virtual) default.
#[must_use]
pub fn volume_for_effective_target(
    config: &Config,
    effective: &RoutingTarget,
    physical: &RoutingTarget,
    preferred_uid: &Option<String>,
) -> Option<u8> {
    if preferred_uid.as_deref() == Some(physical.uid.as_str())
        || matches!(physical.source, RoutingTargetSource::Preferred)
    {
        return config.volume;
    }
    if effective.uid == RUSTY_JACK_VIRTUAL_OUTPUT_UID {
        return config.volume;
    }
    crate::volume_memory::remembered_volume(&effective.uid)
}

/// Build a passthrough plan when the native driver is installed and policy targets HDMI/DP.
#[must_use]
pub fn plan_passthrough(config: &Config, devices: &[OutputDevice]) -> Option<PassthroughPlan> {
    let driver = native_driver_info()?;
    plan_passthrough_with_native_driver(config, devices, &driver)
}

#[must_use]
pub(crate) fn plan_passthrough_with_native_driver(
    config: &Config,
    devices: &[OutputDevice],
    driver: &HalDriverInfo,
) -> Option<PassthroughPlan> {
    let target = select_routing_target(config, devices).ok()?;
    if !route_needs_hdmi_displayport_volume_control(devices, &target.uid) {
        return None;
    }

    let physical = devices.iter().find(|device| device.uid == target.uid)?;
    if physical.uid == RUSTY_JACK_VIRTUAL_OUTPUT_UID {
        return None;
    }

    let volume_percent = config.volume.unwrap_or(100);
    Some(PassthroughPlan {
        virtual_output_uid: RUSTY_JACK_VIRTUAL_OUTPUT_UID.into(),
        physical_uid: physical.uid.clone(),
        physical_name: physical.friendly_label(),
        volume_percent,
        volume_scalar: percent_to_scalar(volume_percent),
        driver_stage: driver.stage.clone(),
    })
}

/// Human-readable note for status output when passthrough is active or planned.
#[must_use]
pub fn passthrough_status_note(
    mode: PassthroughMode,
    plan: Option<&PassthroughPlan>,
) -> Option<String> {
    let plan = plan?;
    match mode {
        PassthroughMode::Disabled => None,
        PassthroughMode::Skeleton => Some(format!(
            "passthrough skeleton armed for {name} at {volume}% via {driver}; live CoreAudio I/O is not wired yet",
            name = plan.physical_name,
            volume = plan.volume_percent,
            driver = RUSTY_JACK_DRIVER_NAME
        )),
        PassthroughMode::Active => Some(format!(
            "passthrough active for {name} at {volume}% via {driver} (virtual default → physical render)",
            name = plan.physical_name,
            volume = plan.volume_percent,
            driver = RUSTY_JACK_DRIVER_NAME
        )),
    }
}

/// Exercise the gain path on a silent buffer (unit tests and future IO hook).
pub fn process_silent_chunk(plan: &PassthroughPlan) -> Vec<f32> {
    let sample_count = PASSTHROUGH_FRAMES_PER_CHUNK * PASSTHROUGH_CHANNEL_COUNT;
    let mut samples = vec![0.0; sample_count];
    apply_stereo_interleaved_gain(&mut samples, plan.volume_scalar);
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceSelectorConfig};
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn config_with_hdmi_preferred(volume: Option<u8>) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            preferred_device: DeviceSelectorConfig {
                name: Some("HDMI".into()),
                uid: Some("hdmi".into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume,
            scalar_webapi_device: None,
        }
    }

    fn hdmi_device(active: bool) -> OutputDevice {
        OutputDevice {
            id: 2,
            uid: "hdmi".into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
        }
    }

    fn virtual_device() -> OutputDevice {
        OutputDevice {
            id: 99,
            uid: RUSTY_JACK_VIRTUAL_OUTPUT_UID.into(),
            name: RUSTY_JACK_DRIVER_NAME.into(),
            transport: TransportKind::Virtual,
            is_alive: true,
            is_default: false,
            is_active: false,
        }
    }

    fn builtin_device(active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: "builtin".into(),
            name: "Built-in Output".into(),
            transport: TransportKind::BuiltIn,
            is_alive: true,
            is_default: false,
            is_active: active,
        }
    }

    fn fake_driver() -> HalDriverInfo {
        HalDriverInfo {
            name: RUSTY_JACK_DRIVER_NAME.into(),
            bundle_id: "com.the-hcma.rusty-jack.driver".into(),
            version: Some("0.1.1".into()),
            stage: Some(PASSTHROUGH_ACTIVE_DRIVER_STAGE.into()),
            install_path: "/tmp/RustyJack.driver".into(),
        }
    }

    #[test]
    fn test_select_effective_routing_target_uses_virtual_when_passthrough_planned() {
        let config = config_with_hdmi_preferred(Some(40));
        let devices = vec![builtin_device(false), hdmi_device(true), virtual_device()];
        let physical = select_physical_routing_target(&config, &devices).expect("physical target");
        assert_eq!(physical.uid, "hdmi");
        let effective =
            select_effective_routing_target_with_native_driver(&config, &devices, &fake_driver())
                .expect("effective target");
        assert_eq!(effective.uid, RUSTY_JACK_VIRTUAL_OUTPUT_UID);
    }

    #[test]
    fn test_plan_passthrough_skips_non_hdmi_target() {
        let mut config = config_with_hdmi_preferred(Some(40));
        config.preferred_device = DeviceSelectorConfig {
            name: Some("Built-in Output".into()),
            uid: Some("builtin".into()),
        };
        let devices = vec![builtin_device(true), hdmi_device(false)];
        assert!(plan_passthrough_with_native_driver(&config, &devices, &fake_driver()).is_none());
    }

    #[test]
    fn test_plan_passthrough_without_native_driver_is_none() {
        if native_driver_info().is_some() {
            return;
        }
        let config = config_with_hdmi_preferred(Some(40));
        let devices = vec![hdmi_device(true)];
        assert!(plan_passthrough(&config, &devices).is_none());
    }

    #[test]
    fn test_plan_passthrough_with_native_driver_and_hdmi_target() {
        let config = config_with_hdmi_preferred(Some(40));
        let devices = vec![builtin_device(false), hdmi_device(true)];
        let plan = plan_passthrough_with_native_driver(&config, &devices, &fake_driver()).unwrap();

        assert_eq!(plan.physical_uid, "hdmi");
        assert_eq!(plan.volume_percent, 40);
        assert!((plan.volume_scalar - 0.4).abs() < f32::EPSILON);
        assert_eq!(plan.virtual_output_uid, RUSTY_JACK_VIRTUAL_OUTPUT_UID);
    }

    #[test]
    fn test_process_silent_chunk_applies_gain() {
        let plan = PassthroughPlan {
            virtual_output_uid: RUSTY_JACK_VIRTUAL_OUTPUT_UID.into(),
            physical_uid: "hdmi".into(),
            physical_name: "HDMI".into(),
            volume_percent: 50,
            volume_scalar: 0.5,
            driver_stage: Some(PASSTHROUGH_ACTIVE_DRIVER_STAGE.into()),
        };
        let samples = process_silent_chunk(&plan);
        assert_eq!(
            samples.len(),
            PASSTHROUGH_FRAMES_PER_CHUNK * PASSTHROUGH_CHANNEL_COUNT
        );
        assert!(samples.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn test_passthrough_controller_apply_plan_is_idempotent() {
        let plan = PassthroughPlan {
            virtual_output_uid: RUSTY_JACK_VIRTUAL_OUTPUT_UID.into(),
            physical_uid: "hdmi".into(),
            physical_name: "HDMI".into(),
            volume_percent: 25,
            volume_scalar: 0.25,
            driver_stage: Some(PASSTHROUGH_ACTIVE_DRIVER_STAGE.into()),
        };
        let mut controller = PassthroughController::default();

        controller.apply_plan(Some(plan.clone()));
        assert_eq!(controller.mode(), PassthroughMode::Skeleton);
        controller.apply_plan(Some(plan));
        controller.apply_plan(None);
        assert_eq!(controller.mode(), PassthroughMode::Disabled);
    }
}
