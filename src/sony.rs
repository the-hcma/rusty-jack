//! Sony ScalarWebAPI wake support for line-out attached speakers.

use crate::config::{Config, SonySpeakerConfig};
use crate::device_select::resolve_device_selector;
use crate::output_device::OutputDevice;
use crate::RustyJackError;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::Message;

const OUTPUT_SELECTED_TRIGGER: &str = "output_selected";
const SYSTEM_SERVICE: &str = "system";
const SSDP_ADDR: &str = "239.255.255.250:1900";
const SCALAR_WEBAPI_ST: &str = "urn:schemas-sony-com:service:ScalarWebAPI:1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SonyWakeResult {
    pub endpoint: String,
    pub status_code: u16,
    pub previous_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScalarEndpoint {
    host: String,
    port: u16,
    path: String,
}

impl ScalarEndpoint {
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

/// Try waking the configured Sony speaker when its Mac output is selected.
pub fn wake_on_output_selected(
    config: &Config,
    devices: &[OutputDevice],
    selected_uid: &str,
) -> Result<Option<SonyWakeResult>, RustyJackError> {
    let Some(sony) = config.sony_speaker.as_ref() else {
        return Ok(None);
    };
    if !sony.enabled || !trigger_enabled(sony, OUTPUT_SELECTED_TRIGGER) {
        return Ok(None);
    }

    let selector = sony.mac_output.clone().into();
    let sony_output_uid = resolve_device_selector(&selector, devices)
        .map_err(|err| RustyJackError::Config(format!("sony_speaker.mac_output: {err}")))?;
    if sony_output_uid != selected_uid {
        return Ok(None);
    }

    let previous_status = current_power_status(sony).ok();
    let mut result = send_wake_command(sony)?;
    result.previous_status = previous_status;
    Ok(Some(result))
}

/// Log Sony wake failures as warnings so audio routing still succeeds.
pub fn warn_on_output_selected(config: &Config, devices: &[OutputDevice], selected_uid: &str) {
    match wake_on_output_selected(config, devices, selected_uid) {
        Ok(Some(result)) => eprintln!("{}", format_wake_message(&result)),
        Ok(None) => {}
        Err(err) => eprintln!("warning: {err}"),
    }
}

#[must_use]
pub fn format_wake_message(result: &SonyWakeResult) -> String {
    if result
        .previous_status
        .as_deref()
        .is_some_and(|status| status.eq_ignore_ascii_case("standby"))
    {
        format!(
            "Sony speaker was standby; waking it via {}.",
            result.endpoint
        )
    } else {
        format!("Sent Sony wake command to {}.", result.endpoint)
    }
}

/// Picker row annotations for configured Sony speaker outputs.
#[must_use]
pub fn picker_power_notes(config: &Config, devices: &[OutputDevice]) -> Vec<(String, String)> {
    let Some(sony) = config.sony_speaker.as_ref() else {
        return vec![];
    };
    if !sony.enabled {
        return vec![];
    }

    let selector = sony.mac_output.clone().into();
    let uid = match resolve_device_selector(&selector, devices) {
        Ok(uid) => uid,
        Err(err) => {
            eprintln!("warning: sony_speaker.mac_output: {err}");
            return vec![];
        }
    };
    let note = match current_power_status(sony) {
        Ok(status) => format!("Sony: {status}"),
        Err(err) => {
            eprintln!("warning: could not read Sony speaker power state: {err}");
            "Sony: unknown".into()
        }
    };

    vec![(uid, note)]
}

fn trigger_enabled(sony: &SonySpeakerConfig, trigger: &str) -> bool {
    sony.triggers
        .iter()
        .any(|value| value.eq_ignore_ascii_case(trigger))
}

fn current_power_status(sony: &SonySpeakerConfig) -> Result<String, RustyJackError> {
    let endpoint = match discover_scalar_endpoint(sony)? {
        Some(endpoint) => endpoint,
        None => configured_endpoint(sony)?,
    };
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
        sony.request_timeout_ms,
    )
    .or_else(|_| {
        post_json(
            &endpoint.host,
            endpoint.port,
            &endpoint.service_path(SYSTEM_SERVICE),
            &payload,
            sony.request_timeout_ms,
        )
    })?;
    power_status_from_response(&response)
}

