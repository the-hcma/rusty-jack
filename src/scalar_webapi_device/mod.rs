//! ScalarWebAPI wake support for external devices attached to a selected Mac output.

mod input;
mod install;

pub use input::{
    configured_speaker_input_label, configured_speaker_input_validation_error,
    current_scalar_webapi_speaker_input, ensure_scalar_webapi_speaker_input,
    format_speaker_input_ensure_message, list_scalar_webapi_speaker_inputs,
    list_scalar_webapi_speaker_inputs_with_feedback, speaker_input_uses_default,
    validate_configured_speaker_input, validate_speaker_input_name,
    validate_speaker_input_name_in_list, validate_speaker_input_name_with_feedback,
    ScalarWebApiSpeakerInput, ScalarWebApiSpeakerInputEnsureResult,
};
pub use install::{
    append_scalar_webapi_to_config_json, maybe_prompt_scalar_webapi_speaker_input,
    maybe_prompt_scalar_webapi_wake_triggers, prompt_add_scalar_webapi_device,
    prompt_scalar_webapi_host_selection, prompt_scalar_webapi_speaker_input,
    prompt_scalar_webapi_wake_triggers, ScalarWebApiInstallSelection,
};

/// A ScalarWebAPI-compatible device discovered on the local network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredScalarWebApiDevice {
    pub host: String,
    pub model: Option<String>,
}

use crate::config::{Config, ScalarWebApiDeviceConfig};
use crate::device_select::{display_label_for_selector, resolve_device_selector};
use crate::output_device::OutputDevice;
use crate::system_default::DeviceList;
use crate::RustyJackError;
use input::configured_speaker_input_name;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static LAST_SUCCESSFUL_WAKE_SENT: Mutex<Option<Instant>> = Mutex::new(None);

/// Return true when enough time has passed since the last successful `setPowerStatus`.
#[must_use]
pub fn wake_attempt_allowed(config: &Config) -> bool {
    let Some(api) = config.scalar_webapi_device.as_ref() else {
        return true;
    };
    wake_attempt_allowed_for_api(api)
}

fn wake_attempt_allowed_for_api(api: &ScalarWebApiDeviceConfig) -> bool {
    let cooldown = Duration::from_millis(api.wake_debounce_ms);
    LAST_SUCCESSFUL_WAKE_SENT
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_none_or(|last| last.elapsed() >= cooldown)
}

fn record_successful_wake_sent() {
    if let Ok(mut guard) = LAST_SUCCESSFUL_WAKE_SENT.lock() {
        *guard = Some(Instant::now());
    }
}

/// Clear the post-wake debounce (for example after network recovery).
pub fn reset_wake_debounce() {
    if let Ok(mut guard) = LAST_SUCCESSFUL_WAKE_SENT.lock() {
        *guard = None;
    }
}

#[cfg(test)]
fn clear_wake_debounce_for_tests() {
    reset_wake_debounce();
}
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::Message;

/// Wake when the configured Mac output is selected (`apply`, `picker`, daemon routing).
pub const OUTPUT_SELECTED_TRIGGER: &str = "output_selected";
/// Wake on daemon idle-to-active transitions (screen unlock, keyboard activity).
pub const KEYBOARD_TRIGGER: &str = "keyboard";
/// Wake on daemon idle-to-active transitions (screen unlock, pointer activity).
pub const MOUSE_TRIGGER: &str = "mouse";

/// Recommended install defaults: unlock/activity plus output selection.
pub const DEFAULT_WAKE_TRIGGERS: &[&str] =
    &[KEYBOARD_TRIGGER, MOUSE_TRIGGER, OUTPUT_SELECTED_TRIGGER];

/// Format configured ScalarWebAPI wake triggers for human-readable status output.
#[must_use]
pub fn format_scalar_webapi_triggers_for_display(
    triggers: &[String],
    mac_output_label: Option<&str>,
) -> String {
    let labels: Vec<String> = triggers
        .iter()
        .map(|trigger| human_readable_trigger_label(trigger, mac_output_label))
        .collect();
    format_readable_trigger_list(&labels)
}

pub(crate) fn human_readable_trigger_label(
    trigger: &str,
    mac_output_label: Option<&str>,
) -> String {
    match trigger.to_ascii_lowercase().as_str() {
        KEYBOARD_TRIGGER => "keyboard activity".into(),
        MOUSE_TRIGGER => "mouse/pointer activity".into(),
        OUTPUT_SELECTED_TRIGGER => mac_output_label
            .map(|name| format!("selecting {name}"))
            .unwrap_or_else(|| "output device selection".into()),
        other => other.to_string(),
    }
}

/// Default config `model` when install does not discover a friendlier label.
pub const GENERIC_SCALAR_WEBAPI_MODEL: &str = "ScalarWebAPI device";

/// ScalarWebAPI speaker linked to a Mac output in config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScalarWebApiMacOutputLink {
    pub mac_output_uid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_output_label: Option<String>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// Build the ScalarWebAPI link shown in `list` / `status` when enabled in config.
#[must_use]
pub fn scalar_webapi_mac_output_link(
    config: &Config,
    devices: &[OutputDevice],
) -> Option<ScalarWebApiMacOutputLink> {
    let api = config
        .scalar_webapi_device
        .as_ref()
        .filter(|api| api.enabled)?;
    let mac_output_uid = configured_mac_output_uid(api, devices)?;
    Some(ScalarWebApiMacOutputLink {
        mac_output_uid,
        mac_output_label: display_label_for_selector(&api.mac_output, devices),
        model: api.model.clone(),
        host: api.host.clone(),
    })
}

/// Attach ScalarWebAPI display metadata to a device list using config.
#[must_use]
pub fn attach_scalar_webapi_mac_output(
    mut list: DeviceList,
    config: Option<&Config>,
) -> DeviceList {
    list.scalar_webapi_mac_output =
        config.and_then(|config| scalar_webapi_mac_output_link(config, &list.devices));
    list
}

/// Short suffix for the linked Mac output row in device tables.
#[must_use]
pub fn format_scalar_webapi_device_column_suffix(link: &ScalarWebApiMacOutputLink) -> String {
    let host = link.host.as_deref().unwrap_or("").trim();
    if link.model == GENERIC_SCALAR_WEBAPI_MODEL || link.model.trim().is_empty() {
        if host.is_empty() {
            String::new()
        } else {
            format!(" — {host}")
        }
    } else if host.is_empty() {
        format!(" — {}", link.model)
    } else {
        format!(" — {} @ {host}", link.model)
    }
}

fn configured_mac_output_uid(
    api: &ScalarWebApiDeviceConfig,
    devices: &[OutputDevice],
) -> Option<String> {
    resolve_device_selector(&api.mac_output.clone().into(), devices)
        .ok()
        .or_else(|| {
            api.mac_output
                .uid
                .as_deref()
                .filter(|uid| !crate::config::is_placeholder_uid(uid))
                .map(str::to_string)
        })
}

