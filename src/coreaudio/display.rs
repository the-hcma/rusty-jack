//! Connected display metadata used to name HDMI/DisplayPort audio outputs.

#![allow(unsafe_code)]

use crate::transport::TransportKind;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryGetValueIfPresent, CFDictionaryRef};
use core_foundation::string::CFString;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

type CgDirectDisplayId = u32;
type CgError = i32;
type IoService = u32;

const CG_ERROR_SUCCESS: CgError = 0;
const IO_DISPLAY_ONLY_PREFERRED_NAME: u32 = 0x0000_0004;
const DISPLAY_PRODUCT_NAME_KEY: &str = "DisplayProductName";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioDisplayIdentity {
    vendor: u16,
    product: u32,
    serial: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayInfo {
    vendor: u32,
    product: u32,
    serial: u32,
    name: Option<String>,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGGetOnlineDisplayList(
        max_displays: u32,
        active_displays: *mut CgDirectDisplayId,
        display_count: *mut u32,
    ) -> CgError;
    fn CGDisplayVendorNumber(display: CgDirectDisplayId) -> u32;
    fn CGDisplayModelNumber(display: CgDirectDisplayId) -> u32;
    fn CGDisplaySerialNumber(display: CgDirectDisplayId) -> u32;
    fn CGDisplayIOServicePort(display: CgDirectDisplayId) -> IoService;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IODisplayCreateInfoDictionary(service: IoService, options: u32) -> CFDictionaryRef;
}

/// Prefer the connected monitor model for live display-backed audio ports.
pub(crate) fn preferred_output_name(
    uid: &str,
    coreaudio_name: &str,
    transport: TransportKind,
    is_alive: bool,
) -> String {
    if is_alive && transport.is_hdmi_class() {
        if let Some(name) = connected_display_name_for_audio_uid(uid) {
            return name;
        }
    }

    coreaudio_name.to_string()
}

fn connected_display_name_for_audio_uid(uid: &str) -> Option<String> {
    let identity = parse_audio_display_identity(uid)?;
    online_displays()
        .into_iter()
        .find(|display| display_matches_audio_identity(display, identity))
        .and_then(|display| display.name)
}

fn online_displays() -> Vec<DisplayInfo> {
    let mut count = 0u32;
    let status = unsafe { CGGetOnlineDisplayList(0, null_mut(), &mut count) };
    if status != CG_ERROR_SUCCESS || count == 0 {
        return Vec::new();
    }

    let mut displays = vec![0u32; count as usize];
    let status = unsafe { CGGetOnlineDisplayList(count, displays.as_mut_ptr(), &mut count) };
    if status != CG_ERROR_SUCCESS {
        return Vec::new();
    }
    displays.truncate(count as usize);

    displays
        .into_iter()
        .map(|display| DisplayInfo {
            vendor: unsafe { CGDisplayVendorNumber(display) },
            product: unsafe { CGDisplayModelNumber(display) },
            serial: unsafe { CGDisplaySerialNumber(display) },
            name: display_product_name(display),
        })
        .collect()
}

fn display_product_name(display: CgDirectDisplayId) -> Option<String> {
    let service = unsafe { CGDisplayIOServicePort(display) };
    if service == 0 {
        return None;
    }

    let info_ref =
        unsafe { IODisplayCreateInfoDictionary(service, IO_DISPLAY_ONLY_PREFERRED_NAME) };
    if info_ref.is_null() {
        return None;
    }

    let info = unsafe { CFDictionary::wrap_under_create_rule(info_ref) };
    let key = CFString::from_static_string(DISPLAY_PRODUCT_NAME_KEY);
    let names = dictionary_get_cf_type(&info, &key)?.downcast_into::<CFDictionary>()?;

    let (_keys, values) = names.get_keys_and_values();
    values.into_iter().find_map(cfstring_value)
}

fn dictionary_get_cf_type(dictionary: &CFDictionary, key: &CFString) -> Option<CFType> {
    let mut value = null();
    let present = unsafe {
        CFDictionaryGetValueIfPresent(
            dictionary.as_concrete_TypeRef(),
            key.as_CFTypeRef().cast::<c_void>(),
            &mut value,
        )
    };
    if present == 0 || value.is_null() {
        return None;
    }

    Some(unsafe { CFType::wrap_under_get_rule(value.cast()) })
}

fn cfstring_value(value: *const c_void) -> Option<String> {
    if value.is_null() {
        return None;
    }

    let value = unsafe { CFType::wrap_under_get_rule(value.cast()) };
    let name = value.downcast_into::<CFString>()?.to_string();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn parse_audio_display_identity(uid: &str) -> Option<AudioDisplayIdentity> {
    let identity = uid.rsplit_once('{')?.1.strip_suffix('}')?;
    let mut parts = identity.split('-');
    let vendor = u16::from_str_radix(parts.next()?, 16).ok()?;
    let product = u32::from_str_radix(parts.next()?, 16).ok()?;
    let serial = u32::from_str_radix(parts.next()?, 16).ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some(AudioDisplayIdentity {
        vendor,
        product,
        serial,
    })
}

fn display_matches_audio_identity(display: &DisplayInfo, identity: AudioDisplayIdentity) -> bool {
    display_vendor_matches_audio(display.vendor, identity.vendor)
        && display.product == identity.product
        && (display.serial == 0 || identity.serial == 0 || display.serial == identity.serial)
}

fn display_vendor_matches_audio(display_vendor: u32, audio_vendor: u16) -> bool {
    let display_vendor = display_vendor & 0xffff;
    display_vendor == u32::from(audio_vendor)
        || display_vendor == u32::from(audio_vendor.swap_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_audio_display_identity() {
        assert_eq!(
            parse_audio_display_identity("AppleHDAEngineOutputDP:0,1,0,1,0:0:{AC10-A120-30594A4C}"),
            Some(AudioDisplayIdentity {
                vendor: 0xac10,
                product: 0xa120,
                serial: 0x3059_4a4c,
            })
        );
    }

    #[test]
    fn test_display_identity_matches_byte_swapped_vendor() {
        let display = DisplayInfo {
            vendor: 0x10ac,
            product: 0xa120,
            serial: 0x3059_4a4c,
            name: Some("DELL U3219Q".into()),
        };
        let identity = AudioDisplayIdentity {
            vendor: 0xac10,
            product: 0xa120,
            serial: 0x3059_4a4c,
        };

        assert!(display_matches_audio_identity(&display, identity));
    }

    #[test]
    fn test_display_identity_rejects_different_product() {
        let display = DisplayInfo {
            vendor: 0x10ac,
            product: 0x4273,
            serial: 0x424d_414c,
            name: Some("DELL U3223QE".into()),
        };
        let identity = AudioDisplayIdentity {
            vendor: 0xac10,
            product: 0xa120,
            serial: 0x3059_4a4c,
        };

        assert!(!display_matches_audio_identity(&display, identity));
    }

    #[test]
    fn test_preferred_output_name_keeps_generic_when_not_alive() {
        assert_eq!(
            preferred_output_name(
                "AppleHDAEngineOutputDP:0,1,0,1,0:0:{AC10-A120-30594A4C}",
                "HDMI",
                TransportKind::Hdmi,
                false,
            ),
            "HDMI"
        );
    }
}
