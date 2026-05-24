//! JSON configuration load and path resolution.

use crate::RustyJackError;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const ENV_CONFIG: &str = "RUSTY_JACK_CONFIG";
const ENV_CONFIG_LEGACY: &str = "HDMI_SOUND_CONTROLLER_CONFIG";

/// User configuration (subset used by `status`; expanded in Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default = "default_auto_switch")]
    pub auto_switch: bool,
    pub preferred_device_uid: String,
    #[serde(default)]
    pub fallback_uids: Vec<String>,
}

fn default_auto_switch() -> bool {
    true
}

impl Config {
    /// True when `preferred_device_uid` is unset or still a template placeholder.
    #[must_use]
    pub fn preferred_uid_is_placeholder(&self) -> bool {
        is_placeholder_uid(&self.preferred_device_uid)
    }
}

#[must_use]
pub fn is_placeholder_uid(uid: &str) -> bool {
    let uid = uid.trim();
    uid.is_empty() || uid.contains("PASTE-UID") || uid.contains("PASTE-LINE-OUT")
}

/// Resolve config path: CLI flag → env → default file.
#[must_use]
pub fn resolve_config_path(cli_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = cli_path {
        return Some(path.to_path_buf());
    }
    if let Ok(path) = std::env::var(ENV_CONFIG) {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    if let Ok(path) = std::env::var(ENV_CONFIG_LEGACY) {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    default_config_path()
}

#[must_use]
pub fn default_config_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".config/rusty-jack/config.json"))
}

/// Load config from disk.
///
/// # Errors
///
/// Returns an error when the file exists but cannot be read or parsed.
pub fn load_config(path: &Path) -> Result<Config, RustyJackError> {
    let raw = std::fs::read_to_string(path).map_err(RustyJackError::Io)?;
    let config: Config = serde_json::from_str(&raw).map_err(|err| {
        RustyJackError::Config(format!("{}: {err}", path.display()))
    })?;
    validate_config(&config)?;
    Ok(config)
}

/// Load config when present; returns `None` if the file does not exist.
///
/// # Errors
///
/// Returns an error when `explicit` is true and the file is missing, or when the file exists but is invalid.
pub fn load_config_optional(path: &Path, explicit: bool) -> Result<Option<Config>, RustyJackError> {
    if !path.exists() {
        if explicit {
            return Err(RustyJackError::Config(format!(
                "config file not found: {}",
                path.display()
            )));
        }
        return Ok(None);
    }
    load_config(path).map(Some)
}

fn validate_config(config: &Config) -> Result<(), RustyJackError> {
    if config.version != 1 {
        return Err(RustyJackError::Config(format!(
            "unsupported config version {} (expected 1)",
            config.version
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_is_placeholder_uid() {
        assert!(is_placeholder_uid("PASTE-UID-FROM-rusty-jack-list"));
        assert!(is_placeholder_uid(""));
        assert!(!is_placeholder_uid("AppleHDAEngineOutput:1B,0,1,1:0"));
    }

    #[test]
    fn test_load_config_round_trip() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "auto_switch": true,
  "preferred_device_uid": "hdmi-uid",
  "fallback_uids": ["dp-uid"]
}}"#
        )
        .unwrap();

        let config = load_config(file.path()).unwrap();
        assert_eq!(config.preferred_device_uid, "hdmi-uid");
        assert!(config.auto_switch);
        assert_eq!(config.fallback_uids, vec!["dp-uid".to_string()]);
    }

    #[test]
    fn test_load_config_rejects_unsupported_version() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{"version": 2, "preferred_device_uid": "x"}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_err());
    }

    #[test]
    fn test_load_config_optional_missing_not_explicit() {
        let path = PathBuf::from("/tmp/rusty-jack-nonexistent-config-test.json");
        assert_eq!(load_config_optional(&path, false).unwrap(), None);
    }
}