fn format_readable_trigger_list(items: &[String]) -> String {
    match items.len() {
        0 => "(none)".into(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        n => {
            let head = items[..n - 1].join(", ");
            format!("{}, and {}", head, items[n - 1])
        }
    }
}

const SYSTEM_SERVICE: &str = "system";
pub(crate) const AV_CONTENT_SERVICE: &str = "avContent";
const SSDP_ADDR: &str = "239.255.255.250:1900";
const SCALAR_WEBAPI_ST: &str = concat!("urn:schemas-", "so", "ny", "-com:service:ScalarWebAPI:1");
const SSDP_PROBE_COUNT: u32 = 3;
const SSDP_PROBE_INTERVAL_MS: u64 = 400;
const SSDP_RECV_SLICE_MS: u64 = 250;
const ENDPOINT_CACHE_TTL: Duration = Duration::from_secs(300);
pub const DISPLAY_POWER_TIMEOUT_MS: u64 = 750;
const SCALAR_DISCOVERY_CACHE_FILE: &str = "scalar-discovery-cache.json";

#[derive(Debug, Clone)]
struct CachedScalarEndpoint {
    host_key: String,
    endpoint: ScalarWebApiDeviceEndpoint,
    speaker_model: Option<String>,
    cached_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedScalarEndpointOnDisk {
    endpoint: ScalarWebApiDeviceEndpoint,
    speaker_model: Option<String>,
    cached_at_unix_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ScalarDiscoveryCacheFile {
    hosts: BTreeMap<String, CachedScalarEndpointOnDisk>,
}

fn endpoint_cache() -> &'static Mutex<Option<CachedScalarEndpoint>> {
    static CACHE: Mutex<Option<CachedScalarEndpoint>> = Mutex::new(None);
    &CACHE
}

#[cfg(test)]
pub(crate) fn clear_scalar_webapi_endpoint_cache_for_tests() {
    let _lock = discovery_cache_test_lock();
    if let Ok(mut guard) = endpoint_cache().lock() {
        *guard = None;
    }
    let _ = std::fs::remove_file(scalar_discovery_cache_path());
}

#[cfg(test)]
fn discovery_cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarWebApiDeviceWakeResult {
    pub endpoint: String,
    pub status_code: u16,
    pub previous_status: Option<String>,
    pub trigger: String,
    pub activity_event: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ScalarWebApiDeviceEndpoint {
    host: String,
    port: u16,
    path: String,
}

impl ScalarWebApiDeviceEndpoint {
    fn base_url(&self) -> String {
        let path = self.path.trim_end_matches('/');
        if path.is_empty() {
            format!("http://{}:{}", self.host, self.port)
        } else {
            format!("http://{}:{}{path}", self.host, self.port)
        }
    }

    fn service_endpoint(&self, service: &str) -> String {
        format!("{}/{}", self.base_url(), service)
    }

    fn service_path(&self, service: &str) -> String {
        let path = self.path.trim_matches('/');
        if path.is_empty() {
            format!("/{service}")
        } else {
            format!("/{path}/{service}")
        }
    }
}

fn scalar_http_url(host: &str, port: u16, path: &str) -> String {
    format!("http://{host}:{port}{path}")
}

fn scalar_ssdp_url() -> String {
    format!("ssdp://{SSDP_ADDR}")
}

fn scalar_speaker_err(url: &str, detail: impl std::fmt::Display) -> RustyJackError {
    RustyJackError::Speaker(format!("url={url}: {detail}"))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn scalar_discovery_cache_path() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir().join(format!("rusty-jack/{SCALAR_DISCOVERY_CACHE_FILE}"))
    }

    #[cfg(not(test))]
    {
        crate::config::default_config_path()
            .and_then(|path| path.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(SCALAR_DISCOVERY_CACHE_FILE)
    }
}

fn load_scalar_discovery_cache_file() -> Option<ScalarDiscoveryCacheFile> {
    let cache_path = scalar_discovery_cache_path();
    let raw = std::fs::read_to_string(cache_path).ok()?;
    serde_json::from_str::<ScalarDiscoveryCacheFile>(&raw).ok()
}

fn write_scalar_discovery_cache_file(cache: &ScalarDiscoveryCacheFile) {
    let cache_path = scalar_discovery_cache_path();
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(contents) = serde_json::to_string_pretty(cache) else {
        return;
    };
    let _ = std::fs::write(cache_path, format!("{contents}\n"));
}

fn cache_entry_is_fresh(cached_at_unix_ms: u64) -> bool {
    now_unix_ms().saturating_sub(cached_at_unix_ms) <= ENDPOINT_CACHE_TTL.as_millis() as u64
}

fn load_scalar_endpoint_from_disk(host_key: &str) -> Option<CachedScalarEndpointOnDisk> {
    let entry = load_scalar_endpoint_from_disk_allow_stale(host_key)?;
    cache_entry_is_fresh(entry.cached_at_unix_ms).then_some(entry)
}

fn load_scalar_endpoint_from_disk_allow_stale(
    host_key: &str,
) -> Option<CachedScalarEndpointOnDisk> {
    let cache = load_scalar_discovery_cache_file()?;
    cache.hosts.get(host_key).cloned()
}

fn memory_cached_endpoint(host_key: &str) -> Option<ScalarWebApiDeviceEndpoint> {
    let guard = endpoint_cache().lock().ok()?;
    let cached = guard.as_ref()?;
    (cached.host_key == host_key && cached.cached_at.elapsed() < ENDPOINT_CACHE_TTL)
        .then(|| cached.endpoint.clone())
}

fn persist_scalar_endpoint_cache(
    host_key: &str,
    endpoint: &ScalarWebApiDeviceEndpoint,
    speaker_model: Option<String>,
) {
    if let Ok(mut guard) = endpoint_cache().lock() {
        *guard = Some(CachedScalarEndpoint {
            host_key: host_key.to_string(),
            endpoint: endpoint.clone(),
            speaker_model: speaker_model.clone(),
            cached_at: Instant::now(),
        });
    }

    let mut cache = load_scalar_discovery_cache_file().unwrap_or_default();
    cache.hosts.insert(
        host_key.to_string(),
        CachedScalarEndpointOnDisk {
            endpoint: endpoint.clone(),
            speaker_model,
            cached_at_unix_ms: now_unix_ms(),
        },
    );
    write_scalar_discovery_cache_file(&cache);
}

fn is_transient_network_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NetworkUnreachable
            | std::io::ErrorKind::HostUnreachable
            | std::io::ErrorKind::NotConnected
    ) || err.raw_os_error() == Some(65)
}

/// Return true when every recommended wake trigger is configured.
#[must_use]
pub fn has_all_default_wake_triggers(triggers: &[String]) -> bool {
    DEFAULT_WAKE_TRIGGERS
        .iter()
        .all(|trigger| trigger_enabled_slice(triggers, trigger))
}

fn trigger_enabled_slice(triggers: &[String], trigger: &str) -> bool {
    triggers
        .iter()
        .any(|value| value.eq_ignore_ascii_case(trigger))
}

/// Try waking the configured ScalarWebAPI device when its Mac output is selected.
pub fn wake_on_output_selected(
    config: &Config,
    devices: &[OutputDevice],
    selected_uid: &str,
) -> Result<Option<ScalarWebApiDeviceWakeResult>, RustyJackError> {
    let Some(api) = config.scalar_webapi_device.as_ref() else {
        return Ok(None);
    };
    if !api.enabled || !trigger_enabled(api, OUTPUT_SELECTED_TRIGGER) {
        return Ok(None);
    }

    let selector = api.mac_output.clone().into();
    let api_output_uid = resolve_device_selector(&selector, devices)
        .map_err(|err| RustyJackError::Config(format!("scalar_webapi_device.mac_output: {err}")))?;
    if api_output_uid != selected_uid {
        return Ok(None);
    }

    try_wake_scalar_webapi_device(api, OUTPUT_SELECTED_TRIGGER, true, None)
}

/// Log ScalarWebAPI wake failures as warnings so audio routing still succeeds.
pub fn warn_on_output_selected(config: &Config, devices: &[OutputDevice], selected_uid: &str) {
    match wake_on_output_selected(config, devices, selected_uid) {
        Ok(Some(result)) => eprintln!("{}", format_wake_message(&result)),
        Ok(None) => {}
        Err(err) => eprintln!("warning: {err}"),
    }
}

/// Try waking the configured ScalarWebAPI device when user input resumes on its Mac output.
pub fn wake_on_activity(
    config: &Config,
    devices: &[OutputDevice],
    active_uid: &str,
    activity_event: Option<&str>,
) -> Result<Option<ScalarWebApiDeviceWakeResult>, RustyJackError> {
    let Some(api) = config.scalar_webapi_device.as_ref() else {
        return Ok(None);
    };
    if !api.enabled || !activity_trigger_enabled(api) {
        return Ok(None);
    }

    let selector = api.mac_output.clone().into();
    let api_output_uid = resolve_device_selector(&selector, devices)
        .map_err(|err| RustyJackError::Config(format!("scalar_webapi_device.mac_output: {err}")))?;
    if api_output_uid != active_uid {
        return Ok(None);
    }

    let trigger = activity_trigger_for_wake(api, activity_event);
    try_wake_scalar_webapi_device(api, trigger, false, activity_event)
}

fn activity_trigger_for_wake(
    api: &ScalarWebApiDeviceConfig,
    activity_event: Option<&str>,
) -> &'static str {
    if let Some(event) = activity_event {
        let trigger = crate::activity::wake_trigger_for_activity_event(event);
        if trigger_enabled(api, trigger) {
            return trigger;
        }
    }
    activity_trigger_label(api)
}

fn activity_trigger_label(api: &ScalarWebApiDeviceConfig) -> &'static str {
    if trigger_enabled(api, KEYBOARD_TRIGGER) {
        KEYBOARD_TRIGGER
    } else {
        MOUSE_TRIGGER
    }
}

