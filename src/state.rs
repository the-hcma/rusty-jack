//! Persist small bits of user state that should survive config changes.

use crate::RustyJackError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreInstallDefault {
    pub output_device_uid: String,
    pub saved_at_unix_seconds: u64,
}

const ENV_STATE_DIR: &str = "RUSTY_JACK_STATE_DIR";

/// Latest daemon activity poll sample (for `rusty-jack status`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivitySnapshot {
    pub sampled_at_unix_seconds: u64,
    pub idle_seconds: f64,
    pub threshold_seconds: f64,
    pub is_idle: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_user: Option<String>,
    pub daemon_user: String,
    pub triggers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_became_active_at_unix_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_became_active_console_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_became_active_daemon_user: Option<String>,
}

fn state_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(ENV_STATE_DIR) {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    if cfg!(test) {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local/state/rusty-jack"))
}

fn pre_install_default_path() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("pre-install-default.json"))
}

fn activity_snapshot_path() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("activity-snapshot.json"))
}

pub fn load_pre_install_default() -> Result<Option<PreInstallDefault>, RustyJackError> {
    let Some(path) = pre_install_default_path() else {
        return Ok(None);
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(RustyJackError::Io(err)),
    };
    serde_json::from_str::<PreInstallDefault>(&raw)
        .map(Some)
        .map_err(|err| RustyJackError::Config(format!("pre-install default JSON: {err}")))
}

pub fn remember_pre_install_default_if_missing(uid: &str) -> Result<bool, RustyJackError> {
    let Some(path) = pre_install_default_path() else {
        return Ok(false);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    let saved_at_unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let value = PreInstallDefault {
        output_device_uid: uid.to_string(),
        saved_at_unix_seconds,
    };
    let raw = serde_json::to_string_pretty(&value)
        .map(|json| format!("{json}\n"))
        .map_err(|err| RustyJackError::Config(format!("pre-install default JSON: {err}")))?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(raw.as_bytes()).map_err(RustyJackError::Io)?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(RustyJackError::Io(err)),
    }
}

pub fn clear_pre_install_default() -> Result<bool, RustyJackError> {
    let Some(path) = pre_install_default_path() else {
        return Ok(false);
    };
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(RustyJackError::Io(err)),
    }
}

pub fn load_activity_snapshot() -> Result<Option<ActivitySnapshot>, RustyJackError> {
    let Some(path) = activity_snapshot_path() else {
        return Ok(None);
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(RustyJackError::Io(err)),
    };
    serde_json::from_str::<ActivitySnapshot>(&raw)
        .map(Some)
        .map_err(|err| RustyJackError::Config(format!("activity snapshot JSON: {err}")))
}

pub fn save_activity_snapshot(snapshot: &ActivitySnapshot) -> Result<(), RustyJackError> {
    let Some(path) = activity_snapshot_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    let raw = serde_json::to_string_pretty(snapshot)
        .map(|json| format!("{json}\n"))
        .map_err(|err| RustyJackError::Config(format!("activity snapshot JSON: {err}")))?;
    std::fs::write(path, raw).map_err(RustyJackError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pre_install_default_round_trip() {
        let value = PreInstallDefault {
            output_device_uid: "BuiltInSpeaker".into(),
            saved_at_unix_seconds: 1_234_567,
        };
        let raw = serde_json::to_string(&value).unwrap();
        let parsed: PreInstallDefault = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn test_activity_snapshot_round_trip() {
        let value = ActivitySnapshot {
            sampled_at_unix_seconds: 1_714_000_000,
            idle_seconds: 12.4,
            threshold_seconds: 60.0,
            is_idle: false,
            console_user: Some("hcma".into()),
            daemon_user: "hcma".into(),
            triggers: vec!["keyboard".into(), "mouse".into()],
            last_became_active_at_unix_seconds: Some(1_713_999_000),
            last_became_active_console_user: Some("hcma".into()),
            last_became_active_daemon_user: Some("hcma".into()),
        };
        let raw = serde_json::to_string(&value).unwrap();
        let parsed: ActivitySnapshot = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed, value);
    }
}