fn send_wake_command(sony: &SonySpeakerConfig) -> Result<SonyWakeResult, RustyJackError> {
    let discovered = match discover_scalar_endpoint(sony) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            eprintln!("warning: Sony endpoint discovery failed: {err}");
            None
        }
    };
    if let Some(endpoint) = discovered.as_ref() {
        match send_wake_command_to(sony, endpoint) {
            Ok(result) => return Ok(result),
            Err(err) => eprintln!("warning: discovered Sony endpoint failed: {err}"),
        }
    }

    let configured = configured_endpoint(sony)?;
    send_wake_command_to(sony, &configured)
}

fn send_wake_command_to(
    sony: &SonySpeakerConfig,
    scalar_endpoint: &ScalarEndpoint,
) -> Result<SonyWakeResult, RustyJackError> {
    let endpoint = scalar_endpoint.service_endpoint(SYSTEM_SERVICE);
    let path = scalar_endpoint.service_path(SYSTEM_SERVICE);
    let wake_id = prime_scalar_services(sony, scalar_endpoint)?;
    let payload = wake_payload(wake_id);

    let response = websocket_json(
        &scalar_endpoint.host,
        scalar_endpoint.port,
        &path,
        &payload,
        sony.request_timeout_ms,
    )
    .or_else(|err| {
        eprintln!("warning: Sony WebSocket wake failed: {err}");
        post_json(
            &scalar_endpoint.host,
            scalar_endpoint.port,
            &path,
            &payload,
            sony.request_timeout_ms,
        )
    })?;

    ensure_success_json(&response, &endpoint)?;
    let status_code = parse_http_status(&response)?;
    if !(200..300).contains(&status_code) {
        return Err(RustyJackError::Speaker(format!(
            "{endpoint} returned HTTP {status_code}"
        )));
    }

    Ok(SonyWakeResult {
        endpoint,
        status_code,
        previous_status: None,
    })
}

fn prime_scalar_services(
    sony: &SonySpeakerConfig,
    scalar_endpoint: &ScalarEndpoint,
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
        &scalar_endpoint.host,
        scalar_endpoint.port,
        &scalar_endpoint.service_path("guide"),
        &guide_payload,
        sony.request_timeout_ms,
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
            &scalar_endpoint.host,
            scalar_endpoint.port,
            &scalar_endpoint.service_path(service),
            &method_types_payload,
            sony.request_timeout_ms,
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

fn sony_host(sony: &SonySpeakerConfig) -> Result<&str, RustyJackError> {
    sony.host
        .as_deref()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| RustyJackError::Config("sony_speaker.host is not set".into()))
}

fn configured_endpoint(sony: &SonySpeakerConfig) -> Result<ScalarEndpoint, RustyJackError> {
    let host = sony_host(sony)?.to_string();
    let path = format!("/{}", sony.path.trim().trim_matches('/'))
        .trim_end_matches('/')
        .to_string();
    Ok(ScalarEndpoint {
        host,
        port: sony.port,
        path,
    })
}

fn discover_scalar_endpoint(
    sony: &SonySpeakerConfig,
) -> Result<Option<ScalarEndpoint>, RustyJackError> {
    let host = sony_host(sony)?;
    let target_ips = resolve_host_ips(host)?;
    if target_ips.is_empty() {
        return Ok(None);
    }

    let timeout = Duration::from_millis(sony.request_timeout_ms.max(1));
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(RustyJackError::Io)?;
    socket
        .set_read_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;
    let request = format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {SSDP_ADDR}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 1\r\n\
         ST: {SCALAR_WEBAPI_ST}\r\n\r\n"
    );
    socket
        .send_to(request.as_bytes(), SSDP_ADDR)
        .map_err(|err| RustyJackError::Speaker(format!("could not send SSDP discovery: {err}")))?;

    let mut buf = [0_u8; 4096];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, addr)) => {
                if !target_ips.contains(&addr.ip()) {
                    continue;
                }
                let response = String::from_utf8_lossy(&buf[..len]);
                let Some(location) = http_header(&response, "location") else {
                    continue;
                };
                let xml = http_get(location, timeout)?;
                if let Some(base_url) = extract_xml_text(&xml, "X_ScalarWebAPI_BaseURL") {
                    return parse_http_url(&base_url).map(Some);
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(err) => {
                return Err(RustyJackError::Speaker(format!(
                    "could not read SSDP response: {err}"
                )));
            }
        }
    }
}