fn try_wake_scalar_webapi_device(
    api: &ScalarWebApiDeviceConfig,
    trigger: &str,
    allow_wake_without_power_status: bool,
    activity_event: Option<&str>,
) -> Result<Option<ScalarWebApiDeviceWakeResult>, RustyJackError> {
    if !crate::network::host_ready_for_scalar_webapi_wake(api.host.as_deref())? {
        return Ok(None);
    }

    let endpoint = match resolve_scalar_webapi_device_endpoint(api)? {
        Some(endpoint) => endpoint,
        None => {
            tracing::warn!(
                target: "daemon",
                "[scalar] no JSON-RPC endpoint for {}; skipping wake (SSDP miss and no usable cache/config port {})",
                scalar_webapi_device_host(api)?,
                api.port
            );
            return Ok(None);
        }
    };

    let previous_status = match current_power_status_at_endpoint(api, &endpoint) {
        Ok(status) => Some(status),
        Err(err) if allow_wake_without_power_status => {
            tracing::debug!(
                target: "daemon",
                "[scalar] power status unavailable at {}: {err}; attempting wake on {trigger}",
                endpoint.base_url()
            );
            None
        }
        Err(err) => {
            tracing::debug!(
                target: "daemon",
                "[scalar] power status unavailable at {}: {err}; skipping wake on {trigger}",
                endpoint.base_url()
            );
            return Ok(None);
        }
    };

    if previous_status
        .as_deref()
        .is_some_and(|status| status.eq_ignore_ascii_case("active"))
    {
        tracing::debug!(
            target: "daemon",
            "[scalar] device already active; skipping wake on {trigger}"
        );
        return finish_scalar_webapi_device_wake(api, trigger, None);
    }

    if !wake_attempt_allowed_for_api(api) {
        tracing::debug!(
            target: "daemon",
            "[scalar] skipping wake on {trigger}; setPowerStatus sent within last {} ms",
            api.wake_debounce_ms
        );
        return Ok(None);
    }

    let mut result = send_wake_command_to(api, &endpoint)?;
    record_successful_wake_sent();
    result.previous_status = previous_status;
    result.trigger = trigger.into();
    result.activity_event = activity_event.map(str::to_string);
    finish_scalar_webapi_device_wake(api, trigger, Some(result))
}

fn finish_scalar_webapi_device_wake(
    api: &ScalarWebApiDeviceConfig,
    trigger: &str,
    wake_result: Option<ScalarWebApiDeviceWakeResult>,
) -> Result<Option<ScalarWebApiDeviceWakeResult>, RustyJackError> {
    if let Some(input_result) = ensure_scalar_webapi_speaker_input(api, trigger)? {
        eprintln!("{}", format_speaker_input_ensure_message(&input_result));
    }
    Ok(wake_result)
}

/// Keep the configured speaker input when the Mac is routed to its output.
pub fn ensure_scalar_webapi_speaker_input_on_active_mac_output(
    config: &Config,
    devices: &[OutputDevice],
    active_uid: &str,
    trigger: &str,
) -> Result<Option<ScalarWebApiSpeakerInputEnsureResult>, RustyJackError> {
    let Some(api) = config.scalar_webapi_device.as_ref() else {
        return Ok(None);
    };
    if !api.enabled || configured_speaker_input_name(api).is_none() {
        return Ok(None);
    }
    let selector = api.mac_output.clone().into();
    let configured_uid = resolve_device_selector(&selector, devices)
        .map_err(|err| RustyJackError::Config(format!("scalar_webapi_device.mac_output: {err}")))?;
    if configured_uid != active_uid {
        return Ok(None);
    }
    if !crate::network::host_ready_for_scalar_webapi_wake(api.host.as_deref())? {
        return Ok(None);
    }
    ensure_scalar_webapi_speaker_input(api, trigger)
}

/// Log speaker-input enforcement failures without failing daemon routing.
pub fn warn_on_speaker_input_enforcement(
    config: &Config,
    devices: &[OutputDevice],
    active_uid: &str,
    trigger: &str,
) {
    match ensure_scalar_webapi_speaker_input_on_active_mac_output(
        config, devices, active_uid, trigger,
    ) {
        Ok(Some(result)) => eprintln!("{}", format_speaker_input_ensure_message(&result)),
        Ok(None) => {}
        Err(err) => eprintln!("warning: {err}"),
    }
}

/// Log activity-triggered ScalarWebAPI wake failures as warnings so daemon routing still succeeds.
pub fn warn_on_activity(
    config: &Config,
    devices: &[OutputDevice],
    active_uid: &str,
    activity_event: Option<&str>,
) {
    match wake_on_activity(config, devices, active_uid, activity_event) {
        Ok(Some(result)) => eprintln!("{}", format_wake_message(&result)),
        Ok(None) => {}
        Err(err) => eprintln!("warning: {err}"),
    }
}

#[must_use]
pub fn format_wake_message(result: &ScalarWebApiDeviceWakeResult) -> String {
    let trigger = human_readable_trigger_label(&result.trigger, None);
    let event_suffix = result
        .activity_event
        .as_deref()
        .map(|event| format!(" ({event})"))
        .unwrap_or_default();
    match result.previous_status.as_deref() {
        Some(status) if status.eq_ignore_ascii_case("standby") => format!(
            "ScalarWebAPI wake on {trigger}{event_suffix}: device was standby; sent setPowerStatus(active) via {}.",
            result.endpoint
        ),
        Some(status) => format!(
            "ScalarWebAPI wake on {trigger}{event_suffix}: power was {status}; sent setPowerStatus(active) via {}.",
            result.endpoint
        ),
        None => format!(
            "ScalarWebAPI wake on {trigger}{event_suffix}: power status unavailable; sent setPowerStatus(active) via {}.",
            result.endpoint
        ),
    }
}

/// Return whether the live speaker input matches config, when both are known.
#[must_use]
pub fn speaker_input_matches_config(api: &ScalarWebApiDeviceConfig) -> Option<bool> {
    let configured = configured_speaker_input_name(api)?;
    let active = current_scalar_webapi_speaker_input(api)?;
    Some(active.eq_ignore_ascii_case(&configured))
}
#[must_use]
pub fn picker_power_notes(config: &Config, devices: &[OutputDevice]) -> Vec<(String, String)> {
    let Some(api) = config.scalar_webapi_device.as_ref() else {
        return vec![];
    };
    if !api.enabled {
        return vec![];
    }

    let selector = api.mac_output.clone().into();
    let uid = match resolve_device_selector(&selector, devices) {
        Ok(uid) => uid,
        Err(err) => {
            eprintln!("warning: scalar_webapi_device.mac_output: {err}");
            return vec![];
        }
    };
    let note = with_scalar_probing_feedback(
        ScalarDiscoveryFeedback::Interactive,
        "  probing ScalarWebAPI speaker",
        || match (
            current_power_status_for_display(api),
            configured_speaker_input_label(api),
            current_scalar_webapi_speaker_input(api),
        ) {
            (Some(power), Some(configured_input), Some(active_input)) => {
                if active_input.eq_ignore_ascii_case(&configured_input) {
                    format!("ScalarWebAPI: {power}; input {configured_input}")
                } else {
                    format!(
                        "ScalarWebAPI: {power}; input {active_input} (expected {configured_input})"
                    )
                }
            }
            (Some(power), Some(configured_input), None) => {
                format!("ScalarWebAPI: {power}; expected input {configured_input}")
            }
            (Some(power), None, Some(active_input)) => {
                format!("ScalarWebAPI: {power}; input {active_input}")
            }
            (Some(power), None, None) => format!("ScalarWebAPI: {power}"),
            (None, Some(configured_input), _) => {
                format!("ScalarWebAPI: unknown; expected input {configured_input}")
            }
            (None, None, _) => "ScalarWebAPI: unknown".into(),
        },
    );

    vec![(uid, note)]
}

fn trigger_enabled(api: &ScalarWebApiDeviceConfig, trigger: &str) -> bool {
    trigger_enabled_slice(&api.triggers, trigger)
}

fn activity_trigger_enabled(api: &ScalarWebApiDeviceConfig) -> bool {
    trigger_enabled(api, KEYBOARD_TRIGGER) || trigger_enabled(api, MOUSE_TRIGGER)
}

pub fn current_power_status(api: &ScalarWebApiDeviceConfig) -> Result<String, RustyJackError> {
    let host = scalar_webapi_device_host(api)?;
    let endpoint = resolve_scalar_webapi_device_endpoint(api)?.ok_or_else(|| {
        scalar_speaker_err(
            &scalar_ssdp_url(),
            format!(
                "host={host}: no JSON-RPC endpoint (SSDP miss and no usable cache/config port {})",
                api.port
            ),
        )
    })?;
    current_power_status_at_endpoint(api, &endpoint)
}

