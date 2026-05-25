//! Output volume read/write via CoreAudio (macOS).

#![allow(
    unsafe_code,
    clippy::cast_possible_truncation,
    clippy::ptr_as_ptr,
    clippy::borrow_as_ptr
)]

use crate::coreaudio::default_output::device_id_for_uid;
use crate::coreaudio::property::{default_output_device_id, device_uid};
use crate::volume_result::{volume_within_tolerance, VolumeEnsureOptions, VolumeEnsureResult};
use crate::RustyJackError;
use coreaudio_sys::{
    kAudioDevicePropertyScopeOutput, kAudioDevicePropertyVolumeScalar,
    kAudioObjectPropertyElementMaster, AudioDeviceID, AudioObjectGetPropertyData,
    AudioObjectPropertyAddress, AudioObjectSetPropertyData,
};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::thread;

fn ok(status: i32) -> bool {
    status == 0
}

fn volume_address() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyVolumeScalar,
        mScope: kAudioDevicePropertyScopeOutput,
        mElement: kAudioObjectPropertyElementMaster,
    }
}

fn scalar_to_percent(scalar: f32) -> u8 {
    (scalar.clamp(0.0, 1.0) * 100.0).round() as u8
}

fn percent_to_scalar(percent: u8) -> f32 {
    (percent.min(100) as f32 / 100.0).clamp(0.0, 1.0)
}

/// Read output volume for `device_id` as 0–100 when the device exposes a volume scalar.
pub fn output_volume_percent(device_id: AudioDeviceID) -> Option<u8> {
    let address = volume_address();
    let mut scalar: f32 = 0.0;
    let mut size = std::mem::size_of::<f32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            null_mut(),
            &mut size,
            &mut scalar as *mut f32 as *mut c_void,
        )
    };
    if ok(status) {
        Some(scalar_to_percent(scalar))
    } else {
        None
    }
}

/// Set output volume for `device_id` (0–100). Returns an error when the property is not settable.
pub fn set_output_volume_percent(
    device_id: AudioDeviceID,
    percent: u8,
) -> Result<(), RustyJackError> {
    let address = volume_address();
    let scalar = percent_to_scalar(percent);
    let status = unsafe {
        AudioObjectSetPropertyData(
            device_id,
            &address,
            0,
            null_mut(),
            std::mem::size_of::<f32>() as u32,
            &scalar as *const f32 as *const c_void,
        )
    };
    if !ok(status) {
        return Err(RustyJackError::CoreAudio(format!(
            "set volume scalar status {status}"
        )));
    }
    Ok(())
}

/// Read output volume for a device UID, when supported.
pub fn output_volume_percent_for_uid(uid: &str) -> Result<Option<u8>, RustyJackError> {
    let device_id = device_id_for_uid(uid)?;
    Ok(output_volume_percent(device_id))
}

/// Set output volume for a device UID (0–100).
pub fn set_output_volume_for_uid(uid: &str, percent: u8) -> Result<(), RustyJackError> {
    let device_id = device_id_for_uid(uid)?;
    set_output_volume_percent(device_id, percent)
}

/// True when the HAL default output is a virtual router such as eqMac.
#[must_use]
pub fn default_output_is_virtual_router() -> bool {
    let Some(id) = default_output_device_id() else {
        return false;
    };
    let Ok(uid) = device_uid(id) else {
        return false;
    };
    uid.contains("EQM") || uid.to_ascii_lowercase().contains("eqmac")
}

/// System output volume via AppleScript (0–100).
pub fn system_output_volume_percent() -> Option<u8> {
    let script = "output volume of (get volume settings)";
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse().ok()
}

/// Set system output volume via AppleScript.
pub fn set_system_output_volume_percent(percent: u8) -> Result<(), RustyJackError> {
    let percent = percent.min(100);
    let script = format!("set volume output volume {percent}");
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|err| RustyJackError::CoreAudio(format!("osascript volume: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RustyJackError::CoreAudio(format!(
            "osascript set volume failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

/// Best-effort read of audible output volume (system level when a virtual router is default).
#[must_use]
pub fn read_effective_output_volume(uid: &str) -> Option<u8> {
    if default_output_is_virtual_router() {
        return system_output_volume_percent();
    }
    output_volume_percent_for_uid(uid)
        .ok()
        .flatten()
        .or_else(system_output_volume_percent)
}

/// Write volume through every path we control (device scalar + system output).
pub fn write_output_volume(uid: &str, percent: u8) {
    let _ = set_system_output_volume_percent(percent);
    let _ = set_output_volume_for_uid(uid, percent);
}

/// Set volume and poll until readback matches (handles eqMac resetting level after route changes).
pub fn ensure_output_volume(uid: &str, percent: u8) -> Result<VolumeEnsureResult, RustyJackError> {
    ensure_output_volume_with_options(uid, percent, &VolumeEnsureOptions::default())
}

pub fn ensure_output_volume_with_options(
    uid: &str,
    percent: u8,
    options: &VolumeEnsureOptions,
) -> Result<VolumeEnsureResult, RustyJackError> {
    let percent = percent.min(100);
    if !options.initial_delay.is_zero() {
        thread::sleep(options.initial_delay);
    }

    let mut last_reading = None;
    for attempt in 1..=options.max_attempts {
        write_output_volume(uid, percent);
        if !options.retry_delay.is_zero() {
            thread::sleep(options.retry_delay);
        }

        last_reading = read_effective_output_volume(uid);
        if last_reading
            .is_some_and(|actual| volume_within_tolerance(actual, percent, options.tolerance))
        {
            return Ok(VolumeEnsureResult {
                target: percent,
                actual: last_reading,
                verified: true,
                attempts: attempt,
            });
        }
    }

    Ok(VolumeEnsureResult {
        target: percent,
        actual: last_reading,
        verified: false,
        attempts: options.max_attempts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_round_trip() {
        assert_eq!(scalar_to_percent(0.0), 0);
        assert_eq!(scalar_to_percent(0.125), 13);
        assert_eq!(scalar_to_percent(1.0), 100);
        assert!((percent_to_scalar(13) - 0.13).abs() < f32::EPSILON);
    }

    #[test]
    fn test_volume_within_tolerance() {
        use crate::volume_result::volume_within_tolerance;

        assert!(volume_within_tolerance(13, 13, 0));
        assert!(volume_within_tolerance(12, 13, 1));
        assert!(volume_within_tolerance(14, 13, 1));
        assert!(!volume_within_tolerance(10, 13, 1));
    }

    #[test]
    #[ignore = "requires live CoreAudio hardware"]
    fn test_probe_current_volume() {
        eprintln!("current volume: {:?}", read_effective_output_volume("test"));
    }
}