fn resolve_host_ips(host: &str) -> Result<Vec<IpAddr>, RustyJackError> {
    (host, 0)
        .to_socket_addrs()
        .map_err(|err| RustyJackError::Speaker(format!("could not resolve {host}: {err}")))
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
    let response = send_http(&endpoint.host, endpoint.port, &request, timeout)?;
    let status_code = parse_http_status(&response)?;
    if !(200..300).contains(&status_code) {
        return Err(RustyJackError::Speaker(format!(
            "{url} returned HTTP {status_code}"
        )));
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
    send_http(host, port, &request, timeout)
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
        RustyJackError::Speaker(format!("could not build WebSocket request: {err}"))
    })?;
    let address: SocketAddr = (host, port)
        .to_socket_addrs()
        .map_err(|err| RustyJackError::Speaker(format!("could not resolve {host}: {err}")))?
        .next()
        .ok_or_else(|| RustyJackError::Speaker(format!("could not resolve {host}")))?;
    let stream = TcpStream::connect_timeout(&address, timeout).map_err(|err| {
        RustyJackError::Speaker(format!("could not connect to {host}:{port}: {err}"))
    })?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;

    let (mut socket, response) = tungstenite::client(request, stream)
        .map_err(|err| RustyJackError::Speaker(format!("WebSocket handshake failed: {err}")))?;
    if response.status().as_u16() != 101 {
        return Err(RustyJackError::Speaker(format!(
            "WebSocket upgrade returned HTTP {}",
            response.status()
        )));
    }

    socket
        .send(Message::Text(body.to_string().into()))
        .map_err(|err| RustyJackError::Speaker(format!("could not send WebSocket frame: {err}")))?;
    let message = socket.read().map_err(|err| {
        RustyJackError::Speaker(format!("could not read WebSocket response: {err}"))
    })?;
    let _ = socket.close(None);
    let body = match message {
        Message::Text(text) => text.to_string(),
        Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Message::Close(_) => {
            return Err(RustyJackError::Speaker(
                "Sony speaker closed WebSocket before response".into(),
            ));
        }
        other => {
            return Err(RustyJackError::Speaker(format!(
                "unexpected WebSocket response: {other:?}"
            )));
        }
    };
    Ok(format!("HTTP/1.1 200 OK\r\n\r\n{body}"))
}

fn send_http(
    host: &str,
    port: u16,
    request: &str,
    timeout: Duration,
) -> Result<String, RustyJackError> {
    let address: SocketAddr = (host, port)
        .to_socket_addrs()
        .map_err(|err| RustyJackError::Speaker(format!("could not resolve {host}: {err}")))?
        .next()
        .ok_or_else(|| RustyJackError::Speaker(format!("could not resolve {host}")))?;
    let mut stream = TcpStream::connect_timeout(&address, timeout).map_err(|err| {
        RustyJackError::Speaker(format!("could not connect to {host}:{port}: {err}"))
    })?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(RustyJackError::Io)?;

    stream
        .write_all(request.as_bytes())
        .map_err(|err| RustyJackError::Speaker(format!("could not send HTTP request: {err}")))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|err| RustyJackError::Speaker(format!("could not read HTTP response: {err}")))?;
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
        return Err(RustyJackError::Speaker(format!(
            "{label} returned error payload: {body}"
        )));
    }
    if body.contains(expected) {
        return Ok(());
    }
    Err(RustyJackError::Speaker(format!(
        "{label} returned unexpected payload: {body}"
    )))
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
        .ok_or_else(|| RustyJackError::Speaker("invalid HTTP response from Sony speaker".into()))
}

