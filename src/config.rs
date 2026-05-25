//! JSON configuration load and path resolution.

use crate::device_select::DeviceSelector;
use crate::RustyJackError;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const ENV_CONFIG: &str = "RUSTY_JACK_CONFIG";
const ENV_CONFIG_LEGACY: &str = "HDMI_SOUND_CONTROLLER_CONFIG";

const DEFAULT_SONY_PORT: u16 = 10000;
const DEFAULT_SONY_PATH: &str = "sony";

/// Pick a device by monitor product name and/or CoreAudio UID.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct DeviceSelectorConfig {
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub monitor_name: Option<String>,
}

impl From<DeviceSelectorConfig> for DeviceSelector {
    fn from(value: DeviceSelectorConfig) -> Self {
        Self {
            uid: value.uid,
            monitor_name: value.monitor_name,
        }
    }
}

/// Sony SRS-ZR5 wake-on-activity settings (Phase 8). Omit when not used.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SonySpeakerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_sony_model")]
    pub model: String,
    /// Hostname, FQDN, or IP address (e.g. `sony.house.hcma` or `192.168.1.42`).
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default = "default_sony_port")]
    pub port: u16,
    #[serde(default = "default_sony_path")]
    pub path: String,
    #[serde(default)]
    pub mac_output: DeviceSelectorConfig,
    #[serde(default = "default_sony_triggers")]
    pub triggers: Vec<String>,
    #[serde(default = "default_wake_debounce_ms")]
    pub wake_debounce_ms: u64,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    #[serde(default = "default_require_quick_start")]
    pub require_quick_start: bool,
}

fn default_sony_model() -> String {
    "SRS-ZR5".into()
}

fn default_sony_port() -> u16 {
    DEFAULT_SONY_PORT
}

fn default_sony_path() -> String {
    DEFAULT_SONY_PATH.into()
}

fn default_sony_triggers() -> Vec<String> {
    vec!["keyboard".into(), "mouse".into(), "output_selected".into()]
}

fn default_wake_debounce_ms() -> u64 {
    30_000
}

fn default_request_timeout_ms() -> u64 {
    3_000
}

fn default_require_quick_start() -> bool {
    true
}

impl SonySpeakerConfig {
    /// ScalarWebAPI base URL built from host, port, and path.
    #[must_use]
    pub fn endpoint_url(&self) -> Option<String> {
        let host = self.host.as_deref()?.trim();
        if host.is_empty() {
            return None;
        }
        let path = self.path.trim().trim_start_matches('/');
        Some(format!("http://{host}:{}:{path}", self.port))
    }
}

/// User configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default = "default_auto_switch")]
    pub auto_switch: bool,
    #[serde(default)]
    pub preferred_device: DeviceSelectorConfig,
    /// Legacy field; use `preferred_device.uid` instead.
    #[serde(default)]
    pub preferred_device_uid: Option<String>,
    #[serde(default)]
    pub fallback_uids: Vec<String>,
    #[serde(default = "default_also_set_system_output")]
    pub also_set_system_output: bool,
    /// Output volume (0–100) to apply when switching to the preferred device. Omitted = leave volume unchanged.
    #[serde(default)]
    pub volume: Option<u8>,
    #[serde(default)]
    pub sony_speaker: Option<SonySpeakerConfig>,
}

fn default_also_set_system_output() -> bool {
    true
}

fn default_auto_switch() -> bool {
    true
}

impl Config {
    /// Effective preferred device selector (`preferred_device` wins over legacy UID).
    #[must_use]
    pub fn preferred_selector(&self) -> DeviceSelector {
        if !self
            .preferred_device
            .uid
            .as_deref()
            .is_none_or(is_placeholder_uid)
            || self
                .preferred_device
                .monitor_name
                .as_deref()
                .is_some_and(|n| !n.trim().is_empty())
        {
            return self.preferred_device.clone().into();
        }

        if let Some(uid) = &self.preferred_device_uid {
            return DeviceSelector {
                uid: Some(uid.clone()),
                monitor_name: None,
            };
        }

        DeviceSelector::default()
    }

    #[must_use]
    pub fn preferred_is_set(&self) -> bool {
        !self.preferred_selector().is_empty()
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
    let config: Config = serde_json::from_str(&raw)
        .map_err(|err| RustyJackError::Config(format!("{}: {err}", path.display())))?;
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

    if !config.preferred_is_set() {
        return Err(RustyJackError::Config(
            "set preferred_device.monitor_name or preferred_device.uid (see config.example.json)"
                .into(),
        ));
    }

    if let Some(volume) = config.volume {
        if volume > 100 {
            return Err(RustyJackError::Config(
                "volume must be between 0 and 100".into(),
            ));
        }
    }

    if let Some(sony) = &config.sony_speaker {
        if sony.enabled {
            let host = sony.host.as_deref().unwrap_or("").trim();
            if host.is_empty() {
                return Err(RustyJackError::Config(
                    "sony_speaker.enabled is true but host is not set".into(),
                ));
            }
            if sony
                .mac_output
                .uid
                .as_deref()
                .is_none_or(is_placeholder_uid)
                && sony
                    .mac_output
                    .monitor_name
                    .as_deref()
                    .is_none_or(|n| n.trim().is_empty())
            {
                return Err(RustyJackError::Config(
                    "sony_speaker.enabled is true but mac_output is not set".into(),
                ));
            }
        }
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
    fn test_load_config_by_monitor_name() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "monitor_name": "DELL U3219Q" }}
}}"#
        )
        .unwrap();

        let config = load_config(file.path()).unwrap();
        assert_eq!(
            config.preferred_selector().monitor_name.as_deref(),
            Some("DELL U3219Q")
        );
    }

    #[test]
    fn test_legacy_preferred_device_uid() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device_uid": "hdmi-uid"
}}"#
        )
        .unwrap();

        let config = load_config(file.path()).unwrap();
        assert_eq!(config.preferred_selector().uid.as_deref(), Some("hdmi-uid"));
    }

    #[test]
    fn test_sony_endpoint_url_from_hostname() {
        let sony = SonySpeakerConfig {
            enabled: true,
            model: "SRS-ZR5".into(),
            host: Some("sony.house.hcma".into()),
            port: 10_000,
            path: "sony".into(),
            mac_output: DeviceSelectorConfig::default(),
            triggers: default_sony_triggers(),
            wake_debounce_ms: 30_000,
            request_timeout_ms: 3_000,
            require_quick_start: true,
        };
        assert_eq!(
            sony.endpoint_url().as_deref(),
            Some("http://sony.house.hcma:10000:sony")
        );
    }

    #[test]
    fn test_sony_disabled_without_host_ok() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "monitor_name": "DELL U3219Q" }},
  "sony_speaker": {{ "enabled": false }}
}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_ok());
    }

    #[test]
    fn test_sony_enabled_requires_host() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "monitor_name": "DELL U3219Q" }},
  "sony_speaker": {{ "enabled": true, "mac_output": {{ "monitor_name": "Built-in" }} }}
}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_err());
    }

    #[test]
    fn test_rejects_volume_out_of_range() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "monitor_name": "DELL U3219Q" }},
  "volume": 101
}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_err());
    }

    #[test]
    fn test_load_config_with_volume() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "monitor_name": "DELL U3219Q" }},
  "volume": 13
}}"#
        )
        .unwrap();
        let config = load_config(file.path()).unwrap();
        assert_eq!(config.volume, Some(13));
    }

    #[test]
    fn test_rejects_missing_preferred() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, r#"{{"version": 1}}"#).unwrap();
        assert!(load_config(file.path()).is_err());
    }
}
