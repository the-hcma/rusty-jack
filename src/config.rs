//! JSON configuration load and path resolution.

use crate::device_select::DeviceSelector;
use crate::RustyJackError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

const ENV_CONFIG: &str = "RUSTY_JACK_CONFIG";
const ENV_CONFIG_LEGACY: &str = "HDMI_SOUND_CONTROLLER_CONFIG";

const DEFAULT_SCALAR_WEBAPI_DEVICE_PORT: u16 = 10000;
const DEFAULT_SCALAR_WEBAPI_DEVICE_PATH: &str = concat!("so", "ny");

/// Pick a device by CoreAudio UID, with an optional human-readable device name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct DeviceSelectorConfig {
    /// Human-readable device label. This is emitted for readability; `uid` is
    /// the stable selector.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
}

impl From<DeviceSelectorConfig> for DeviceSelector {
    fn from(value: DeviceSelectorConfig) -> Self {
        Self { uid: value.uid }
    }
}

/// ScalarWebAPI wake-on-activity settings. Omit when not used.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScalarWebApiDeviceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_scalar_webapi_device_model")]
    pub model: String,
    /// Hostname, FQDN, or IP address (e.g. `scalarwebapi-device.local` or `192.168.1.42`).
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default = "default_scalar_webapi_device_port")]
    pub port: u16,
    #[serde(default = "default_scalar_webapi_device_path")]
    pub path: String,
    #[serde(default)]
    pub mac_output: DeviceSelectorConfig,
    #[serde(default = "default_scalar_webapi_device_triggers")]
    pub triggers: Vec<String>,
    #[serde(default = "default_wake_debounce_ms")]
    pub wake_debounce_ms: u64,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    #[serde(default = "default_require_quick_start")]
    pub require_quick_start: bool,
    /// When true, send a Wake-on-LAN packet before/alongside ScalarWebAPI `setPowerStatus`.
    #[serde(default)]
    pub wake_on_lan: bool,
}

fn default_scalar_webapi_device_model() -> String {
    "ScalarWebAPI device".into()
}

fn default_scalar_webapi_device_port() -> u16 {
    DEFAULT_SCALAR_WEBAPI_DEVICE_PORT
}

fn default_scalar_webapi_device_path() -> String {
    DEFAULT_SCALAR_WEBAPI_DEVICE_PATH.into()
}

fn default_scalar_webapi_device_triggers() -> Vec<String> {
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

impl ScalarWebApiDeviceConfig {
    /// ScalarWebAPI base URL built from host, port, and path.
    #[must_use]
    pub fn endpoint_url(&self) -> Option<String> {
        let host = self.host.as_deref()?.trim();
        if host.is_empty() {
            return None;
        }
        let path = self.path.trim().trim_start_matches('/');
        if path.is_empty() {
            Some(format!("http://{host}:{}", self.port))
        } else {
            Some(format!("http://{host}:{}/{path}", self.port))
        }
    }
}

/// Reserved logging settings (used by the daemon).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_file")]
    pub file: String,
}

fn default_log_level() -> String {
    "info".into()
}

fn default_log_file() -> String {
    "~/Library/Logs/rusty-jack.log".into()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file: default_log_file(),
        }
    }
}

/// User configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default = "default_auto_switch")]
    pub auto_switch: bool,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_switch_delay_ms")]
    pub switch_delay_ms: u64,
    #[serde(default = "default_activity_idle_threshold_ms")]
    pub activity_idle_threshold_ms: u64,
    #[serde(default = "default_activity_poll_interval_ms")]
    pub activity_poll_interval_ms: u64,
    /// Activity source for ScalarWebAPI wake triggers: `idle` (default) or `event_tap`.
    #[serde(default = "default_activity_monitor")]
    pub activity_monitor: String,
    #[serde(default)]
    pub preferred_device: DeviceSelectorConfig,
    /// Legacy field; use `preferred_device.uid` instead.
    #[serde(default)]
    pub preferred_device_uid: Option<String>,
    #[serde(default)]
    pub fallback_uids: Vec<String>,
    #[serde(default = "default_also_set_system_output")]
    pub also_set_system_output: bool,
    /// Preferred output volume (0–100). Non-preferred outputs use per-device volume memory.
    #[serde(default)]
    pub volume: Option<u8>,
    #[serde(default)]
    pub scalar_webapi_device: Option<ScalarWebApiDeviceConfig>,
    #[serde(default)]
    pub logging: LoggingConfig,
}