fn parse_http_url(url: &str) -> Result<ScalarEndpoint, RustyJackError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| RustyJackError::Speaker(format!("unsupported Sony endpoint URL: {url}")))?;
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        let port = port.parse().map_err(|_| {
            RustyJackError::Speaker(format!("invalid Sony endpoint port in URL: {url}"))
        })?;
        (host, port)
    } else {
        (host_port, 80)
    };
    if host.is_empty() {
        return Err(RustyJackError::Speaker(format!(
            "invalid Sony endpoint URL: {url}"
        )));
    }
    Ok(ScalarEndpoint {
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
            monitor_name: None,
        }
    }

    fn config_for(uid: &str) -> Config {
        Config {
            version: 1,
            auto_switch: true,
            preferred_device: DeviceSelectorConfig {
                uid: Some(uid.into()),
                monitor_name: None,
            },
            preferred_device_uid: None,
            fallback_uids: vec![],
            also_set_system_output: true,
            volume: None,
            sony_speaker: Some(SonySpeakerConfig {
                enabled: true,
                model: "SRS-ZR5".into(),
                host: Some("sony.house.hcma".into()),
                port: 10_000,
                path: "sony".into(),
                mac_output: DeviceSelectorConfig {
                    uid: Some(uid.into()),
                    monitor_name: None,
                },
                triggers: vec![OUTPUT_SELECTED_TRIGGER.into()],
                wake_debounce_ms: 30_000,
                request_timeout_ms: 3_000,
                require_quick_start: true,
            }),
        }
    }

    #[test]
    fn test_configured_endpoint_uses_slash_path() {
        let config = config_for("line-out");
        let sony = config.sony_speaker.as_ref().unwrap();
        let endpoint = configured_endpoint(sony).unwrap();
        assert_eq!(
            endpoint.service_endpoint(SYSTEM_SERVICE),
            "http://sony.house.hcma:10000/sony/system"
        );
        assert_eq!(endpoint.service_path(SYSTEM_SERVICE), "/sony/system");
    }

    #[test]
    fn test_parse_discovered_endpoint_url() {
        let endpoint = parse_http_url("http://192.168.86.18:54480/sony").unwrap();
        assert_eq!(endpoint.host, "192.168.86.18");
        assert_eq!(endpoint.port, 54_480);
        assert_eq!(endpoint.path, "/sony");
        assert_eq!(
            endpoint.service_endpoint(SYSTEM_SERVICE),
            "http://192.168.86.18:54480/sony/system"
        );
    }

    #[test]
    fn test_extract_scalar_base_url_from_upnp_xml() {
        let xml = r#"
<root>
  <av:X_ScalarWebAPI_BaseURL xmlns:av="urn:schemas-sony-com:av">http://192.168.86.18:54480/sony</av:X_ScalarWebAPI_BaseURL>
</root>"#;
        assert_eq!(
            extract_xml_text(xml, "X_ScalarWebAPI_BaseURL").as_deref(),
            Some("http://192.168.86.18:54480/sony")
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
        let sony = config.sony_speaker.as_mut().unwrap();
        sony.triggers = vec!["Output_Selected".into()];
        assert!(trigger_enabled(sony, OUTPUT_SELECTED_TRIGGER));
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
    fn test_format_wake_message_mentions_waking_from_standby() {
        let message = format_wake_message(&SonyWakeResult {
            endpoint: "http://speaker/sony/system".into(),
            status_code: 200,
            previous_status: Some("standby".into()),
        });
        assert!(message.contains("was standby"));
        assert!(message.contains("waking"));
    }

    #[test]
    fn test_format_wake_message_generic_when_status_unknown() {
        let message = format_wake_message(&SonyWakeResult {
            endpoint: "http://speaker/sony/system".into(),
            status_code: 200,
            previous_status: None,
        });
        assert!(message.starts_with("Sent Sony wake command"));
    }

    #[test]
    fn test_selection_filter_skips_non_sony_output_before_network() {
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
        config.sony_speaker.as_mut().unwrap().enabled = false;
        let devices = vec![device("line-out")];
        assert_eq!(
            wake_on_output_selected(&config, &devices, "line-out").unwrap(),
            None
        );
    }
}
