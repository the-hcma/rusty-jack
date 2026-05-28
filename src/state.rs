//! Persist small bits of user state that should survive config changes.

use crate::RustyJackError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreInstallDefault {
    pub output_device_uid: String,
    pub saved_at_unix_seconds: u64,
}

fn state_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".local/state/rusty-jack")
            .join("pre-install-default.json"),
    )
}

pub fn load_pre_install_default() -> Result<Option<PreInstallDefault>, RustyJackError> {
    let Some(path) = state_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(RustyJackError::Io)?;
    serde_json::from_str::<PreInstallDefault>(&raw)
        .map(Some)
        .map_err(|err| RustyJackError::Config(format!("pre-install default JSON: {err}")))
}

pub fn remember_pre_install_default_if_missing(uid: &str) -> Result<bool, RustyJackError> {
    let Some(path) = state_path() else {
        return Ok(false);
    };
    if path.exists() {
        return Ok(false);
    }
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
    std::fs::write(path, raw).map_err(RustyJackError::Io)?;
    Ok(true)
}

pub fn clear_pre_install_default() -> Result<bool, RustyJackError> {
    let Some(path) = state_path() else {
        return Ok(false);
    };
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path).map_err(RustyJackError::Io)?;
    Ok(true)
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
}
