//! ScalarWebAPI speaker input selection (`avContent` service).

use super::{
    display_endpoint_for_api, endpoint_from_config, post_json,
    resolve_scalar_webapi_device_endpoint, scalar_speaker_err, with_scalar_probing_feedback,
    ScalarDiscoveryFeedback, ScalarWebApiDeviceEndpoint, AV_CONTENT_SERVICE,
};
use crate::config::{ScalarWebApiDeviceConfig, DEFAULT_SCALAR_WEBAPI_SPEAKER_INPUT};
use crate::RustyJackError;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static LAST_SUCCESSFUL_INPUT_SWITCH: Mutex<Option<Instant>> = Mutex::new(None);

/// A physical input on a ScalarWebAPI speaker (HDMI, Audio in, Bluetooth, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScalarWebApiSpeakerInput {
    pub uri: String,
    pub title: String,
}

/// Outcome when rusty-jack switches the speaker input to match config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarWebApiSpeakerInputEnsureResult {
    pub endpoint: String,
    pub configured_input: String,
    pub previous_input: String,
    pub trigger: String,
}

/// List inputs advertised by the speaker (`getCurrentExternalTerminalsStatus`).
pub fn list_scalar_webapi_speaker_inputs(
    api: &ScalarWebApiDeviceConfig,
) -> Result<Vec<ScalarWebApiSpeakerInput>, RustyJackError> {
    list_scalar_webapi_speaker_inputs_with_feedback(api, ScalarDiscoveryFeedback::Silent)
}

/// Like [`list_scalar_webapi_speaker_inputs`], with optional interactive progress output.
pub fn list_scalar_webapi_speaker_inputs_with_feedback(
    api: &ScalarWebApiDeviceConfig,
    feedback: ScalarDiscoveryFeedback,
) -> Result<Vec<ScalarWebApiSpeakerInput>, RustyJackError> {
    with_scalar_probing_feedback(feedback, "  probing speaker inputs", || {
        let endpoint = resolve_scalar_webapi_input_endpoint(api)?;
        list_scalar_webapi_speaker_inputs_at_endpoint(api, &endpoint)
    })
}

fn list_scalar_webapi_speaker_inputs_at_endpoint(
    api: &ScalarWebApiDeviceConfig,
    endpoint: &ScalarWebApiDeviceEndpoint,
) -> Result<Vec<ScalarWebApiSpeakerInput>, RustyJackError> {
    let path = endpoint.service_path(AV_CONTENT_SERVICE);
    let payloads = [
        serde_json::json!({
            "method": "getCurrentExternalTerminalsStatus",
            "params": [],
            "id": 1,
            "version": "1.0"
        })
        .to_string(),
        serde_json::json!({
            "method": "getCurrentExternalTerminalsStatus",
            "params": [{}],
            "id": 1,
            "version": "1.2"
        })
        .to_string(),
    ];

    let mut last_err = None;
    for payload in payloads {
        match post_json(
            &endpoint.host,
            endpoint.port,
            &path,
            &payload,
            api.request_timeout_ms,
        ) {
            Ok(response) => match speaker_inputs_from_terminals_response(&response) {
                Ok(inputs) if !inputs.is_empty() => return Ok(inputs),
                Ok(_) => continue,
                Err(err) => last_err = Some(err),
            },
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        scalar_speaker_err(
            &endpoint.service_endpoint(AV_CONTENT_SERVICE),
            "could not list speaker inputs",
        )
    }))
}

/// Validate the effective speaker input (explicit or default) against the device.
pub fn validate_configured_speaker_input(
    api: &ScalarWebApiDeviceConfig,
) -> Result<ScalarWebApiSpeakerInput, RustyJackError> {
    let name = configured_speaker_input_name(api).ok_or_else(|| {
        RustyJackError::Config("scalar_webapi_device.speaker_input is not configured".into())
    })?;
    validate_speaker_input_name(api, &name)
}

/// Return a validation error for status output when the speaker rejects the configured input.
pub fn configured_speaker_input_validation_error(api: &ScalarWebApiDeviceConfig) -> Option<String> {
    if !api.enabled {
        return None;
    }
    validate_configured_speaker_input(api)
        .err()
        .map(|err| err.to_string())
}

/// Validate a configured speaker input label against the device and return its URI.
pub fn validate_speaker_input_name(
    api: &ScalarWebApiDeviceConfig,
    name: &str,
) -> Result<ScalarWebApiSpeakerInput, RustyJackError> {
    validate_speaker_input_name_with_feedback(api, name, ScalarDiscoveryFeedback::Silent)
}

