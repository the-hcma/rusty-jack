//! `rusty-jack resume` — restart a paused daemon.

use crate::config::{load_config_optional, resolve_config_path, Config};
use crate::coreaudio::AudioHal;
use crate::daemon::{daemon_tick, DaemonTickReason};
use crate::launchd::{daemon_status, print_resume_result, resume_daemon, DaemonStatus};
use anyhow::Result;
use std::path::Path;

/// Re-enable and load the LaunchAgent after `pause`.
pub fn run(hal: &dyn AudioHal, json: bool, config_path: Option<&Path>) -> Result<()> {
    let prepared_volume = prepare_audio_before_resume_if_needed(hal, config_path)?;
    let result = resume_daemon().map_err(anyhow::Error::new)?;

    if json {
        let value = serde_json::to_string_pretty(&result)?;
        println!("{value}");
    } else {
        print_resume_result(&result);
        if let Some(volume) = prepared_volume {
            println!("  prepared audio: routed and restored to {volume}%");
        }
    }

    Ok(())
}

fn prepare_audio_before_resume_if_needed(
    hal: &dyn AudioHal,
    config_path: Option<&Path>,
) -> Result<Option<u8>> {
    if !matches!(
        daemon_status().map_err(anyhow::Error::new)?,
        DaemonStatus::Paused { .. }
    ) {
        return Ok(None);
    }

    let Some(config) = load_resume_config(config_path)? else {
        return Ok(None);
    };

    apply_policy_before_resume(hal, &config)
}

fn load_resume_config(config_path: Option<&Path>) -> Result<Option<Config>> {
    let Some(path) = resolve_config_path(config_path) else {
        return Ok(None);
    };
    let explicit = config_path.is_some();
    Ok(load_config_optional(&path, explicit)?)
}

fn apply_policy_before_resume(hal: &dyn AudioHal, config: &Config) -> Result<Option<u8>> {
    let Some(volume) = config.volume else {
        return Ok(None);
    };

    let _ = daemon_tick(hal, config, DaemonTickReason::Startup).map_err(anyhow::Error::new)?;
    Ok(Some(volume))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceSelectorConfig;
    use crate::coreaudio::mock::{MockHal, SetVolumeCall};
    use crate::launchd::ResumeResult;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn config_with_volume(volume: Option<u8>) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            preferred_device: DeviceSelectorConfig {
                uid: Some("hdmi".into()),
                monitor_name: None,
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume,
            scalar_webapi_device: None,
        }
    }

    fn device(uid: &str, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "Output".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: active,
            is_active: active,
            monitor_name: None,
        }
    }

    #[test]
    fn test_run_json_when_not_installed() {
        if matches!(resume_daemon().unwrap(), ResumeResult::NotInstalled { .. }) {
            let hal = MockHal::new(vec![]);
            run(&hal, true, None).unwrap();
        }
    }

    #[test]
    fn test_apply_policy_before_resume_switches_and_restores_volume() {
        let hal = MockHal::new(vec![device("builtin", true), device("hdmi", false)])
            .with_default("builtin");

        let restored = apply_policy_before_resume(&hal, &config_with_volume(Some(25))).unwrap();

        assert_eq!(restored, Some(25));
        assert_eq!(
            hal.volume_calls(),
            vec![
                SetVolumeCall {
                    uid: "hdmi".into(),
                    percent: 25,
                },
                SetVolumeCall {
                    uid: "hdmi".into(),
                    percent: 25,
                }
            ]
        );
    }

    #[test]
    fn test_apply_policy_before_resume_restores_volume_when_already_on_target() {
        let hal = MockHal::new(vec![device("hdmi", true)]).with_default("hdmi");

        let restored = apply_policy_before_resume(&hal, &config_with_volume(Some(25))).unwrap();

        assert_eq!(restored, Some(25));
        assert_eq!(
            hal.volume_calls(),
            vec![SetVolumeCall {
                uid: "hdmi".into(),
                percent: 25,
            }]
        );
    }

    #[test]
    fn test_apply_policy_before_resume_skips_without_config_volume() {
        let hal = MockHal::new(vec![device("builtin", true)]).with_default("builtin");

        let restored = apply_policy_before_resume(&hal, &config_with_volume(None)).unwrap();

        assert_eq!(restored, None);
        assert!(hal.volume_calls().is_empty());
    }
}