fn default_also_set_system_output() -> bool {
    true
}

fn default_auto_switch() -> bool {
    true
}

fn default_poll_interval_ms() -> u64 {
    3_000
}

fn default_switch_delay_ms() -> u64 {
    500
}

fn default_activity_idle_threshold_ms() -> u64 {
    60_000
}

fn default_activity_monitor() -> String {
    "idle".into()
}

fn default_activity_poll_interval_ms() -> u64 {
    1_000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            auto_switch: default_auto_switch(),
            poll_interval_ms: default_poll_interval_ms(),
            switch_delay_ms: default_switch_delay_ms(),
            activity_idle_threshold_ms: default_activity_idle_threshold_ms(),
            activity_poll_interval_ms: default_activity_poll_interval_ms(),
            activity_monitor: default_activity_monitor(),
            preferred_device: DeviceSelectorConfig::default(),
            preferred_device_uid: None,
            fallback_uids: Vec::new(),
            also_set_system_output: default_also_set_system_output(),
            volume: None,
            scalar_webapi_device: None,
            logging: LoggingConfig::default(),
        }
    }
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
        {
            return self.preferred_device.clone().into();
        }

        if let Some(uid) = &self.preferred_device_uid {
            return DeviceSelector {
                uid: Some(uid.clone()),
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
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| RustyJackError::Config(format!("{}: {err}", path.display())))?;
    let config: Config = serde_json::from_value(value.clone())
        .map_err(|err| RustyJackError::Config(format!("{}: {err}", path.display())))?;
    validate_config(&config)?;
    rewrite_config_if_needed(path, &raw, &value)?;
    Ok(config)
}

/// Render JSON with all object keys sorted lexicographically at every level.
pub fn render_lexicographic_json(value: &Value) -> Result<String, RustyJackError> {
    let mut value = value.clone();
    sort_json_keys(&mut value);
    serde_json::to_string_pretty(&value)
        .map(|json| format!("{json}\n"))
        .map_err(|err| RustyJackError::Config(format!("could not render config: {err}")))
}

fn rewrite_config_if_needed(path: &Path, raw: &str, value: &Value) -> Result<(), RustyJackError> {
    let canonical = render_lexicographic_json(value)?;
    if raw != canonical {
        atomic_write(path, &canonical)?;
    }
    Ok(())
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), RustyJackError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    let file_name = path.file_name().ok_or_else(|| {
        RustyJackError::Config(format!("config path has no file name: {}", path.display()))
    })?;
    let temp_path = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
    {
        let mut file = std::fs::File::create(&temp_path).map_err(RustyJackError::Io)?;
        file.write_all(contents.as_bytes())
            .map_err(RustyJackError::Io)?;
        file.sync_all().map_err(RustyJackError::Io)?;
    }
    std::fs::rename(&temp_path, path).map_err(RustyJackError::Io)?;
    Ok(())
}