pub fn current_power_status_for_display(api: &ScalarWebApiDeviceConfig) -> Option<String> {
    let endpoint = display_endpoint_for_api(api)?;
    let mut timeout_api = api.clone();
    timeout_api.request_timeout_ms = DISPLAY_POWER_TIMEOUT_MS;
    current_power_status_at_endpoint(&timeout_api, &endpoint).ok()
}

pub fn cached_speaker_model_for_display(api: &ScalarWebApiDeviceConfig) -> Option<String> {
    let host_key = scalar_webapi_device_host(api).ok()?.to_string();
    if let Ok(guard) = endpoint_cache().lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.host_key == host_key && cached.cached_at.elapsed() < ENDPOINT_CACHE_TTL {
                return cached.speaker_model.clone();
            }
        }
    }
    load_scalar_endpoint_from_disk(&host_key).and_then(|cached| cached.speaker_model)
}

pub fn should_show_distinct_speaker_model(config_model: &str, hardware: &str) -> bool {
    let config_model = config_model.trim();
    let hardware = hardware.trim();
    !hardware.is_empty()
        && !config_model.is_empty()
        && !hardware.eq_ignore_ascii_case(config_model)
        && !config_model.eq_ignore_ascii_case(GENERIC_SCALAR_WEBAPI_MODEL)
}

pub fn hardware_speaker_model_hint(location: &str, xml: &str) -> Option<String> {
    model_hint_from_location_url(location).or_else(|| model_hint_from_upnp_xml(xml))
}

pub(crate) fn display_endpoint_for_api(
    api: &ScalarWebApiDeviceConfig,
) -> Option<ScalarWebApiDeviceEndpoint> {
    let host_key = scalar_webapi_device_host(api).ok()?.to_string();
    if let Some(endpoint) = memory_cached_endpoint(&host_key) {
        return Some(endpoint);
    }
    if let Some(cached) = load_scalar_endpoint_from_disk(&host_key) {
        persist_scalar_endpoint_cache(&host_key, &cached.endpoint, cached.speaker_model.clone());
        return Some(cached.endpoint);
    }
    if let Some(cached) = load_scalar_endpoint_from_disk_allow_stale(&host_key) {
        persist_scalar_endpoint_cache(&host_key, &cached.endpoint, cached.speaker_model.clone());
        return Some(cached.endpoint);
    }
    endpoint_from_config(api).ok()
}

fn current_power_status_at_endpoint(
    api: &ScalarWebApiDeviceConfig,
    endpoint: &ScalarWebApiDeviceEndpoint,
) -> Result<String, RustyJackError> {
    let payload = serde_json::json!({
        "method": "getPowerStatus",
        "params": [],
        "id": 1,
        "version": "1.1"
    })
    .to_string();
    let response = websocket_json(
        &endpoint.host,
        endpoint.port,
        &endpoint.service_path(SYSTEM_SERVICE),
        &payload,
        api.request_timeout_ms,
    )
    .or_else(|_| {
        post_json(
            &endpoint.host,
            endpoint.port,
            &endpoint.service_path(SYSTEM_SERVICE),
            &payload,
            api.request_timeout_ms,
        )
    })?;
    power_status_from_response(&response)
}

pub(crate) fn resolve_scalar_webapi_device_endpoint(
    api: &ScalarWebApiDeviceConfig,
) -> Result<Option<ScalarWebApiDeviceEndpoint>, RustyJackError> {
    let host_key = scalar_webapi_device_host(api)?.to_string();
    if let Some(endpoint) = memory_cached_endpoint(&host_key) {
        return Ok(Some(endpoint));
    }
    if let Some(cached) = load_scalar_endpoint_from_disk(&host_key) {
        persist_scalar_endpoint_cache(&host_key, &cached.endpoint, cached.speaker_model.clone());
        return Ok(Some(cached.endpoint));
    }

    match discover_scalar_webapi_device_ssdp_hit(api) {
        Ok(Some(hit)) => {
            persist_scalar_endpoint_cache(&host_key, &hit.endpoint, hit.model.clone());
            return Ok(Some(hit.endpoint));
        }
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(
                target: "daemon",
                "[scalar] SSDP discovery failed for {host_key}: {}; trying cache/config endpoint",
                err.detail_message()
            );
        }
    }

    Ok(endpoint_after_ssdp_miss(api, &host_key))
}

/// Prefer a prior discovery cache entry, then config `host`/`port`/`path`, when SSDP misses.
fn endpoint_after_ssdp_miss(
    api: &ScalarWebApiDeviceConfig,
    host_key: &str,
) -> Option<ScalarWebApiDeviceEndpoint> {
    if let Some(cached) = load_scalar_endpoint_from_disk_allow_stale(host_key) {
        tracing::warn!(
            target: "daemon",
            "[scalar] SSDP found no JSON-RPC endpoint for {host_key}; using cached {}:{}{}",
            cached.endpoint.host,
            cached.endpoint.port,
            cached.endpoint.path
        );
        persist_scalar_endpoint_cache(host_key, &cached.endpoint, cached.speaker_model.clone());
        return Some(cached.endpoint);
    }

    match endpoint_from_config(api) {
        Ok(endpoint) => {
            tracing::warn!(
                target: "daemon",
                "[scalar] SSDP found no JSON-RPC endpoint for {host_key}; using configured {}:{}{}",
                endpoint.host,
                endpoint.port,
                endpoint.path
            );
            persist_scalar_endpoint_cache(host_key, &endpoint, None);
            Some(endpoint)
        }
        Err(err) => {
            tracing::warn!(
                target: "daemon",
                "[scalar] SSDP found no JSON-RPC endpoint for {host_key} and config endpoint is unusable: {}",
                err.detail_message()
            );
            None
        }
    }
}

fn send_wake_command_to(
    api: &ScalarWebApiDeviceConfig,
    api_endpoint: &ScalarWebApiDeviceEndpoint,
) -> Result<ScalarWebApiDeviceWakeResult, RustyJackError> {
    let endpoint = api_endpoint.service_endpoint(SYSTEM_SERVICE);
    let path = api_endpoint.service_path(SYSTEM_SERVICE);
    let wake_id = match prime_scalar_webapi_device_services(api, api_endpoint) {
        Ok(id) => id,
        Err(err) => {
            tracing::warn!(
                target: "daemon",
                "[scalar] service priming failed at {}: {}; sending setPowerStatus without prime",
                api_endpoint.base_url(),
                err.detail_message()
            );
            1
        }
    };
    let payload = wake_payload(wake_id);

    // Prefer HTTP POST for power-on: WebSocket wake can report success while the
    // device stays in standby; POST matches the reliable manual curl path.
    let response = post_json(
        &api_endpoint.host,
        api_endpoint.port,
        &path,
        &payload,
        api.request_timeout_ms,
    )
    .or_else(|err| {
        tracing::warn!(
            target: "daemon",
            "[scalar] HTTP setPowerStatus failed at {endpoint}: {}; trying WebSocket",
            err.detail_message()
        );
        websocket_json(
            &api_endpoint.host,
            api_endpoint.port,
            &path,
            &payload,
            api.request_timeout_ms,
        )
    })?;

    ensure_success_json(&response, &endpoint)?;
    let status_code = parse_http_status(&response)?;
    if !(200..300).contains(&status_code) {
        return Err(scalar_speaker_err(
            &endpoint,
            format!("returned HTTP {status_code}"),
        ));
    }

    Ok(ScalarWebApiDeviceWakeResult {
        endpoint,
        status_code,
        previous_status: None,
        trigger: String::new(),
        activity_event: None,
    })
}

fn prime_scalar_webapi_device_services(
    api: &ScalarWebApiDeviceConfig,
    api_endpoint: &ScalarWebApiDeviceEndpoint,
) -> Result<u64, RustyJackError> {
    let mut id = 1_u64;
    let guide_payload = serde_json::json!({
        "method": "getSupportedApiInfo",
        "params": [{}],
        "id": id,
        "version": "1.0"
    })
    .to_string();
    id += 1;
    let guide_response = post_json(
        &api_endpoint.host,
        api_endpoint.port,
        &api_endpoint.service_path("guide"),
        &guide_payload,
        api.request_timeout_ms,
    )?;
    ensure_json_has(&guide_response, "\"result\"", "guide.getSupportedApiInfo")?;

    for service in ["appControl", "audio", "avContent", "guide", SYSTEM_SERVICE] {
        let method_types_payload = serde_json::json!({
            "method": "getMethodTypes",
            "params": [""],
            "id": id,
            "version": "1.0"
        })
        .to_string();
        id += 1;
        let response = websocket_json(
            &api_endpoint.host,
            api_endpoint.port,
            &api_endpoint.service_path(service),
            &method_types_payload,
            api.request_timeout_ms,
        )?;
        ensure_json_has(
            &response,
            "\"results\"",
            &format!("{service}.getMethodTypes"),
        )?;
    }

    Ok(id)
}

