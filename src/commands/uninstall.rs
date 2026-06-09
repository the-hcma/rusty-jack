//! `rusty-jack uninstall` — remove LaunchAgent, driver, and optionally config.

use crate::coreaudio;
use crate::launchd::{print_disable_result, uninstall_daemon};
use crate::logging::{print_log_purge_result, purge_daemon_logs, LogPurgeResult};
use crate::native_driver::{
    print_uninstall_result as print_driver_uninstall_result, uninstall_if_installed,
};
use crate::setup::{
    maybe_remove_default_config, print_config_removal_result, terminal_is_interactive,
    ConfigRemovalMode,
};
use crate::RustyJackError;
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Stop the daemon, remove the LaunchAgent plist, and optionally remove driver/config.
#[allow(clippy::too_many_arguments)]
pub fn run(
    json: bool,
    remove_config: bool,
    keep_config: bool,
    only_driver: bool,
    no_restore_audio: bool,
    purge_logs: bool,
    purge: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    let interactive = !json && terminal_is_interactive();
    let remove_config = remove_config || purge;
    let purge_logs = purge_logs || purge || remove_config;
    if only_driver {
        let native_driver = uninstall_if_installed(interactive).map_err(anyhow::Error::new)?;
        if json {
            let value = serde_json::to_string_pretty(&serde_json::json!({
                "native_driver": native_driver,
            }))?;
            println!("{value}");
        } else {
            print_driver_uninstall_result(&native_driver);
        }
        return Ok(());
    }

    let daemon = uninstall_daemon().map_err(anyhow::Error::new)?;
    let native_driver = uninstall_if_installed(interactive).map_err(anyhow::Error::new)?;
    let restore = if no_restore_audio {
        RestoreAudioResult::Skipped {
            reason: "disabled by --no-restore-audio".into(),
        }
    } else {
        restore_audio_best_effort().unwrap_or_else(|err| RestoreAudioResult::Error {
            message: err.to_string(),
        })
    };
    let mode = if remove_config {
        ConfigRemovalMode::Remove
    } else if keep_config || json {
        ConfigRemovalMode::Keep
    } else {
        ConfigRemovalMode::Prompt
    };
    let config = maybe_remove_default_config(mode, interactive).map_err(anyhow::Error::new)?;
    let logs = if purge_logs {
        purge_daemon_logs(config_path).unwrap_or_else(|err| LogPurgeResult {
            removed: Vec::new(),
            missing: Vec::new(),
            errors: vec![(PathBuf::from("<logs>"), err.to_string())],
        })
    } else {
        LogPurgeResult {
            removed: Vec::new(),
            missing: Vec::new(),
            errors: Vec::new(),
        }
    };

    if json {
        let value = serde_json::to_string_pretty(&serde_json::json!({
            "daemon": daemon,
            "native_driver": native_driver,
            "restore_audio": restore,
            "config": config,
            "logs": logs,
        }))?;
        println!("{value}");
    } else {
        print_disable_result(&daemon);
        print_driver_uninstall_result(&native_driver);
        print_restore_audio_result(&restore);
        print_config_removal_result(&config);
        print_log_purge_result(&logs);
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RestoreAudioResult {
    Restored { uid: String },
    NotFound,
    Skipped { reason: String },
    Failed { uid: String, message: String },
    Error { message: String },
}

fn restore_audio_best_effort() -> Result<RestoreAudioResult, RustyJackError> {
    let saved = crate::state::load_pre_install_default()?;
    let Some(saved) = saved else {
        return Ok(RestoreAudioResult::NotFound);
    };
    let uid = saved.output_device_uid;
    let hal =
        coreaudio::platform_hal().map_err(|err| RustyJackError::CoreAudio(err.to_string()))?;
    match hal.set_default_output(&uid, true) {
        Ok(()) => {
            let _ = crate::state::clear_pre_install_default();
            Ok(RestoreAudioResult::Restored { uid })
        }
        Err(err) => Ok(RestoreAudioResult::Failed {
            uid,
            message: err.to_string(),
        }),
    }
}

fn print_restore_audio_result(result: &RestoreAudioResult) {
    match result {
        RestoreAudioResult::Restored { uid } => {
            println!("Restored default output");
            println!("  uid: {uid}");
        }
        RestoreAudioResult::NotFound => {}
        RestoreAudioResult::Skipped { reason } => {
            println!("Skipped restoring default output");
            println!("  reason: {reason}");
        }
        RestoreAudioResult::Failed { uid, message } => {
            eprintln!("Warning: failed to restore default output");
            eprintln!("  uid: {uid}");
            eprintln!("  error: {message}");
        }
        RestoreAudioResult::Error { message } => {
            eprintln!("Warning: failed to restore default output");
            eprintln!("  error: {message}");
        }
    }
}
