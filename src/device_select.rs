//! Resolve a configured device selector (UID or monitor name) to a CoreAudio UID.

use crate::output_device::OutputDevice;

/// Pick a device by explicit UID or by unique monitor product name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceSelector {
    pub uid: Option<String>,
    pub monitor_name: Option<String>,
}

impl DeviceSelector {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.uid
            .as_deref()
            .is_none_or(crate::config::is_placeholder_uid)
            && self
                .monitor_name
                .as_deref()
                .is_none_or(|n| n.trim().is_empty())
    }
}

/// Outcome of resolving a [`DeviceSelector`] against live devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    NotSpecified,
    UidNotFound(String),
    MonitorNotFound(String),
    MonitorAmbiguous {
        name: String,
        count: usize,
    },
    MonitorMismatch {
        uid: String,
        expected: String,
        actual: Option<String>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSpecified => write!(f, "no device selector configured"),
            Self::UidNotFound(uid) => write!(f, "device uid `{uid}` is not connected"),
            Self::MonitorNotFound(name) => {
                write!(f, "no connected output with monitor name `{name}`")
            }
            Self::MonitorAmbiguous { name, count } => write!(
                f,
                "monitor name `{name}` matches {count} outputs — set `uid` to disambiguate"
            ),
            Self::MonitorMismatch {
                uid,
                expected,
                actual,
            } => {
                if let Some(actual) = actual {
                    write!(
                        f,
                        "device uid `{uid}` is connected to monitor `{actual}`, but config expects `{expected}`"
                    )
                } else {
                    write!(
                        f,
                        "device uid `{uid}` has no monitor name, but config expects `{expected}`"
                    )
                }
            }
        }
    }
}

/// Resolve selector to a CoreAudio device UID.
pub fn resolve_device_selector(
    selector: &DeviceSelector,
    devices: &[OutputDevice],
) -> Result<String, ResolveError> {
    if selector.is_empty() {
        return Err(ResolveError::NotSpecified);
    }

    if let Some(uid) = selector
        .uid
        .as_deref()
        .filter(|u| !crate::config::is_placeholder_uid(u))
    {
        let device = devices
            .iter()
            .find(|d| d.uid == uid)
            .ok_or_else(|| ResolveError::UidNotFound(uid.to_string()))?;

        if let Some(expected) = selector
            .monitor_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let actual = device.monitor_name.as_deref().map(str::trim);
            if !actual.is_some_and(|name| name.eq_ignore_ascii_case(expected)) {
                return Err(ResolveError::MonitorMismatch {
                    uid: uid.to_string(),
                    expected: expected.to_string(),
                    actual: device.monitor_name.clone(),
                });
            }
        }

        return Ok(device.uid.clone());
    }

    if let Some(name) = selector
        .monitor_name
        .as_deref()
        .filter(|n| !n.trim().is_empty())
    {
        let matches: Vec<&OutputDevice> = devices
            .iter()
            .filter(|d| {
                d.monitor_name
                    .as_deref()
                    .is_some_and(|m| m.eq_ignore_ascii_case(name.trim()))
            })
            .collect();

        return match matches.len() {
            0 => Err(ResolveError::MonitorNotFound(name.to_string())),
            1 => Ok(matches[0].uid.clone()),
            n => Err(ResolveError::MonitorAmbiguous {
                name: name.to_string(),
                count: n,
            }),
        };
    }

    Err(ResolveError::NotSpecified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportKind;

    fn device(uid: &str, monitor: &str, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
            monitor_name: Some(monitor.into()),
        }
    }

    #[test]
    fn test_resolve_by_monitor_name_unique() {
        let devices = vec![device("hdmi-1", "DELL U3219Q", true)];
        let selector = DeviceSelector {
            uid: None,
            monitor_name: Some("DELL U3219Q".into()),
        };
        assert_eq!(
            resolve_device_selector(&selector, &devices).unwrap(),
            "hdmi-1"
        );
    }

    #[test]
    fn test_resolve_monitor_ambiguous() {
        let devices = vec![
            device("hdmi-1", "DELL U3219Q", true),
            device("hdmi-2", "DELL U3219Q", false),
        ];
        let selector = DeviceSelector {
            uid: None,
            monitor_name: Some("DELL U3219Q".into()),
        };
        assert!(matches!(
            resolve_device_selector(&selector, &devices),
            Err(ResolveError::MonitorAmbiguous { count: 2, .. })
        ));
    }

    #[test]
    fn test_resolve_by_uid() {
        let devices = vec![device("hdmi-1", "DELL U3219Q", true)];
        let selector = DeviceSelector {
            uid: Some("hdmi-1".into()),
            monitor_name: None,
        };
        assert_eq!(
            resolve_device_selector(&selector, &devices).unwrap(),
            "hdmi-1"
        );
    }

    #[test]
    fn test_resolve_by_uid_requires_matching_monitor_when_configured() {
        let devices = vec![device("hdmi-1", "DELL U3223QE", true)];
        let selector = DeviceSelector {
            uid: Some("hdmi-1".into()),
            monitor_name: Some("DELL U3219Q".into()),
        };

        assert!(matches!(
            resolve_device_selector(&selector, &devices),
            Err(ResolveError::MonitorMismatch { .. })
        ));
    }

    #[test]
    fn test_resolve_by_uid_allows_matching_monitor() {
        let devices = vec![device("hdmi-1", "DELL U3219Q", true)];
        let selector = DeviceSelector {
            uid: Some("hdmi-1".into()),
            monitor_name: Some("dell u3219q".into()),
        };

        assert_eq!(
            resolve_device_selector(&selector, &devices).unwrap(),
            "hdmi-1"
        );
    }
}
