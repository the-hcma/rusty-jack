//! Resolve a configured device selector to a CoreAudio UID.

use crate::output_device::OutputDevice;

/// Pick a device by explicit UID.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceSelector {
    pub uid: Option<String>,
}

impl DeviceSelector {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.uid
            .as_deref()
            .is_none_or(crate::config::is_placeholder_uid)
    }
}

/// Outcome of resolving a [`DeviceSelector`] against live devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    NotSpecified,
    UidNotFound(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSpecified => write!(f, "no device selector configured"),
            Self::UidNotFound(uid) => write!(f, "device uid `{uid}` is not connected"),
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

    let Some(uid) = selector
        .uid
        .as_deref()
        .filter(|u| !crate::config::is_placeholder_uid(u))
    else {
        return Err(ResolveError::NotSpecified);
    };

    let device = devices
        .iter()
        .find(|d| d.uid == uid)
        .ok_or_else(|| ResolveError::UidNotFound(uid.to_string()))?;
    Ok(device.uid.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportKind;

    fn device(uid: &str, active: bool) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "HDMI".into(),
            transport: TransportKind::Hdmi,
            is_alive: true,
            is_default: false,
            is_active: active,
        }
    }

    #[test]
    fn test_resolve_by_uid() {
        let devices = vec![device("hdmi-1", true)];
        let selector = DeviceSelector {
            uid: Some("hdmi-1".into()),
        };
        assert_eq!(
            resolve_device_selector(&selector, &devices).unwrap(),
            "hdmi-1"
        );
    }

    #[test]
    fn test_resolve_by_uid_not_found() {
        let devices = vec![device("hdmi-1", true)];
        let selector = DeviceSelector {
            uid: Some("missing".into()),
        };

        assert!(matches!(
            resolve_device_selector(&selector, &devices),
            Err(ResolveError::UidNotFound(uid)) if uid == "missing"
        ));
    }
}