/// Like [`validate_speaker_input_name`], with optional interactive progress output.
pub fn validate_speaker_input_name_with_feedback(
    api: &ScalarWebApiDeviceConfig,
    name: &str,
    feedback: ScalarDiscoveryFeedback,
) -> Result<ScalarWebApiSpeakerInput, RustyJackError> {
    let inputs = list_scalar_webapi_speaker_inputs_with_feedback(api, feedback)?;
    validate_speaker_input_name_in_list(name, &inputs)
}

/// Validate a speaker input label against a known device input list.
pub fn validate_speaker_input_name_in_list(
    name: &str,
    inputs: &[ScalarWebApiSpeakerInput],
) -> Result<ScalarWebApiSpeakerInput, RustyJackError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(RustyJackError::Config(
            "scalar_webapi_device.speaker_input cannot be empty".into(),
        ));
    }
    match find_speaker_input_by_title(inputs, name) {
        Some(input) => Ok(input.clone()),
        None => Err(RustyJackError::Config(format!(
            "scalar_webapi_device.speaker_input {name:?} is not a speaker input; available: {}",
            format_speaker_input_names(inputs)
        ))),
    }
}

/// Read the active speaker input label, when the device reports one.
pub fn current_scalar_webapi_speaker_input(api: &ScalarWebApiDeviceConfig) -> Option<String> {
    let endpoint = display_endpoint_for_api(api)?;
    let inputs = list_scalar_webapi_speaker_inputs_at_endpoint(api, &endpoint).ok()?;
    let active_uri = current_scalar_webapi_speaker_input_uri_at_endpoint(api, &endpoint)
        .ok()
        .flatten()?;
    speaker_input_title_for_uri(&inputs, &active_uri)
}

fn current_scalar_webapi_speaker_input_uri_at_endpoint(
    api: &ScalarWebApiDeviceConfig,
    endpoint: &ScalarWebApiDeviceEndpoint,
) -> Result<Option<String>, RustyJackError> {
    let path = endpoint.service_path(AV_CONTENT_SERVICE);
    let payload = serde_json::json!({
        "method": "getAvailablePlaybackFunction",
        "params": [{}],
        "id": 1,
        "version": "1.0"
    })
    .to_string();
    let response = post_json(
        &endpoint.host,
        endpoint.port,
        &path,
        &payload,
        api.request_timeout_ms,
    )?;
    active_speaker_input_uri_from_playback_response(&response)
}

/// Ensure the speaker uses the configured input label when reachable.
pub fn ensure_scalar_webapi_speaker_input(
    api: &ScalarWebApiDeviceConfig,
    trigger: &str,
) -> Result<Option<ScalarWebApiSpeakerInputEnsureResult>, RustyJackError> {
    let configured_name = match configured_speaker_input_name(api) {
        Some(name) => name,
        None => return Ok(None),
    };

    if !input_switch_allowed(api) {
        return Ok(None);
    }

    let endpoint = resolve_scalar_webapi_input_endpoint(api)?;
    let inputs = list_scalar_webapi_speaker_inputs_at_endpoint(api, &endpoint)?;
    let configured = find_speaker_input_by_title(&inputs, &configured_name).ok_or_else(|| {
        RustyJackError::Config(format!(
            "scalar_webapi_device.speaker_input {configured_name:?} is not a speaker input; available: {}",
            format_speaker_input_names(&inputs)
        ))
    })?;

    let previous_uri = match current_scalar_webapi_speaker_input_uri_at_endpoint(api, &endpoint)? {
        Some(uri) => uri,
        None => return Ok(None),
    };

    if speaker_input_uris_match(&previous_uri, &configured.uri) {
        return Ok(None);
    }

    let previous_input =
        speaker_input_title_for_uri(&inputs, &previous_uri).unwrap_or_else(|| previous_uri.clone());

    set_scalar_webapi_speaker_input_at_endpoint(api, &endpoint, &configured.uri)?;
    record_successful_input_switch();

    Ok(Some(ScalarWebApiSpeakerInputEnsureResult {
        endpoint: endpoint.service_endpoint(AV_CONTENT_SERVICE),
        configured_input: configured.title.clone(),
        previous_input,
        trigger: trigger.into(),
    }))
}

#[must_use]
pub fn format_speaker_input_ensure_message(
    result: &ScalarWebApiSpeakerInputEnsureResult,
) -> String {
    format!(
        "ScalarWebAPI input on {}: switched from {} to {} via {}.",
        human_readable_trigger_label(&result.trigger, None),
        result.previous_input,
        result.configured_input,
        result.endpoint
    )
}

/// Configured speaker input label for status and picker output.
#[must_use]
pub fn configured_speaker_input_label(api: &ScalarWebApiDeviceConfig) -> Option<String> {
    configured_speaker_input_name(api)
}

/// Whether the effective speaker input comes from the built-in default.
#[must_use]
pub fn speaker_input_uses_default(api: &ScalarWebApiDeviceConfig) -> bool {
    api.enabled
        && api
            .speaker_input
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
}