fn sort_json_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let mut entries = std::mem::take(map).into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut sorted = Map::new();
            for (key, mut value) in entries {
                sort_json_keys(&mut value);
                sorted.insert(key, value);
            }
            *map = sorted;
        }
        Value::Array(values) => {
            for value in values {
                sort_json_keys(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Load config when present; returns `None` if the file does not exist.
///
/// # Errors
///
/// Returns an error when `explicit` is true and the file is missing, or when the file exists but is invalid.
pub fn load_config_optional(path: &Path, explicit: bool) -> Result<Option<Config>, RustyJackError> {
    match load_config(path) {
        Ok(config) => Ok(Some(config)),
        Err(RustyJackError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            if explicit {
                Err(RustyJackError::Config(format!(
                    "config file not found: {}",
                    path.display()
                )))
            } else {
                Ok(None)
            }
        }
        Err(err) => Err(err),
    }
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
            "set preferred_device.uid (see config.example.json)".into(),
        ));
    }

    if let Some(volume) = config.volume {
        if volume > 100 {
            return Err(RustyJackError::Config(
                "volume must be between 0 and 100".into(),
            ));
        }
    }

    if let Some(api) = &config.scalar_webapi_device {
        if api.enabled {
            let host = api.host.as_deref().unwrap_or("").trim();
            if host.is_empty() {
                return Err(RustyJackError::Config(
                    "scalar_webapi_device.enabled is true but host is not set".into(),
                ));
            }
            if api.mac_output.uid.as_deref().is_none_or(is_placeholder_uid) {
                return Err(RustyJackError::Config(
                    "scalar_webapi_device.enabled is true but mac_output is not set".into(),
                ));
            }
        }
    }

    if config.poll_interval_ms == 0 {
        return Err(RustyJackError::Config(
            "poll_interval_ms must be greater than 0 until event listeners are implemented".into(),
        ));
    }
    if config.activity_poll_interval_ms == 0 {
        return Err(RustyJackError::Config(
            "activity_poll_interval_ms must be greater than 0".into(),
        ));
    }
    if config.activity_idle_threshold_ms == 0 {
        return Err(RustyJackError::Config(
            "activity_idle_threshold_ms must be greater than 0".into(),
        ));
    }
    if !matches!(config.activity_monitor.as_str(), "idle" | "event_tap") {
        return Err(RustyJackError::Config(
            "activity_monitor must be \"idle\" or \"event_tap\"".into(),
        ));
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
    fn test_load_config_by_device_name_and_uid() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "name": "External Headphones", "uid": "BuiltInHeadphoneOutputDevice" }}
}}"#
        )
        .unwrap();

        let config = load_config(file.path()).unwrap();
        assert_eq!(
            config.preferred_device.name.as_deref(),
            Some("External Headphones")
        );
        assert_eq!(
            config.preferred_selector().uid.as_deref(),
            Some("BuiltInHeadphoneOutputDevice")
        );
        assert_eq!(config.poll_interval_ms, 3_000);
        assert_eq!(config.switch_delay_ms, 500);
        assert_eq!(config.activity_idle_threshold_ms, 60_000);
        assert_eq!(config.activity_poll_interval_ms, 1_000);
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
    fn test_scalar_webapi_device_endpoint_url_from_hostname() {
        let protocol_path = default_scalar_webapi_device_path();
        let api = ScalarWebApiDeviceConfig {
            enabled: true,
            model: "ScalarWebAPI device".into(),
            host: Some("scalarwebapi-device.local".into()),
            port: 10_000,
            path: protocol_path.clone(),
            mac_output: DeviceSelectorConfig::default(),
            triggers: default_scalar_webapi_device_triggers(),
            wake_debounce_ms: 30_000,
            request_timeout_ms: 3_000,
            require_quick_start: true,
            wake_on_lan: false,
        };
        let expected_url = format!("http://scalarwebapi-device.local:10000/{protocol_path}");
        assert_eq!(api.endpoint_url().as_deref(), Some(expected_url.as_str()));
    }

    #[test]
    fn test_scalar_webapi_device_disabled_without_host_ok() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }},
  "scalar_webapi_device": {{ "enabled": false }}
}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_ok());
    }

    #[test]
    fn test_scalar_webapi_device_enabled_requires_host() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }},
  "scalar_webapi_device": {{ "enabled": true, "mac_output": {{ "name": "Built-in" }} }}
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
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }},
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
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }},
  "volume": 13
}}"#
        )
        .unwrap();
        let config = load_config(file.path()).unwrap();
        assert_eq!(config.volume, Some(13));
    }

    #[test]
    fn test_load_config_with_logging_section() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }},
  "logging": {{
    "file": "/tmp/rusty-jack-test.log",
    "level": "debug"
  }}
}}"#
        )
        .unwrap();
        let config = load_config(file.path()).unwrap();
        assert_eq!(config.logging.file, "/tmp/rusty-jack-test.log");
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_load_config_defaults_logging_when_omitted() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }}
}}"#
        )
        .unwrap();
        let config = load_config(file.path()).unwrap();
        assert_eq!(config.logging, LoggingConfig::default());
    }

    #[test]
    fn test_load_config_rewrites_keys_lexicographically() {
        let mut file = NamedTempFile::new().unwrap();
        let protocol_path = default_scalar_webapi_device_path();
        write!(
            file,
            r#"{{
  "version": 1,
  "volume": 80,
  "scalar_webapi_device": {{
    "wake_debounce_ms": 2000,
    "triggers": ["output_selected"],
    "request_timeout_ms": 3000,
    "require_quick_start": true,
    "path": "{protocol_path}",
    "model": "ScalarWebAPI device",
    "mac_output": {{
      "uid": "BuiltInHeadphoneOutputDevice"
    }},
    "host": "scalarwebapi-device.local",
    "enabled": true
  }},
  "preferred_device": {{
    "uid": "BuiltInHeadphoneOutputDevice"
  }},
  "poll_interval_ms": 2000,
  "fallback_uids": [],
  "auto_switch": true,
  "also_set_system_output": true
}}"#
        )
        .unwrap();

        let config = load_config(file.path()).unwrap();
        let expected = format!(
            r#"{{
  "also_set_system_output": true,
  "auto_switch": true,
  "fallback_uids": [],
  "poll_interval_ms": 2000,
  "preferred_device": {{
    "uid": "BuiltInHeadphoneOutputDevice"
  }},
  "scalar_webapi_device": {{
    "enabled": true,
    "host": "scalarwebapi-device.local",
    "mac_output": {{
      "uid": "BuiltInHeadphoneOutputDevice"
    }},
    "model": "ScalarWebAPI device",
    "path": "{protocol_path}",
    "request_timeout_ms": 3000,
    "require_quick_start": true,
    "triggers": [
      "output_selected"
    ],
    "wake_debounce_ms": 2000
  }},
  "version": 1,
  "volume": 80
}}
"#
        );

        assert_eq!(config.volume, Some(80));
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), expected);
    }

    #[test]
    fn test_render_lexicographic_json_sorts_nested_objects() {
        let value = serde_json::json!({
            "z": 1,
            "a": {
                "d": 4,
                "b": 2
            }
        });

        assert_eq!(
            render_lexicographic_json(&value).unwrap(),
            r#"{
  "a": {
    "b": 2,
    "d": 4
  },
  "z": 1
}
"#
        );
    }

    #[test]
    fn test_rejects_zero_poll_interval() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "poll_interval_ms": 0,
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }}
}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_err());
    }

    #[test]
    fn test_rejects_zero_activity_interval() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "activity_poll_interval_ms": 0,
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }}
}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_err());
    }

    #[test]
    fn test_rejects_zero_activity_threshold() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"{{
  "version": 1,
  "activity_idle_threshold_ms": 0,
  "preferred_device": {{ "uid": "BuiltInHeadphoneOutputDevice" }}
}}"#
        )
        .unwrap();
        assert!(load_config(file.path()).is_err());
    }

    #[test]
    fn test_rejects_missing_preferred() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, r#"{{"version": 1}}"#).unwrap();
        assert!(load_config(file.path()).is_err());
    }
}
