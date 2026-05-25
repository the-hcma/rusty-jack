//! CoreAudio property reads via `coreaudio-sys`.

#![allow(
    unsafe_code,
    clippy::cast_ptr_alignment,
    clippy::ptr_as_ptr,
    clippy::borrow_as_ptr,
    clippy::ptr_cast_constness,
    clippy::cast_possible_truncation
)]

use crate::RustyJackError;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use coreaudio_sys::{
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyScopeOutput, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyTransportType, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioBufferList, AudioDeviceID,
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress,
};
use std::ffi::c_void;
use std::ptr::null_mut;

fn property_address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMaster,
    }
}

fn ok(status: i32) -> bool {
    status == 0
}

fn get_property_u32(
    device_id: AudioObjectID,
    selector: u32,
    scope: u32,
) -> Result<u32, RustyJackError> {
    let address = property_address(selector, scope);
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            null_mut(),
            &mut size,
            &mut value as *mut u32 as *mut c_void,
        )
    };
    if !ok(status) {
        return Err(RustyJackError::CoreAudio(format!(
            "get_property_u32({selector}) status {status}"
        )));
    }
    Ok(value)
}

fn get_property_cfstring(
    device_id: AudioObjectID,
    selector: u32,
    scope: u32,
) -> Result<String, RustyJackError> {
    let address = property_address(selector, scope);
    let mut value: *const c_void = null_mut();
    let mut size = std::mem::size_of::<*const c_void>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            null_mut(),
            &mut size,
            &mut value as *mut *const c_void as *mut c_void,
        )
    };
    if !ok(status) || value.is_null() {
        return Err(RustyJackError::CoreAudio(format!(
            "get_property_cfstring({selector}) status {status}"
        )));
    }
    let cf = unsafe { CFString::wrap_under_get_rule(value as *mut _) };
    Ok(cf.to_string())
}

fn try_get_property_cfstring(
    device_id: AudioObjectID,
    selector: u32,
    scope: u32,
) -> Option<String> {
    get_property_cfstring(device_id, selector, scope).ok()
}

fn try_get_property_u32(device_id: AudioObjectID, selector: u32, scope: u32) -> Option<u32> {
    get_property_u32(device_id, selector, scope).ok()
}

/// All HAL device IDs.
pub fn all_device_ids() -> Result<Vec<AudioDeviceID>, RustyJackError> {
    let address = property_address(
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    );
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &address, 0, null_mut(), &mut size)
    };
    if !ok(status) {
        return Err(RustyJackError::CoreAudio(format!(
            "devices size status {status}"
        )));
    }

    let count = size as usize / std::mem::size_of::<AudioDeviceID>();
    let mut ids = vec![0u32; count];
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            null_mut(),
            &mut size,
            ids.as_mut_ptr().cast(),
        )
    };
    if !ok(status) {
        return Err(RustyJackError::CoreAudio(format!(
            "devices data status {status}"
        )));
    }
    Ok(ids)
}

pub fn default_output_device_id() -> Option<AudioDeviceID> {
    system_property_device_id(kAudioHardwarePropertyDefaultOutputDevice)
}

fn system_property_device_id(selector: u32) -> Option<AudioDeviceID> {
    let address = property_address(selector, kAudioObjectPropertyScopeGlobal);
    let mut id: AudioDeviceID = 0;
    let mut size = std::mem::size_of::<AudioDeviceID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            null_mut(),
            &mut size,
            &mut id as *mut AudioDeviceID as *mut c_void,
        )
    };
    if ok(status) && id != 0 {
        Some(id)
    } else {
        None
    }
}

pub fn device_uid(device_id: AudioDeviceID) -> Result<String, RustyJackError> {
    get_property_cfstring(
        device_id,
        kAudioDevicePropertyDeviceUID,
        kAudioObjectPropertyScopeGlobal,
    )
}

pub fn device_name(device_id: AudioDeviceID) -> Result<String, RustyJackError> {
    use coreaudio_sys::kAudioDevicePropertyDeviceNameCFString;
    get_property_cfstring(
        device_id,
        kAudioDevicePropertyDeviceNameCFString,
        kAudioDevicePropertyScopeOutput,
    )
}

pub fn device_transport_type(device_id: AudioDeviceID) -> Result<u32, RustyJackError> {
    get_property_u32(
        device_id,
        kAudioDevicePropertyTransportType,
        kAudioObjectPropertyScopeGlobal,
    )
}