fn wake_payload(id: u64) -> String {
    serde_json::json!({
        "method": "setPowerStatus",
        "params": [{ "status": "active" }],
        "id": id,
        "version": "1.1"
    })
    .to_string()
}

fn scalar_webapi_device_host(api: &ScalarWebApiDeviceConfig) -> Result<&str, RustyJackError> {
    api.host
        .as_deref()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| RustyJackError::Config("scalar_webapi_device.host is not set".into()))
}

fn endpoint_from_config(
    api: &ScalarWebApiDeviceConfig,
) -> Result<ScalarWebApiDeviceEndpoint, RustyJackError> {
    let host = scalar_webapi_device_host(api)?.to_string();
    let path = format!("/{}", api.path.trim().trim_matches('/'))
        .trim_end_matches('/')
        .to_string();
    Ok(ScalarWebApiDeviceEndpoint {
        host,
        port: api.port,
        path,
    })
}

/// Whether user-visible ScalarWebAPI probing shows a live progress indicator on stderr.
///
/// Covers SSDP/LAN discovery, configured-speaker SSDP refresh, and HTTP speaker probes
/// (power, inputs) in interactive CLI paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScalarDiscoveryFeedback {
    #[default]
    Silent,
    Interactive,
}

/// Scan the local network for ScalarWebAPI devices via SSDP/UPnP.
pub fn discover_scalar_webapi_devices_on_lan(
    request_timeout_ms: u64,
) -> Result<Vec<DiscoveredScalarWebApiDevice>, RustyJackError> {
    discover_scalar_webapi_devices_on_lan_with_feedback(
        request_timeout_ms,
        ScalarDiscoveryFeedback::Silent,
    )
}

/// Like [`discover_scalar_webapi_devices_on_lan`], with optional interactive progress output.
pub fn discover_scalar_webapi_devices_on_lan_with_feedback(
    request_timeout_ms: u64,
    feedback: ScalarDiscoveryFeedback,
) -> Result<Vec<DiscoveredScalarWebApiDevice>, RustyJackError> {
    let timeout = Duration::from_millis(request_timeout_ms.max(1_000));
    let _progress = ScalarProbingProgressGuard::start(feedback, "  probing local network");
    let hits = run_scalar_webapi_ssdp_discovery(timeout, None, "lan")?;
    Ok(hits
        .into_iter()
        .filter(|hit| {
            !is_scalar_webapi_tv_device(
                hit.model.as_deref(),
                Some(hit.location.as_str()),
                Some(hit.xml.as_str()),
            )
        })
        .map(|hit| DiscoveredScalarWebApiDevice {
            host: hit.endpoint.host,
            model: hit.model,
        })
        .collect())
}

pub fn refresh_scalar_webapi_discovery_cache(
    api: &ScalarWebApiDeviceConfig,
) -> Result<Option<DiscoveredScalarWebApiDevice>, RustyJackError> {
    refresh_scalar_webapi_discovery_cache_with_feedback(api, ScalarDiscoveryFeedback::Silent)
}

/// Like [`refresh_scalar_webapi_discovery_cache`], with optional interactive progress output.
pub fn refresh_scalar_webapi_discovery_cache_with_feedback(
    api: &ScalarWebApiDeviceConfig,
    feedback: ScalarDiscoveryFeedback,
) -> Result<Option<DiscoveredScalarWebApiDevice>, RustyJackError> {
    with_scalar_probing_feedback(feedback, "  probing configured speaker", || {
        let host_key = scalar_webapi_device_host(api)?.to_string();
        let hit = discover_scalar_webapi_device_ssdp_hit(api)?;
        if let Some(hit) = hit {
            let discovered = DiscoveredScalarWebApiDevice {
                host: hit.endpoint.host.clone(),
                model: hit.model.clone(),
            };
            persist_scalar_endpoint_cache(&host_key, &hit.endpoint, hit.model);
            return Ok(Some(discovered));
        }
        Ok(None)
    })
}

fn discover_scalar_webapi_device_ssdp_hit(
    api: &ScalarWebApiDeviceConfig,
) -> Result<Option<ScalarWebApiSsdpHit>, RustyJackError> {
    let host = scalar_webapi_device_host(api)?;
    let target_ips = resolve_host_ips(host)?;
    if target_ips.is_empty() {
        return Ok(None);
    }

    let timeout = Duration::from_millis(api.request_timeout_ms.max(1_000));
    let hits = run_scalar_webapi_ssdp_discovery(timeout, Some(&target_ips), host)?;
    Ok(hits.into_iter().next())
}

#[derive(Debug, Clone)]
struct ScalarWebApiSsdpHit {
    endpoint: ScalarWebApiDeviceEndpoint,
    model: Option<String>,
    location: String,
    xml: String,
}

fn run_scalar_webapi_ssdp_discovery(
    total_timeout: Duration,
    target_ips: Option<&[IpAddr]>,
    host_context: &str,
) -> Result<Vec<ScalarWebApiSsdpHit>, RustyJackError> {
    if !crate::network::lan_connectivity_ready() {
        return Ok(Vec::new());
    }

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(RustyJackError::Io)?;
    let deadline = Instant::now() + total_timeout;
    let http_timeout = Duration::from_millis(total_timeout.as_millis().clamp(500, 3_000) as u64);

    for probe in 0..SSDP_PROBE_COUNT {
        if Instant::now() >= deadline {
            break;
        }
        send_scalar_webapi_msearch_retry(&socket, host_context)?;
        if probe + 1 < SSDP_PROBE_COUNT {
            let remaining_ms = (deadline - Instant::now()).as_millis() as u64;
            let sleep_ms = SSDP_PROBE_INTERVAL_MS.min(remaining_ms);
            if sleep_ms > 0 {
                std::thread::sleep(Duration::from_millis(sleep_ms));
            }
        }
    }

    collect_scalar_webapi_ssdp_hits_until(&socket, deadline, target_ips, host_context, http_timeout)
}

pub(crate) fn with_scalar_probing_feedback<T>(
    feedback: ScalarDiscoveryFeedback,
    message: &str,
    probe: impl FnOnce() -> T,
) -> T {
    let _progress = ScalarProbingProgressGuard::start(feedback, message);
    probe()
}

pub(crate) struct ScalarProbingProgressGuard(Option<DiscoveryProgress>);

impl ScalarProbingProgressGuard {
    fn start(feedback: ScalarDiscoveryFeedback, message: &str) -> Self {
        if feedback == ScalarDiscoveryFeedback::Interactive {
            Self(Some(DiscoveryProgress::start(message)))
        } else {
            Self(None)
        }
    }
}

impl Drop for ScalarProbingProgressGuard {
    fn drop(&mut self) {
        if let Some(progress) = self.0.take() {
            progress.stop();
        }
    }
}

struct DiscoveryProgress {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl DiscoveryProgress {
    fn start(message: &str) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let message = message.to_string();
        let join = thread::spawn(move || {
            let frames = ['-', '\\', '|', '/'];
            let mut frame_index = 0usize;
            let mut stderr = std::io::stderr();
            while !stop_flag.load(Ordering::Relaxed) {
                let frame = frames[frame_index % frames.len()];
                frame_index = frame_index.wrapping_add(1);
                let _ = write!(stderr, "\r{message} {frame}");
                let _ = stderr.flush();
                thread::sleep(Duration::from_millis(120));
            }
        });
        Self {
            stop,
            join: Some(join),
        }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        let mut stderr = std::io::stderr();
        let _ = write!(stderr, "\r\x1b[K");
        let _ = stderr.flush();
    }
}

fn send_scalar_webapi_msearch_retry(
    socket: &UdpSocket,
    host_context: &str,
) -> Result<(), RustyJackError> {
    for attempt in 0..2 {
        match send_scalar_webapi_msearch(socket, host_context)? {
            Some(()) => return Ok(()),
            None if attempt == 0 => std::thread::sleep(Duration::from_millis(100)),
            None => return Ok(()),
        }
    }
    Ok(())
}

fn scalar_webapi_msearch_request() -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {SSDP_ADDR}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 1\r\n\
         ST: {SCALAR_WEBAPI_ST}\r\n\r\n"
    )
}