pub(crate) fn configured_speaker_input_name(api: &ScalarWebApiDeviceConfig) -> Option<String> {
    if !api.enabled {
        return None;
    }
    api.speaker_input
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| Some(DEFAULT_SCALAR_WEBAPI_SPEAKER_INPUT.into()))
}

fn find_speaker_input_by_title<'a>(
    inputs: &'a [ScalarWebApiSpeakerInput],
    name: &str,
) -> Option<&'a ScalarWebApiSpeakerInput> {
    let name = name.trim();
    inputs
        .iter()
        .find(|input| input.title.eq_ignore_ascii_case(name))
}

fn speaker_input_title_for_uri(inputs: &[ScalarWebApiSpeakerInput], uri: &str) -> Option<String> {
    inputs
        .iter()
        .find(|input| speaker_input_uris_match(&input.uri, uri))
        .map(|input| input.title.clone())
}

fn format_speaker_input_names(inputs: &[ScalarWebApiSpeakerInput]) -> String {
    match inputs.len() {
        0 => "(none)".into(),
        1 => inputs[0].title.clone(),
        n => {
            let head = inputs[..n - 1]
                .iter()
                .map(|input| input.title.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}, and {}", head, inputs[n - 1].title)
        }
    }
}

fn resolve_scalar_webapi_input_endpoint(
    api: &ScalarWebApiDeviceConfig,
) -> Result<ScalarWebApiDeviceEndpoint, RustyJackError> {
    if let Some(endpoint) = resolve_scalar_webapi_device_endpoint(api)? {
        return Ok(endpoint);
    }
    endpoint_from_config(api)
}

fn set_scalar_webapi_speaker_input_at_endpoint(
    api: &ScalarWebApiDeviceConfig,
    endpoint: &ScalarWebApiDeviceEndpoint,
    uri: &str,
) -> Result<(), RustyJackError> {
    let path = endpoint.service_path(AV_CONTENT_SERVICE);
    let service = endpoint.service_endpoint(AV_CONTENT_SERVICE);
    let payloads = [
        (
            serde_json::json!({
                "method": "setPlayContent",
                "params": [{ "uri": uri, "output": "" }],
                "id": 1,
                "version": "1.2"
            })
            .to_string(),
            "1.2",
        ),
        (
            serde_json::json!({
                "method": "setPlayContent",
                "params": [{ "uri": uri, "output": "" }],
                "id": 1,
                "version": "1.0"
            })
            .to_string(),
            "1.0",
        ),
    ];

    let mut last_err = None;
    for (payload, version) in payloads {
        match post_json(
            &endpoint.host,
            endpoint.port,
            &path,
            &payload,
            api.request_timeout_ms,
        ) {
            Ok(response) if json_has_result(&response) => return Ok(()),
            Ok(response) => {
                last_err = Some(scalar_speaker_err(
                    &service,
                    format!(
                        "setPlayContent({version}) returned unexpected payload: {}",
                        response_body(&response)
                    ),
                ));
            }
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        scalar_speaker_err(&service, "setPlayContent failed for all API versions")
    }))
}

fn speaker_inputs_from_terminals_response(
    response: &str,
) -> Result<Vec<ScalarWebApiSpeakerInput>, RustyJackError> {
    let body = response_body(response);
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| RustyJackError::Speaker(format!("invalid speaker inputs JSON: {err}")))?;
    if value.get("error").is_some() {
        return Err(RustyJackError::Speaker(format!(
            "speaker inputs query returned error payload: {body}"
        )));
    }

    let terminals = value
        .get("result")
        .and_then(|result| result.as_array())
        .and_then(|outer| outer.first())
        .and_then(|inner| inner.as_array())
        .ok_or_else(|| {
            RustyJackError::Speaker(format!("missing speaker inputs in response: {body}"))
        })?;

    let mut inputs = Vec::new();
    for terminal in terminals {
        let Some(uri) = terminal
            .get("uri")
            .and_then(|uri| uri.as_str())
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
        else {
            continue;
        };
        let meta = terminal
            .get("meta")
            .and_then(|meta| meta.as_str())
            .unwrap_or_default();
        if meta.contains("meta:zone:output") {
            continue;
        }
        let title = terminal
            .get("title")
            .and_then(|title| title.as_str())
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(uri)
            .to_string();
        inputs.push(ScalarWebApiSpeakerInput {
            uri: uri.to_string(),
            title,
        });
    }
    Ok(inputs)
}

fn active_speaker_input_uri_from_playback_response(
    response: &str,
) -> Result<Option<String>, RustyJackError> {
    let body = response_body(response);
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| RustyJackError::Speaker(format!("invalid playback function JSON: {err}")))?;
    if value.get("error").is_some() {
        return Ok(None);
    }
    let uri = value
        .get("result")
        .and_then(|result| result.as_array())
        .and_then(|outer| outer.first())
        .and_then(|inner| inner.as_array())
        .and_then(|inputs| inputs.first())
        .and_then(|input| input.get("uri"))
        .and_then(|uri| uri.as_str())
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
        .map(str::to_string);
    Ok(uri)
}