pub fn device_is_alive(device_id: AudioDeviceID) -> Result<bool, RustyJackError> {
    let v = get_property_u32(
        device_id,
        kAudioDevicePropertyDeviceIsAlive,
        kAudioDevicePropertyScopeOutput,
    )?;
    Ok(v != 0)
}

pub fn device_manufacturer(device_id: AudioDeviceID) -> Option<String> {
    use coreaudio_sys::kAudioDevicePropertyDeviceManufacturerCFString;
    try_get_property_cfstring(
        device_id,
        kAudioDevicePropertyDeviceManufacturerCFString,
        kAudioObjectPropertyScopeGlobal,
    )
}

pub fn device_model_uid(device_id: AudioDeviceID) -> Option<String> {
    use coreaudio_sys::kAudioDevicePropertyModelUID;
    try_get_property_cfstring(
        device_id,
        kAudioDevicePropertyModelUID,
        kAudioObjectPropertyScopeGlobal,
    )
}

pub fn device_plugin_bundle_id(device_id: AudioDeviceID) -> Option<String> {
    use coreaudio_sys::{kAudioDevicePropertyPlugIn, kAudioPlugInPropertyBundleID};
    let plugin_id = try_get_property_u32(
        device_id,
        kAudioDevicePropertyPlugIn,
        kAudioObjectPropertyScopeGlobal,
    )?;
    if plugin_id == 0 {
        return None;
    }
    try_get_property_cfstring(
        plugin_id,
        kAudioPlugInPropertyBundleID,
        kAudioObjectPropertyScopeGlobal,
    )
}

/// True when the device has at least one output channel in its stream configuration.
pub fn device_has_output_streams(device_id: AudioDeviceID) -> bool {
    let address = property_address(
        kAudioDevicePropertyStreamConfiguration,
        kAudioDevicePropertyScopeOutput,
    );
    let mut size = 0u32;
    let status =
        unsafe { AudioObjectGetPropertyDataSize(device_id, &address, 0, null_mut(), &mut size) };
    if !ok(status) || size == 0 {
        return false;
    }

    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            null_mut(),
            &mut size,
            buffer.as_mut_ptr().cast(),
        )
    };
    if !ok(status) {
        return false;
    }

    let list = buffer.as_ptr().cast::<AudioBufferList>();
    if list.is_null() {
        return false;
    }
    let n = unsafe { (*list).mNumberBuffers };
    for i in 0..n {
        let channels = unsafe { (*list).mBuffers[i as usize].mNumberChannels };
        if channels > 0 {
            return true;
        }
    }
    false
}

#[cfg(all(test, target_os = "macos"))]
mod hardware_tests {
    use super::*;

    #[test]
    #[ignore = "requires live CoreAudio hardware"]
    fn test_probe_default_device_properties() {
        use coreaudio_sys::{
            kAudioDevicePropertyDeviceManufacturerCFString, kAudioDevicePropertyModelUID,
            kAudioDevicePropertyPlugIn, kAudioPlugInPropertyBundleID,
        };

        let Some(id) = default_output_device_id() else {
            eprintln!("no default");
            return;
        };

        for (label, res) in [
            ("uid", device_uid(id)),
            ("name", device_name(id)),
            (
                "manufacturer",
                get_property_cfstring(
                    id,
                    kAudioDevicePropertyDeviceManufacturerCFString,
                    kAudioObjectPropertyScopeGlobal,
                ),
            ),
            (
                "model_uid",
                get_property_cfstring(
                    id,
                    kAudioDevicePropertyModelUID,
                    kAudioObjectPropertyScopeGlobal,
                ),
            ),
            (
                "transport",
                device_transport_type(id).map(|t| format!("{t}")),
            ),
        ] {
            eprintln!("{label}: {res:?}");
        }

        if let Ok(plugin_id) = get_property_u32(
            id,
            kAudioDevicePropertyPlugIn,
            kAudioObjectPropertyScopeGlobal,
        ) {
            eprintln!("plugin_id: {plugin_id}");
            if let Ok(bundle) = get_property_cfstring(
                plugin_id,
                kAudioPlugInPropertyBundleID,
                kAudioObjectPropertyScopeGlobal,
            ) {
                eprintln!("plugin bundle: {bundle}");
            }
        }
    }
}
