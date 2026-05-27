//! `rusty-jack resume` — restart a paused daemon.

use crate::config::{load_config_optional, resolve_config_path, Config};
use crate::coreaudio::AudioHal;
use crate::daemon::{daemon_tick, DaemonTickReason};
use crate::launchd::{
    daemon_status, print_resume_result, resume_daemon, DaemonPauseReason, DaemonStatus,
};
use crate::setup::terminal_is_interactive;
use anyhow::Result;
use dialoguer::console::style;
use dialoguer::Confirm;
use std::path::Path;

/// Re-enable and load the LaunchAgent after `pause`.
pub fn run(hal: &dyn AudioHal, json: bool, config_path: Option<&Path>) -> Result<()> {
    let preparation = prepare_audio_before_resume_if_needed(
        hal,
        config_path,
        !json && terminal_is_interactive(),
    )?;
    let prepared_volume = match preparation {
        ResumePreparation::Continue { prepared_volume } => prepared_volume,
        ResumePreparation::Cancelled { selected_label } => {
            if !json {
                println!(
                    "Cancelled. Daemon is still paused and {selected_label} remains selected."
                );
            }
            return Ok(());
        }
    };
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResumePreparation {
    Continue { prepared_volume: Option<u8> },
    Cancelled { selected_label: String },
}

fn prepare_audio_before_resume_if_needed(
    hal: &dyn AudioHal,
    config_path: Option<&Path>,
    interactive: bool,
) -> Result<ResumePreparation> {
    let status = daemon_status().map_err(anyhow::Error::new)?;
    let DaemonStatus::Paused { pause_reason, .. } = status else {
        return Ok(ResumePreparation::Continue {
            prepared_volume: None,
        });
    };

    if let Some(reason) = pause_reason.as_ref() {
        if !confirm_resume_after_picker_override(reason, interactive)? {
            return Ok(ResumePreparation::Cancelled {
                selected_label: picker_override_selected_label(reason).to_string(),
            });
        }
    }

    let Some(config) = load_resume_config(config_path)? else {
        return Ok(ResumePreparation::Continue {
            prepared_volume: None,
        });
    };

    Ok(ResumePreparation::Continue {
        prepared_volume: apply_policy_before_resume(hal, &config)?,
    })
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

fn confirm_resume_after_picker_override(
    reason: &DaemonPauseReason,
    interactive: bool,
) -> Result<bool> {
    if !interactive {
        return Ok(true);
    }

    println!("{}", picker_override_resume_warning(reason));
    Confirm::new()
        .with_prompt(
            style(concat!(
                "Resume auto-routing and switch back to the configured output?\n",
                "If you say no, the daemon will remain paused."
            ))
            .cyan()
            .to_string(),
        )
        .default(false)
        .interact()
        .map_err(anyhow::Error::new)
}

fn picker_override_resume_warning(reason: &DaemonPauseReason) -> String {
    match reason {
        DaemonPauseReason::PickerOverride {
            selected_label,
            preferred_uid,
            ..
        } => {
            let preferred = preferred_uid
                .as_deref()
                .map_or("the configured preferred output".into(), |uid| {
                    format!("configured preferred output `{uid}`")
                });
            format!(
                "The daemon is paused because you manually picked {selected_label}. Resuming auto-routing will switch back to {preferred}."
            )
        }
    }
}

fn picker_override_selected_label(reason: &DaemonPauseReason) -> &str {
    match reason {
        DaemonPauseReason::PickerOverride { selected_label, .. } => selected_label,
    }
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
                name: None,
                uid: Some("hdmi".into()),
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
        }
    }

    #[test]
    fn test_run_json_when_not_installed() {
        let json = serde_json::to_string(&ResumeResult::NotInstalled {
            plist_path: "/tmp/com.example.rusty-jack.plist".into(),
        })
        .unwrap();
        assert!(json.contains("\"status\":\"not_installed\""));
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

    #[test]
    fn test_picker_override_resume_warning_mentions_manual_pick() {
        let reason = DaemonPauseReason::picker_override(
            "line-out".into(),
            "Line Out".into(),
            Some("hdmi".into()),
        );

        let warning = picker_override_resume_warning(&reason);

        assert!(warning.contains("manually picked Line Out"));
        assert!(warning.contains("configured preferred output `hdmi`"));
    }

    #[test]
    fn test_picker_override_selected_label() {
        let reason = DaemonPauseReason::picker_override("line-out".into(), "Line Out".into(), None);

        assert_eq!(picker_override_selected_label(&reason), "Line Out");
    }
}
