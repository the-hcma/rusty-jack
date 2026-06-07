//! CoreAudio transport type (FourCC) parsing and HDMI-related classification.

use serde::Serialize;
use std::fmt;

#[cfg(target_os = "macos")]
use coreaudio_sys::{
    kAudioDeviceTransportTypeAggregate, kAudioDeviceTransportTypeAirPlay,
    kAudioDeviceTransportTypeAutoAggregate, kAudioDeviceTransportTypeBluetooth,
    kAudioDeviceTransportTypeBluetoothLE, kAudioDeviceTransportTypeBuiltIn,
    kAudioDeviceTransportTypeDisplayPort, kAudioDeviceTransportTypeFireWire,
    kAudioDeviceTransportTypeHDMI, kAudioDeviceTransportTypePCI,
    kAudioDeviceTransportTypeThunderbolt, kAudioDeviceTransportTypeUSB,
    kAudioDeviceTransportTypeVirtual,
};

/// Known `kAudioDevicePropertyTransportType` values (FourCC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    BuiltIn,
    Hdmi,
    DisplayPort,
    Thunderbolt,
    Usb,
    Bluetooth,
    BluetoothLe,
    AirPlay,
    Pci,
    FireWire,
    Aggregate,
    Virtual,
    Unknown,
}

impl TransportKind {
    #[must_use]
    #[allow(non_upper_case_globals)]
    pub fn from_fourcc(code: u32) -> Self {
        #[cfg(target_os = "macos")]
        {
            match code {
                kAudioDeviceTransportTypeBuiltIn => Self::BuiltIn,
                kAudioDeviceTransportTypeHDMI => Self::Hdmi,
                kAudioDeviceTransportTypeDisplayPort => Self::DisplayPort,
                kAudioDeviceTransportTypeThunderbolt => Self::Thunderbolt,
                kAudioDeviceTransportTypeUSB => Self::Usb,
                kAudioDeviceTransportTypeBluetooth => Self::Bluetooth,
                kAudioDeviceTransportTypeBluetoothLE => Self::BluetoothLe,
                kAudioDeviceTransportTypeAirPlay => Self::AirPlay,
                kAudioDeviceTransportTypePCI => Self::Pci,
                kAudioDeviceTransportTypeFireWire => Self::FireWire,
                kAudioDeviceTransportTypeAggregate | kAudioDeviceTransportTypeAutoAggregate => {
                    Self::Aggregate
                }
                kAudioDeviceTransportTypeVirtual => Self::Virtual,
                _ => Self::Unknown,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = code;
            Self::Unknown
        }
    }

    /// External display / dock audio paths useful for Rusty Jack.
    #[must_use]
    pub fn is_hdmi_class(self) -> bool {
        matches!(
            self,
            Self::Hdmi | Self::DisplayPort | Self::Thunderbolt | Self::Usb
        )
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::Hdmi => "hdmi",
            Self::DisplayPort => "displayport",
            Self::Thunderbolt => "thunderbolt",
            Self::Usb => "usb",
            Self::Bluetooth => "bluetooth",
            Self::BluetoothLe => "bluetooth-le",
            Self::AirPlay => "airplay",
            Self::Pci => "pci",
            Self::FireWire => "firewire",
            Self::Aggregate => "aggregate",
            Self::Virtual => "virtual",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for TransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    #[allow(non_upper_case_globals)]
    fn test_hdmi_constant() {
        assert_eq!(
            TransportKind::from_fourcc(kAudioDeviceTransportTypeHDMI),
            TransportKind::Hdmi
        );
        assert!(TransportKind::Hdmi.is_hdmi_class());
    }

    #[test]
    fn test_builtin_not_hdmi_class() {
        assert!(!TransportKind::BuiltIn.is_hdmi_class());
    }
}