fn speaker_input_uris_match(current: &str, configured: &str) -> bool {
    current.eq_ignore_ascii_case(configured)
}

fn input_switch_allowed(api: &ScalarWebApiDeviceConfig) -> bool {
    let cooldown = Duration::from_millis(api.wake_debounce_ms);
    LAST_SUCCESSFUL_INPUT_SWITCH
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_none_or(|last| last.elapsed() >= cooldown)
}

fn record_successful_input_switch() {
    if let Ok(mut guard) = LAST_SUCCESSFUL_INPUT_SWITCH.lock() {
        *guard = Some(Instant::now());
    }
}

fn json_has_result(response: &str) -> bool {
    response_body(response).contains("\"result\"")
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.trim())
        .unwrap_or(response.trim())
}

use super::human_readable_trigger_label;

#[cfg(test)]
mod tests {
    use super::*;

    const TERMINALS_RESPONSE: &str = "HTTP/1.1 200 OK\r\n\r\n{\"result\":[[{\"uri\":\"extInput:btAudio\",\"title\":\"Bluetooth Audio\",\"connection\":\"unknown\",\"meta\":\"meta:btaudio\"},{\"uri\":\"extInput:line?port=1\",\"title\":\"Audio in\",\"connection\":\"unknown\",\"meta\":\"meta:line\"},{\"uri\":\"extInput:hdmi\",\"title\":\"HDMI\",\"connection\":\"unconnected\",\"meta\":\"meta:hdmi\"}]],\"id\":4}";

    const PLAYBACK_RESPONSE: &str = "HTTP/1.1 200 OK\r\n\r\n{\"result\":[[{\"uri\":\"extInput:hdmi\",\"functions\":[{\"function\":\"\",\"isAvailable\":true}]}]],\"id\":20}";

    fn sample_inputs() -> Vec<ScalarWebApiSpeakerInput> {
        speaker_inputs_from_terminals_response(TERMINALS_RESPONSE).unwrap()
    }

    #[test]
    fn test_speaker_inputs_from_terminals_response() {
        let inputs = sample_inputs();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[1].uri, "extInput:line?port=1");
        assert_eq!(inputs[1].title, "Audio in");
    }

    #[test]
    fn test_active_speaker_input_uri_from_playback_response() {
        let uri = active_speaker_input_uri_from_playback_response(PLAYBACK_RESPONSE)
            .unwrap()
            .unwrap();
        assert_eq!(uri, "extInput:hdmi");
    }

    #[test]
    fn test_find_speaker_input_by_title_is_case_insensitive() {
        let inputs = sample_inputs();
        let input = find_speaker_input_by_title(&inputs, "audio in").unwrap();
        assert_eq!(input.uri, "extInput:line?port=1");
    }

    #[test]
    fn test_speaker_input_title_for_uri() {
        let inputs = sample_inputs();
        assert_eq!(
            speaker_input_title_for_uri(&inputs, "extInput:hdmi").as_deref(),
            Some("HDMI")
        );
    }

    #[test]
    fn test_speaker_input_uris_match() {
        assert!(speaker_input_uris_match(
            "extInput:line?port=1",
            "extInput:line?port=1"
        ));
        assert!(!speaker_input_uris_match(
            "extInput:hdmi",
            "extInput:line?port=1"
        ));
    }

    #[test]
    fn test_validate_speaker_input_name_in_list_rejects_unknown_default() {
        let inputs = vec![ScalarWebApiSpeakerInput {
            uri: "extInput:hdmi".into(),
            title: "HDMI".into(),
        }];
        let err = validate_speaker_input_name_in_list(DEFAULT_SCALAR_WEBAPI_SPEAKER_INPUT, &inputs)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Audio in"));
        assert!(err.contains("HDMI"));
    }

    #[test]
    fn test_configured_speaker_input_uses_default_when_unset() {
        let api = ScalarWebApiDeviceConfig {
            enabled: true,
            model: "ScalarWebAPI device".into(),
            host: Some("127.0.0.1".into()),
            port: 10_000,
            path: concat!("so", "ny").into(),
            mac_output: crate::config::DeviceSelectorConfig::default(),
            triggers: vec![],
            wake_debounce_ms: 5_000,
            request_timeout_ms: 3_000,
            require_quick_start: true,
            speaker_input: None,
        };
        assert_eq!(
            configured_speaker_input_name(&api).as_deref(),
            Some(DEFAULT_SCALAR_WEBAPI_SPEAKER_INPUT)
        );
        assert!(speaker_input_uses_default(&api));
    }
}
