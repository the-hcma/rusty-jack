//! Best-effort network access fingerprint and ScalarWebAPI reachability checks.

use crate::RustyJackError;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAccessSnapshot {
    pub interface: String,
    pub gateway: Option<String>,
    pub ip_address: Option<String>,
}

pub fn current_network_access_snapshot() -> Result<Option<NetworkAccessSnapshot>, RustyJackError> {
    platform_network_access_snapshot()
}

/// Return true when the Mac has a default route and interface address suitable for LAN wake.
#[must_use]
pub fn lan_connectivity_ready() -> bool {
    current_network_access_snapshot()
        .ok()
        .flatten()
        .is_some_and(|snapshot| {
            snapshot
                .ip_address
                .as_ref()
                .is_some_and(|ip| !ip.is_empty())
        })
}

/// Return true when ScalarWebAPI wake traffic should be attempted for `host`.
pub fn host_ready_for_scalar_webapi_wake(host: Option<&str>) -> Result<bool, RustyJackError> {
    let Some(host) = host.map(str::trim).filter(|host| !host.is_empty()) else {
        return Ok(false);
    };
    if !lan_connectivity_ready() {
        return Ok(false);
    }
    host_is_reachable(host)
}

#[cfg(target_os = "macos")]
fn platform_network_access_snapshot() -> Result<Option<NetworkAccessSnapshot>, RustyJackError> {
    let route = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .map_err(RustyJackError::Io)?;
    if !route.status.success() {
        return Ok(None);
    }

    let route = String::from_utf8_lossy(&route.stdout);
    let Some(interface) = parse_route_value(&route, "interface") else {
        return Ok(None);
    };
    let gateway = parse_route_value(&route, "gateway");
    let ip_address = interface_ipv4(&interface)?;

    Ok(Some(NetworkAccessSnapshot {
        interface,
        gateway,
        ip_address,
    }))
}

#[cfg(not(target_os = "macos"))]
fn platform_network_access_snapshot() -> Result<Option<NetworkAccessSnapshot>, RustyJackError> {
    Ok(None)
}

fn host_is_reachable(host: &str) -> Result<bool, RustyJackError> {
    platform_host_is_reachable(host)
}

#[cfg(target_os = "macos")]
fn platform_host_is_reachable(host: &str) -> Result<bool, RustyJackError> {
    use std::ffi::CString;

    use system_configuration::network_reachability::{ReachabilityFlags, SCNetworkReachability};

    let host = CString::new(host)
        .map_err(|_| RustyJackError::Config("ScalarWebAPI host is not valid UTF-8".into()))?;
    let Some(reachability) = SCNetworkReachability::from_host(&host) else {
        return Ok(false);
    };
    let flags = reachability
        .reachability()
        .map_err(|err| RustyJackError::Config(format!("reachability flags for {host:?}: {err}")))?;
    Ok(flags.intersects(ReachabilityFlags::REACHABLE))
}

#[cfg(not(target_os = "macos"))]
fn platform_host_is_reachable(_host: &str) -> Result<bool, RustyJackError> {
    Ok(true)
}

#[cfg(target_os = "macos")]
fn interface_ipv4(interface: &str) -> Result<Option<String>, RustyJackError> {
    let output = Command::new("ipconfig")
        .args(["getifaddr", interface])
        .output()
        .map_err(RustyJackError::Io)?;
    if !output.status.success() {
        return Ok(None);
    }

    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!ip.is_empty()).then_some(ip))
}

#[must_use]
pub fn parse_route_value(output: &str, key: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key)
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_route_value() {
        let output = r#"
   route to: default
destination: default
       mask: default
    gateway: 192.168.86.1
  interface: en0
"#;

        assert_eq!(
            parse_route_value(output, "interface").as_deref(),
            Some("en0")
        );
        assert_eq!(
            parse_route_value(output, "gateway").as_deref(),
            Some("192.168.86.1")
        );
        assert_eq!(parse_route_value(output, "missing"), None);
    }

    #[test]
    fn test_host_ready_for_scalar_webapi_wake_requires_host() {
        assert!(!host_ready_for_scalar_webapi_wake(None).unwrap());
        assert!(!host_ready_for_scalar_webapi_wake(Some("")).unwrap());
    }
}
