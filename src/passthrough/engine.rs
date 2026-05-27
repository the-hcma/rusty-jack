//! CoreAudio IO proc that renders passthrough ring audio to the physical HDMI/DP device.

#![cfg(target_os = "macos")]
#![allow(
    unsafe_code,
    clippy::cast_possible_truncation,
    clippy::ptr_as_ptr,
    clippy::borrow_as_ptr
)]

use crate::coreaudio::device_id_for_uid;
use crate::passthrough::ring::PassthroughRing;
use crate::passthrough::{PassthroughPlan, PASSTHROUGH_CHANNEL_COUNT};
use crate::RustyJackError;
use coreaudio_sys::{
    AudioBufferList, AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceID,
    AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop, AudioObjectID, AudioTimeStamp,
    OSStatus,
};
use std::ffi::c_void;
use std::sync::Arc;

struct EngineInner {
    ring: PassthroughRing,
}

/// Live passthrough renderer for one physical output device.
pub struct PassthroughEngine {
    device_id: AudioDeviceID,
    proc_id: AudioDeviceIOProcID,
    inner: *const EngineInner,
}

impl PassthroughEngine {
    /// Start rendering passthrough audio to the plan's physical output UID.
    ///
    /// # Errors
    ///
    /// Returns an error when the ring cannot be opened, the device is missing, or IO cannot start.
    pub fn start(plan: &PassthroughPlan) -> Result<Self, RustyJackError> {
        let ring = PassthroughRing::open()?;
        let device_id = device_id_for_uid(&plan.physical_uid)?;
        let inner = Arc::new(EngineInner { ring });
        let client_data = Arc::into_raw(inner);
        let mut proc_id: AudioDeviceIOProcID = None;
        let status = unsafe {
            AudioDeviceCreateIOProcID(
                device_id,
                Some(io_proc),
                client_data as *mut c_void,
                &mut proc_id as *mut AudioDeviceIOProcID,
            )
        };
        if status != 0 {
            unsafe {
                Arc::from_raw(client_data);
            }
            return Err(RustyJackError::CoreAudio(format!(
                "AudioDeviceCreateIOProcID status {status}"
            )));
        }
        let start_status = unsafe { AudioDeviceStart(device_id, proc_id) };
        if start_status != 0 {
            unsafe {
                AudioDeviceDestroyIOProcID(device_id, proc_id);
                Arc::from_raw(client_data);
            }
            return Err(RustyJackError::CoreAudio(format!(
                "AudioDeviceStart status {start_status}"
            )));
        }

        eprintln!(
            "passthrough: rendering to {} ({})",
            plan.physical_name, plan.physical_uid
        );

        Ok(Self {
            device_id,
            proc_id,
            inner: client_data,
        })
    }
}

impl Drop for PassthroughEngine {
    fn drop(&mut self) {
        unsafe {
            let _ = AudioDeviceStop(self.device_id, self.proc_id);
            let _ = AudioDeviceDestroyIOProcID(self.device_id, self.proc_id);
            Arc::from_raw(self.inner);
        }
        eprintln!("passthrough: stopped physical render");
    }
}

unsafe extern "C" fn io_proc(
    _device: AudioObjectID,
    _now: *const AudioTimeStamp,
    _input_data: *const AudioBufferList,
    _input_time: *const AudioTimeStamp,
    output_data: *mut AudioBufferList,
    _output_time: *const AudioTimeStamp,
    client_data: *mut c_void,
) -> OSStatus {
    if output_data.is_null() || client_data.is_null() {
        return 0;
    }
    let inner = unsafe { &*(client_data as *const EngineInner) };
    fill_output_buffer(inner, output_data);
    0
}

fn fill_output_buffer(inner: &EngineInner, output_data: *mut AudioBufferList) {
    let buffer_list = unsafe { &mut *output_data };
    if buffer_list.mNumberBuffers == 0 {
        return;
    }
    let buffer = &mut buffer_list.mBuffers[0];
    let byte_capacity = buffer.mDataByteSize as usize;
    if buffer.mData.is_null() || byte_capacity == 0 {
        return;
    }
    let sample_capacity = byte_capacity / std::mem::size_of::<f32>();
    let dest = unsafe { std::slice::from_raw_parts_mut(buffer.mData as *mut f32, sample_capacity) };
    dest.fill(0.0);

    let Some((seq, frame_count, samples)) = inner.ring.latest_frame() else {
        return;
    };
    let sample_count = frame_count * PASSTHROUGH_CHANNEL_COUNT;
    let copy_count = sample_count.min(dest.len()).min(samples.len());
    dest[..copy_count].copy_from_slice(&samples[..copy_count]);
    inner.ring.mark_consumed(seq);
}
