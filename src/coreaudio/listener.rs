//! CoreAudio property listeners for daemon wake-ups (macOS only).
//!
//! These listeners are a best-effort complement to polling. They allow the daemon to react
//! quickly to device list changes and default-output changes without removing the polling
//! safety net (which helps after sleep/wake or missed callbacks).

#![allow(unsafe_code)]

use crate::RustyJackError;
use coreaudio_sys::{
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioObjectAddPropertyListener, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListener,
};
use std::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreAudioEvent {
    DevicesChanged,
    DefaultOutputChanged,
}

pub struct CoreAudioListener {
    rx: Receiver<CoreAudioEvent>,
    sender_ptr: *mut Sender<CoreAudioEvent>,
}

unsafe impl Send for CoreAudioListener {}
unsafe impl Sync for CoreAudioListener {}

fn property_address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    }
}

extern "C" fn listener_proc(
    _object_id: AudioObjectID,
    number_addresses: u32,
    addresses: *const AudioObjectPropertyAddress,
    client_data: *mut core::ffi::c_void,
) -> i32 {
    if client_data.is_null() || addresses.is_null() || number_addresses == 0 {
        return 0;
    }
    // Safety: `client_data` is owned by `CoreAudioListener` and lives until Drop, which removes
    // these listeners before freeing.
    let tx = unsafe { &*(client_data as *const Sender<CoreAudioEvent>) };
    let selector = unsafe { (*addresses).mSelector };
    let event = match selector {
        // These are `coreaudio-sys` C globals (non-upper-case); match by numeric value.
        x if x == kAudioHardwarePropertyDevices => CoreAudioEvent::DevicesChanged,
        x if x == kAudioHardwarePropertyDefaultOutputDevice => CoreAudioEvent::DefaultOutputChanged,
        _ => return 0,
    };
    let _ = tx.send(event);
    0
}

fn add_listener(
    selector: u32,
    sender_ptr: *mut Sender<CoreAudioEvent>,
) -> Result<(), RustyJackError> {
    let address = property_address(selector);
    let status = unsafe {
        AudioObjectAddPropertyListener(
            kAudioObjectSystemObject,
            &address,
            Some(listener_proc),
            sender_ptr.cast(),
        )
    };
    if status != 0 {
        return Err(RustyJackError::CoreAudio(format!(
            "AudioObjectAddPropertyListener({selector}) status {status}"
        )));
    }
    Ok(())
}

fn remove_listener(selector: u32, sender_ptr: *mut Sender<CoreAudioEvent>) {
    let address = property_address(selector);
    unsafe {
        let _ = AudioObjectRemovePropertyListener(
            kAudioObjectSystemObject,
            &address,
            Some(listener_proc),
            sender_ptr.cast(),
        );
    }
}

impl CoreAudioListener {
    pub fn new() -> Result<Self, RustyJackError> {
        let (tx, rx) = std::sync::mpsc::channel::<CoreAudioEvent>();
        let sender_ptr = Box::into_raw(Box::new(tx));

        if let Err(err) = add_listener(kAudioHardwarePropertyDevices, sender_ptr) {
            unsafe { drop(Box::from_raw(sender_ptr)) };
            return Err(err);
        }
        if let Err(err) = add_listener(kAudioHardwarePropertyDefaultOutputDevice, sender_ptr) {
            remove_listener(kAudioHardwarePropertyDevices, sender_ptr);
            unsafe { drop(Box::from_raw(sender_ptr)) };
            return Err(err);
        }

        Ok(Self { rx, sender_ptr })
    }

    #[must_use]
    pub fn receiver(&self) -> &Receiver<CoreAudioEvent> {
        &self.rx
    }
}

impl Drop for CoreAudioListener {
    fn drop(&mut self) {
        remove_listener(kAudioHardwarePropertyDefaultOutputDevice, self.sender_ptr);
        remove_listener(kAudioHardwarePropertyDevices, self.sender_ptr);
        unsafe {
            drop(Box::from_raw(self.sender_ptr));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_address_fields() {
        let a = property_address(kAudioHardwarePropertyDevices);
        assert_eq!(a.mSelector, kAudioHardwarePropertyDevices);
        assert_eq!(a.mScope, kAudioObjectPropertyScopeGlobal);
        assert_eq!(a.mElement, kAudioObjectPropertyElementMaster);
    }
}
