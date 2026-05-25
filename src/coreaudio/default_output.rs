//! Set the system default output device via CoreAudio HAL.

#![allow(
    unsafe_code,
    clippy::cast_possible_truncation,
    clippy::ptr_as_ptr,
    clippy::borrow_as_ptr
)]

use crate::coreaudio::property::{all_device_ids, default_output_device_id, device_uid};
use crate::RustyJackError;
use coreaudio_sys::{
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDefaultSystemOutputDevice,
    kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioDeviceID, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
};
use std::ffi::c_void;
use std::ptr::null_mut;

fn property_address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    }
}

fn ok(status: i32) -> bool {
    status == 0
}

fn set_system_default(selector: u32, device_id: AudioDeviceID) -> Result<(), RustyJackError> {
    let address = property_address(selector);
    let status = unsafe {
        AudioObjectSetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            null_mut(),
            std::mem::size_of::<AudioDeviceID>() as u32,
            &device_id as *const AudioDeviceID as *const c_void,
        )
    };
    if !ok(status) {
        return Err(RustyJackError::CoreAudio(format!(
            "set default output (selector {selector}) status {status}"
        )));
    }
    Ok(())
}

/// CoreAudio device ID for a UID, when the device is present.
pub fn device_id_for_uid(uid: &str) -> Result<AudioDeviceID, RustyJackError> {
    for id in all_device_ids()? {
        if device_uid(id).ok().as_deref() == Some(uid) {
            return Ok(id);
        }
    }
    Err(RustyJackError::CoreAudio(format!(
        "no device with uid `{uid}`"
    )))
}

/// UID of the current default output device.
pub fn default_output_uid() -> Result<Option<String>, RustyJackError> {
    let Some(id) = default_output_device_id() else {
        return Ok(None);
    };
    Ok(Some(device_uid(id)?))
}

/// Set the system default output (and optionally alert/sound-effects output).
pub fn set_default_output(uid: &str, also_system: bool) -> Result<(), RustyJackError> {
    let device_id = device_id_for_uid(uid)?;
    set_system_default(kAudioHardwarePropertyDefaultOutputDevice, device_id)?;
    if also_system {
        set_system_default(kAudioHardwarePropertyDefaultSystemOutputDevice, device_id)?;
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod hardware_tests {
    use super::*;

    #[test]
    #[ignore = "mutates system audio default"]
    fn test_builtin_default_shows_active() {
        use crate::coreaudio::hal::CoreAudioHal;
        use crate::coreaudio::traits::AudioHal;
        use crate::transport::TransportKind;

        let Some(original) = default_output_uid().unwrap() else {
            return;
        };
        let hal = CoreAudioHal::new().unwrap();
        let before = hal.list_outputs().unwrap();
        let Some(builtin_uid) = before
            .devices
            .iter()
            .find(|d| d.transport == TransportKind::BuiltIn)
            .map(|d| d.uid.clone())
        else {
            eprintln!("skip: no built-in device on this machine");
            return;
        };

        set_default_output(&builtin_uid, true).unwrap();
        let list = hal.list_outputs().unwrap();
        let builtin = list
            .devices
            .iter()
            .find(|d| d.uid == builtin_uid)
            .expect("built-in in list");
        eprintln!(
            "builtin is_default={} is_active={}",
            builtin.is_default, builtin.is_active
        );
        assert!(builtin.is_default, "built-in should be HAL default");
        assert!(builtin.is_active, "built-in should be active");
        assert!(
            list.system_default.is_none(),
            "no virtual footer when physical default"
        );

        set_default_output(&original, true).unwrap();
    }

    #[test]
    #[ignore = "mutates system audio default"]
    fn test_set_default_round_trip() {
        let Some(original) = default_output_uid().unwrap() else {
            return;
        };
        let ids = all_device_ids().unwrap();
        let Some(other_id) = ids
            .into_iter()
            .find(|id| device_uid(*id).ok().as_deref() != Some(original.as_str()))
        else {
            return;
        };
        let other_uid = device_uid(other_id).unwrap();
        set_default_output(&other_uid, false).unwrap();
        assert_eq!(
            default_output_uid().unwrap().as_deref(),
            Some(other_uid.as_str())
        );
        set_default_output(&original, false).unwrap();
    }
}