/// `Ok(Some(()))` when sent, `Ok(None)` when skipped due to transient network errors.
fn send_scalar_webapi_msearch(
    socket: &UdpSocket,
    host_context: &str,
) -> Result<Option<()>, RustyJackError> {
    match socket.send_to(scalar_webapi_msearch_request().as_bytes(), SSDP_ADDR) {
        Ok(_) => Ok(Some(())),
        Err(err) if is_transient_network_error(&err) => {
            let ssdp_url = scalar_ssdp_url();
            tracing::debug!(
                target: "daemon",
                "[scalar] url={ssdp_url} host={host_context}: SSDP M-SEARCH skipped: {err}"
            );
            Ok(None)
        }
        Err(err) => {
            let ssdp_url = scalar_ssdp_url();
            Err(scalar_speaker_err(
                &ssdp_url,
                format!("host={host_context}: could not send SSDP M-SEARCH: {err}"),
            ))
        }
    }
}

fn collect_scalar_webapi_ssdp_hits_until(
    socket: &UdpSocket,
    deadline: Instant,
    target_ips: Option<&[IpAddr]>,
    host_context: &str,
    http_timeout: Duration,
) -> Result<Vec<ScalarWebApiSsdpHit>, RustyJackError> {
    let mut hits = Vec::new();
    let mut buf = [0_u8; 4096];
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        let read_timeout = remaining.min(Duration::from_millis(SSDP_RECV_SLICE_MS));
        socket
            .set_read_timeout(Some(read_timeout))
            .map_err(RustyJackError::Io)?;
        match socket.recv_from(&mut buf) {
            Ok((len, addr)) => {
                if target_ips.is_some_and(|ips| !ips.contains(&addr.ip())) {
                    continue;
                }
                let response = String::from_utf8_lossy(&buf[..len]);
                let Some(location) = http_header(&response, "location") else {
                    continue;
                };
                let Ok(xml) = http_get(location, http_timeout) else {
                    continue;
                };
                let Some(base_url) = extract_xml_text(&xml, "X_ScalarWebAPI_BaseURL") else {
                    continue;
                };
                let Ok(endpoint) = parse_http_url(&base_url) else {
                    continue;
                };
                if hits
                    .iter()
                    .any(|hit: &ScalarWebApiSsdpHit| hit.endpoint.host == endpoint.host)
                {
                    continue;
                }
                hits.push(ScalarWebApiSsdpHit {
                    endpoint,
                    model: hardware_speaker_model_hint(location, &xml),
                    location: location.to_string(),
                    xml,
                });
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(err) => {
                let ssdp_url = scalar_ssdp_url();
                return Err(scalar_speaker_err(
                    &ssdp_url,
                    format!("host={host_context}: could not read SSDP response: {err}"),
                ));
            }
        }
    }
    Ok(hits)
}

fn model_hint_from_upnp_xml(xml: &str) -> Option<String> {
    extract_xml_text(xml, "friendlyName")
        .or_else(|| extract_xml_text(xml, "modelName"))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn model_hint_from_location_url(location: &str) -> Option<String> {
    let filename = location.rsplit('/').next()?.trim();
    let stem = filename.strip_suffix(".xml")?;
    let (_, model) = stem.rsplit_once('_')?;
    (!model.is_empty()).then(|| model.to_string())
}

fn is_scalar_webapi_tv_device(
    model: Option<&str>,
    location: Option<&str>,
    xml: Option<&str>,
) -> bool {
    for text in [
        model.unwrap_or_default(),
        location.unwrap_or_default(),
        xml.unwrap_or_default(),
    ] {
        let lower = text.to_ascii_lowercase();
        if lower.contains("bravia")
            || lower.contains("sony tv")
            || lower.contains("mediarenderer_tv")
            || lower.contains("_tv.xml")
        {
            return true;
        }
    }
    false
}

fn resolve_host_ips(host: &str) -> Result<Vec<IpAddr>, RustyJackError> {
    (host, 0)
        .to_socket_addrs()
        .map_err(|err| {
            scalar_speaker_err(
                &scalar_http_url(host, 0, "/"),
                format!("host={host}: could not resolve: {err}"),
            )
        })
        .map(|addrs| addrs.map(|addr| addr.ip()).collect())
}

fn http_header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    response.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn extract_xml_text(xml: &str, local_name: &str) -> Option<String> {
    let tag = xml.find(local_name)? + local_name.len();
    let start = tag + xml[tag..].find('>')? + 1;
    let value = xml[start..].split('<').next()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn http_get(url: &str, timeout: Duration) -> Result<String, RustyJackError> {
    let endpoint = parse_http_url(url)?;
    let request_path = if endpoint.path.is_empty() {
        "/"
    } else {
        &endpoint.path
    };
    let request = format!(
        "GET {} HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Accept: */*\r\n\
         Connection: close\r\n\r\n",
        request_path, endpoint.host, endpoint.port
    );
    let response = send_http(
        &scalar_http_url(&endpoint.host, endpoint.port, request_path),
        &endpoint.host,
        endpoint.port,
        &request,
        timeout,
    )?;
    let status_code = parse_http_status(&response)?;
    if !(200..300).contains(&status_code) {
        return Err(scalar_speaker_err(
            url,
            format!("GET returned HTTP {status_code}"),
        ));
    }
    Ok(response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or(&response)
        .to_string())
}

fn post_json(
    host: &str,
    port: u16,
    path: &str,
    body: &str,
    timeout_ms: u64,
) -> Result<String, RustyJackError> {
    let timeout = Duration::from_millis(timeout_ms.max(1));
    let url = scalar_http_url(host, port, path);
    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Content-Type: application/json\r\n\
         Accept: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {body}",
        body.len()
    );
    send_http(&url, host, port, &request, timeout)
}

fn websocket_json(
    host: &str,
    port: u16,
    path: &str,
    body: &str,
    timeout_ms: u64,
) -> Result<String, RustyJackError> {
    let timeout = Duration::from_millis(timeout_ms.max(1));
    let request_path = if path.is_empty() { "/" } else { path };
    let url = format!("ws://{host}:{port}{request_path}");
    let request = url.as_str().into_client_request().map_err(|err| {
        scalar_speaker_err(&url, format!("could not build WebSocket request: {err}"))
    })?;
    let address: SocketAddr = (host, port)
        .to_socket_addrs()
        .map_err(|err| scalar_speaker_err(&url, format!("could not resolve host: {err}")))?
        .next()
        .ok_or_else(|| scalar_speaker_err(&url, "could not resolve host"))?;
    let stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|err| scalar_speaker_err(&url, format!("connect failed: {err}")))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;

    let (mut socket, response) = tungstenite::client(request, stream)
        .map_err(|err| scalar_speaker_err(&url, format!("WebSocket handshake failed: {err}")))?;
    if response.status().as_u16() != 101 {
        return Err(scalar_speaker_err(
            &url,
            format!("WebSocket upgrade returned HTTP {}", response.status()),
        ));
    }

    socket
        .send(Message::Text(body.to_string().into()))
        .map_err(|err| {
            scalar_speaker_err(&url, format!("could not send WebSocket frame: {err}"))
        })?;
    let message = socket.read().map_err(|err| {
        scalar_speaker_err(&url, format!("could not read WebSocket response: {err}"))
    })?;
    let _ = socket.close(None);
    let body = match message {
        Message::Text(text) => text.to_string(),
        Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Message::Close(_) => {
            return Err(scalar_speaker_err(
                &url,
                "device closed WebSocket before response",
            ));
        }
        other => {
            return Err(scalar_speaker_err(
                &url,
                format!("unexpected WebSocket response: {other:?}"),
            ));
        }
    };
    Ok(format!("HTTP/1.1 200 OK\r\n\r\n{body}"))
}

fn send_http(
    url: &str,
    host: &str,
    port: u16,
    request: &str,
    timeout: Duration,
) -> Result<String, RustyJackError> {
    let address: SocketAddr = (host, port)
        .to_socket_addrs()
        .map_err(|err| scalar_speaker_err(url, format!("could not resolve host: {err}")))?
        .next()
        .ok_or_else(|| scalar_speaker_err(url, "could not resolve host"))?;
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|err| scalar_speaker_err(url, format!("connect failed: {err}")))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;

    stream
        .write_all(request.as_bytes())
        .map_err(|err| scalar_speaker_err(url, format!("could not send HTTP request: {err}")))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|err| scalar_speaker_err(url, format!("could not read HTTP response: {err}")))?;
    Ok(String::from_utf8_lossy(&response).into_owned())
}

fn ensure_success_json(response: &str, endpoint: &str) -> Result<(), RustyJackError> {
    ensure_json_has(response, "\"result\"", endpoint)
}

fn ensure_json_has(response: &str, expected: &str, label: &str) -> Result<(), RustyJackError> {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.trim())
        .unwrap_or(response.trim());
    if body.contains("\"error\"") {
        return Err(scalar_speaker_err(
            label,
            format!("returned error payload: {body}"),
        ));
    }
    if body.contains(expected) {
        return Ok(());
    }
    Err(scalar_speaker_err(
        label,
        format!("returned unexpected payload: {body}"),
    ))
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.trim())
        .unwrap_or(response.trim())
}

fn power_status_from_response(response: &str) -> Result<String, RustyJackError> {
    let body = response_body(response);
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| RustyJackError::Speaker(format!("invalid power status JSON: {err}")))?;
    value
        .get("result")
        .and_then(|result| result.get(0))
        .and_then(|result| result.get("status"))
        .and_then(|status| status.as_str())
        .map(str::to_string)
        .ok_or_else(|| RustyJackError::Speaker(format!("missing power status in response: {body}")))
}

fn parse_http_status(response: &str) -> Result<u16, RustyJackError> {
    response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| {
            RustyJackError::Speaker("invalid HTTP response from ScalarWebAPI device".into())
        })
}

fn parse_http_url(url: &str) -> Result<ScalarWebApiDeviceEndpoint, RustyJackError> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        RustyJackError::Speaker(format!("unsupported ScalarWebAPI endpoint URL: {url}"))
    })?;
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        let port = port.parse().map_err(|_| {
            RustyJackError::Speaker(format!("invalid ScalarWebAPI endpoint port in URL: {url}"))
        })?;
        (host, port)
    } else {
        (host_port, 80)
    };
    if host.is_empty() {
        return Err(RustyJackError::Speaker(format!(
            "invalid ScalarWebAPI endpoint URL: {url}"
        )));
    }
    Ok(ScalarWebApiDeviceEndpoint {
        host: host.to_string(),
        port,
        path: format!("/{path}").trim_end_matches('/').to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceSelectorConfig;
    use crate::transport::TransportKind;

    fn device(uid: &str) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "External Headphones".into(),
            transport: TransportKind::BuiltIn,
            is_alive: true,
            is_default: false,
            is_active: false,
        }
    }

    fn clear_scalar_webapi_memory_endpoint_cache_for_tests() {
        if let Ok(mut guard) = endpoint_cache().lock() {
            *guard = None;
        }
    }

    fn config_for(uid: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            poll_interval_ms: 3_000,
            switch_delay_ms: 500,
            activity_idle_threshold_ms: 60_000,
            activity_poll_interval_ms: 1_000,
            activity_monitor: "idle".into(),
            preferred_device: DeviceSelectorConfig {
                name: None,
                uid: Some(uid.into()),
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            scalar_webapi_device: Some(ScalarWebApiDeviceConfig {
                enabled: true,
                model: "ScalarWebAPI device".into(),
                host: Some("scalarwebapi-device.local".into()),
                port: 10_000,
                path: protocol_path(),
                mac_output: DeviceSelectorConfig {
                    name: None,
                    uid: Some(uid.into()),
                },
                triggers: vec![OUTPUT_SELECTED_TRIGGER.into()],
                wake_debounce_ms: 5_000,
                request_timeout_ms: 3_000,
                require_quick_start: true,
                speaker_input: None,
            }),
            ..Default::default()
        }
    }

    fn protocol_path() -> String {
        ["so", "ny"].concat()
    }

    #[test]
    fn test_configured_endpoint_uses_slash_path() {
        let config = config_for("line-out");
        let api = config.scalar_webapi_device.as_ref().unwrap();
        let endpoint = endpoint_from_config(api).unwrap();
        let expected_path = protocol_path();
        assert_eq!(
            endpoint.service_endpoint(SYSTEM_SERVICE),
            format!("http://scalarwebapi-device.local:10000/{expected_path}/system")
        );
        assert_eq!(
            endpoint.service_path(SYSTEM_SERVICE),
            format!("/{expected_path}/system")
        );
    }

    #[test]
    fn test_parse_discovered_endpoint_url() {
        let expected_path = protocol_path();
        let endpoint =
            parse_http_url(&format!("http://192.168.86.18:54480/{expected_path}")).unwrap();
        assert_eq!(endpoint.host, "192.168.86.18");
        assert_eq!(endpoint.port, 54_480);
        assert_eq!(endpoint.path, format!("/{expected_path}"));
        assert_eq!(
            endpoint.service_endpoint(SYSTEM_SERVICE),
            format!("http://192.168.86.18:54480/{expected_path}/system")
        );
    }

    #[test]
    fn test_model_hint_from_location_url() {
        assert_eq!(
            model_hint_from_location_url("http://192.168.86.18:54380/MediaRenderer_SRS-ZR5.xml")
                .as_deref(),
            Some("SRS-ZR5")
        );
        assert_eq!(
            model_hint_from_location_url("http://speaker/device.xml"),
            None
        );
    }

    #[test]
    fn test_display_endpoint_uses_config_without_ssdp() {
        clear_scalar_webapi_endpoint_cache_for_tests();
        let mut config = config_for("line-out");
        config.scalar_webapi_device.as_mut().unwrap().host = Some("offline.test".into());
        let api = config.scalar_webapi_device.as_ref().unwrap();
        let endpoint = display_endpoint_for_api(api).unwrap();
        assert_eq!(endpoint.host, "offline.test");
        assert_eq!(endpoint.port, 10_000);
        assert!(endpoint.path.ends_with(&protocol_path()));
    }

    #[test]
    fn test_endpoint_after_ssdp_miss_prefers_stale_cache_over_config() {
        let _lock = discovery_cache_test_lock();
        clear_scalar_webapi_memory_endpoint_cache_for_tests();
        let mut config = config_for("line-out");
        let api = config.scalar_webapi_device.as_mut().unwrap();
        let host_key = "stale-cache.test";
        api.host = Some(host_key.into());
        api.port = 10_000;
        let stale = ScalarWebApiDeviceEndpoint {
            host: host_key.into(),
            port: 54_480,
            path: format!("/{}", protocol_path()),
        };
        let mut cache = load_scalar_discovery_cache_file().unwrap_or_default();
        cache.hosts.insert(
            host_key.into(),
            CachedScalarEndpointOnDisk {
                endpoint: stale.clone(),
                speaker_model: Some("SRS-ZR5".into()),
                cached_at_unix_ms: now_unix_ms()
                    .saturating_sub(ENDPOINT_CACHE_TTL.as_millis() as u64 + 60_000),
            },
        );
        write_scalar_discovery_cache_file(&cache);

        assert!(load_scalar_endpoint_from_disk(host_key).is_none());
        assert!(load_scalar_endpoint_from_disk_allow_stale(host_key).is_some());

        let resolved = endpoint_after_ssdp_miss(api, host_key).unwrap();
        assert_eq!(resolved.port, 54_480);
        assert_eq!(resolved.host, host_key);
    }

    #[test]
    fn test_endpoint_after_ssdp_miss_uses_configured_port() {
        let _lock = discovery_cache_test_lock();
        clear_scalar_webapi_memory_endpoint_cache_for_tests();
        let mut config = config_for("line-out");
        let api = config.scalar_webapi_device.as_mut().unwrap();
        let host_key = "config-fallback.test";
        api.host = Some(host_key.into());
        api.port = 54_480;
        let resolved = endpoint_after_ssdp_miss(api, host_key).unwrap();
        assert_eq!(resolved.host, host_key);
        assert_eq!(resolved.port, 54_480);
        assert!(resolved.path.ends_with(&protocol_path()));
    }

    #[test]
    fn test_load_scalar_endpoint_from_disk_keeps_stale_entries() {
        let _lock = discovery_cache_test_lock();
        clear_scalar_webapi_memory_endpoint_cache_for_tests();
        let host_key = "keep-stale.test";
        let endpoint = ScalarWebApiDeviceEndpoint {
            host: host_key.into(),
            port: 54_480,
            path: format!("/{}", protocol_path()),
        };
        let mut cache = load_scalar_discovery_cache_file().unwrap_or_default();
        cache.hosts.insert(
            host_key.into(),
            CachedScalarEndpointOnDisk {
                endpoint: endpoint.clone(),
                speaker_model: Some("SRS-ZR5".into()),
                cached_at_unix_ms: now_unix_ms()
                    .saturating_sub(ENDPOINT_CACHE_TTL.as_millis() as u64 + 1),
            },
        );
        write_scalar_discovery_cache_file(&cache);

        assert!(load_scalar_endpoint_from_disk(host_key).is_none());
        let kept = load_scalar_endpoint_from_disk_allow_stale(host_key).unwrap();
        assert_eq!(kept.endpoint.port, 54_480);
        assert!(load_scalar_discovery_cache_file()
            .unwrap()
            .hosts
            .contains_key(host_key));
    }

    #[test]
    fn test_hardware_speaker_model_hint_prefers_location_stem() {
        let xml = r#"
<root>
  <friendlyName>Friendly Name</friendlyName>
</root>"#;
        assert_eq!(
            hardware_speaker_model_hint(
                "http://192.168.86.18:54380/MediaRenderer_SRS-ZR5.xml",
                xml
            )
            .as_deref(),
            Some("SRS-ZR5")
        );
    }

    #[test]
    fn test_should_show_distinct_speaker_model() {
        assert!(should_show_distinct_speaker_model("The Lair", "SRS-ZR5"));
        assert!(!should_show_distinct_speaker_model("SRS-ZR5", "SRS-ZR5"));
        assert!(!should_show_distinct_speaker_model(
            GENERIC_SCALAR_WEBAPI_MODEL,
            "SRS-ZR5"
        ));
    }

    #[test]
    fn test_is_scalar_webapi_tv_device_detects_bravia() {
        assert!(is_scalar_webapi_tv_device(
            Some("BRAVIA 4K VH2"),
            Some("http://192.168.1.10:8080/MediaRenderer_BRAVIA.xml"),
            None,
        ));
        assert!(!is_scalar_webapi_tv_device(
            Some("SRS-ZR5"),
            Some("http://192.168.86.18:54380/MediaRenderer_SRS-ZR5.xml"),
            None,
        ));
    }

    #[test]
    fn test_format_scalar_webapi_device_column_suffix() {
        let link = ScalarWebApiMacOutputLink {
            mac_output_uid: "BuiltInHeadphoneOutputDevice".into(),
            mac_output_label: Some("External Headphones".into()),
            model: "The Lair".into(),
            host: Some("192.168.86.18".into()),
        };
        assert_eq!(
            format_scalar_webapi_device_column_suffix(&link),
            " — The Lair @ 192.168.86.18"
        );
    }

    #[test]
    fn test_model_hint_from_upnp_xml() {
        let xml = r#"
<root>
  <friendlyName>SRS-ZR5</friendlyName>
</root>"#;
        assert_eq!(model_hint_from_upnp_xml(xml).as_deref(), Some("SRS-ZR5"));
    }

    #[test]
    fn test_extract_scalar_base_url_from_upnp_xml() {
        let expected_path = protocol_path();
        let expected_url = format!("http://192.168.86.18:54480/{expected_path}");
        let xml = format!(
            r#"
<root>
  <av:X_ScalarWebAPI_BaseURL xmlns:av="urn:schemas-scalarwebapi-com:av">{expected_url}</av:X_ScalarWebAPI_BaseURL>
</root>"#
        );
        assert_eq!(
            extract_xml_text(&xml, "X_ScalarWebAPI_BaseURL").as_deref(),
            Some(expected_url.as_str())
        );
    }

    #[test]
    fn test_http_header_is_case_insensitive() {
        let response = "HTTP/1.1 200 OK\r\nLOCATION: http://speaker/desc.xml\r\n\r\n";
        assert_eq!(
            http_header(response, "location"),
            Some("http://speaker/desc.xml")
        );
    }

    #[test]
    fn test_trigger_matching_is_case_insensitive() {
        let mut config = config_for("line-out");
        let api = config.scalar_webapi_device.as_mut().unwrap();
        api.triggers = vec!["Output_Selected".into()];
        assert!(trigger_enabled(api, OUTPUT_SELECTED_TRIGGER));
    }

    #[test]
    fn test_format_scalar_webapi_triggers_for_display() {
        let triggers = vec![
            KEYBOARD_TRIGGER.into(),
            MOUSE_TRIGGER.into(),
            OUTPUT_SELECTED_TRIGGER.into(),
        ];
        assert_eq!(
            format_scalar_webapi_triggers_for_display(&triggers, Some("External Headphones")),
            "keyboard activity, mouse/pointer activity, and selecting External Headphones"
        );
        assert_eq!(
            format_scalar_webapi_triggers_for_display(&[KEYBOARD_TRIGGER.into()], None),
            "keyboard activity"
        );
        assert_eq!(
            format_scalar_webapi_triggers_for_display(&[], None),
            "(none)"
        );
    }

    #[test]
    fn test_parse_http_status() {
        assert_eq!(
            parse_http_status("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap(),
            200
        );
        assert!(parse_http_status("").is_err());
    }

    #[test]
    fn test_power_status_from_response() {
        let response = "HTTP/1.1 200 OK\r\n\r\n{\"result\":[{\"status\":\"standby\"}],\"id\":1}";
        assert_eq!(power_status_from_response(response).unwrap(), "standby");
    }

    #[test]
    fn test_scalar_speaker_err_includes_url() {
        let err = scalar_speaker_err(
            "ssdp://239.255.255.250:1900",
            "host=192.168.86.18: could not send SSDP M-SEARCH: No route to host",
        );
        assert_eq!(
            err.to_string(),
            "speaker wake error: url=ssdp://239.255.255.250:1900: host=192.168.86.18: could not send SSDP M-SEARCH: No route to host"
        );
    }

    #[test]
    fn test_is_transient_network_error() {
        let err = std::io::Error::from_raw_os_error(65);
        assert!(is_transient_network_error(&err));
    }

    #[test]
    fn test_wake_debounce_blocks_until_cooldown_elapses() {
        clear_wake_debounce_for_tests();
        let api = ScalarWebApiDeviceConfig {
            enabled: true,
            model: "test".into(),
            host: Some("192.168.1.1".into()),
            port: 10_000,
            path: "/sony".into(),
            mac_output: Default::default(),
            triggers: vec![],
            wake_debounce_ms: 50,
            request_timeout_ms: 3_000,
            require_quick_start: true,
            speaker_input: None,
        };
        assert!(wake_attempt_allowed_for_api(&api));
        record_successful_wake_sent();
        assert!(!wake_attempt_allowed_for_api(&api));
        std::thread::sleep(Duration::from_millis(60));
        assert!(wake_attempt_allowed_for_api(&api));
    }

    #[test]
    fn test_reset_wake_debounce_allows_immediate_retry() {
        clear_wake_debounce_for_tests();
        let api = ScalarWebApiDeviceConfig {
            enabled: true,
            model: "test".into(),
            host: Some("192.168.1.1".into()),
            port: 10_000,
            path: "/sony".into(),
            mac_output: Default::default(),
            triggers: vec![],
            wake_debounce_ms: 5_000,
            request_timeout_ms: 3_000,
            require_quick_start: true,
            speaker_input: None,
        };
        record_successful_wake_sent();
        assert!(!wake_attempt_allowed_for_api(&api));
        reset_wake_debounce();
        assert!(wake_attempt_allowed_for_api(&api));
    }

    #[test]
    fn test_format_wake_message_mentions_waking_from_standby() {
        let message = format_wake_message(&ScalarWebApiDeviceWakeResult {
            endpoint: format!("http://speaker/{}/system", protocol_path()),
            status_code: 200,
            previous_status: Some("standby".into()),
            trigger: OUTPUT_SELECTED_TRIGGER.into(),
            activity_event: Some("KeyDown".into()),
        });
        assert!(message.contains("output device selection"));
        assert!(message.contains("standby"));
        assert!(message.contains("(KeyDown)"));
    }

    #[test]
    fn test_format_wake_message_generic_when_status_unknown() {
        let message = format_wake_message(&ScalarWebApiDeviceWakeResult {
            endpoint: format!("http://speaker/{}/system", protocol_path()),
            status_code: 200,
            previous_status: None,
            trigger: KEYBOARD_TRIGGER.into(),
            activity_event: None,
        });
        assert!(message.contains("keyboard activity"));
        assert!(message.contains("power status unavailable"));
    }

    #[test]
    fn test_selection_filter_skips_non_scalar_webapi_device_output_before_network() {
        clear_scalar_webapi_endpoint_cache_for_tests();
        let config = config_for("line-out");
        let devices = vec![device("line-out"), device("hdmi")];
        assert_eq!(
            wake_on_output_selected(&config, &devices, "hdmi").unwrap(),
            None
        );
    }

    #[test]
    fn test_disabled_config_skips_wake() {
        let mut config = config_for("line-out");
        config.scalar_webapi_device.as_mut().unwrap().enabled = false;
        let devices = vec![device("line-out")];
        assert_eq!(
            wake_on_output_selected(&config, &devices, "line-out").unwrap(),
            None
        );
    }
}
