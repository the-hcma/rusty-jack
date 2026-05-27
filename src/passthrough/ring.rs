//! Shared-memory ring between the HAL driver and daemon passthrough engine.

#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

use crate::passthrough::{
    PASSTHROUGH_CHANNEL_COUNT, PASSTHROUGH_FRAMES_PER_CHUNK, PASSTHROUGH_SAMPLE_RATE_HZ,
};
use crate::RustyJackError;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub const PASSTHROUGH_RING_MAGIC: u32 = 0x5450_4A52;
pub const PASSTHROUGH_RING_VERSION: u32 = 1;
pub const PASSTHROUGH_RING_SLOT_COUNT: usize = 16;
pub const PASSTHROUGH_RING_SAMPLES: usize =
    PASSTHROUGH_FRAMES_PER_CHUNK * PASSTHROUGH_CHANNEL_COUNT;

#[repr(C)]
struct PassthroughHeader {
    write_index: u64,
    read_index: u64,
    magic: u32,
    version: u32,
    volume_scalar: f32,
    muted: u32,
    sample_rate_hz: u32,
    frame_size: u32,
    channel_count: u32,
    reserved: u32,
}

#[repr(C)]
struct PassthroughSlot {
    seq: u64,
    frame_count: u32,
    reserved: u32,
    samples: [f32; PASSTHROUGH_RING_SAMPLES],
}

#[repr(C)]
struct PassthroughRingLayout {
    header: PassthroughHeader,
    slots: [PassthroughSlot; PASSTHROUGH_RING_SLOT_COUNT],
}

/// Memory-mapped passthrough ring file shared with the HAL driver.
pub struct PassthroughRing {
    mapped: *mut PassthroughRingLayout,
    _path: PathBuf,
}

impl PassthroughRing {
    /// Open (or create) the shared ring file under Application Support.
    ///
    /// # Errors
    ///
    /// Returns an error when the home directory or mmap setup fails.
    pub fn open() -> Result<Self, RustyJackError> {
        let path = ring_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
        }
        use std::os::fd::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(RustyJackError::Io)?;
        let layout_size = std::mem::size_of::<PassthroughRingLayout>();
        file.set_len(layout_size as u64)
            .map_err(RustyJackError::Io)?;

        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                layout_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if mapped == libc::MAP_FAILED {
            return Err(RustyJackError::Io(std::io::Error::last_os_error()));
        }

        let ring = mapped as *mut PassthroughRingLayout;
        let magic = unsafe { (*ring).header.magic };
        if magic != PASSTHROUGH_RING_MAGIC {
            unsafe {
                std::ptr::write_bytes(ring, 0, 1);
                (*ring).header.magic = PASSTHROUGH_RING_MAGIC;
                (*ring).header.version = PASSTHROUGH_RING_VERSION;
                (*ring).header.sample_rate_hz = PASSTHROUGH_SAMPLE_RATE_HZ;
                (*ring).header.frame_size = PASSTHROUGH_FRAMES_PER_CHUNK as u32;
                (*ring).header.channel_count = PASSTHROUGH_CHANNEL_COUNT as u32;
                (*ring).header.volume_scalar = 1.0;
            }
        }

        Ok(Self {
            mapped: ring,
            _path: path,
        })
    }

    /// Latest captured frame written by the driver, if any.
    #[must_use]
    pub fn latest_frame(&self) -> Option<(u64, usize, [f32; PASSTHROUGH_RING_SAMPLES])> {
        let write_index = self.write_index().load(Ordering::Acquire);
        if write_index == 0 {
            return None;
        }
        let slot_index = ((write_index - 1) % PASSTHROUGH_RING_SLOT_COUNT as u64) as usize;
        let slot = unsafe { &(*self.mapped).slots[slot_index] };
        let seq = self.slot_seq(slot_index).load(Ordering::Acquire);
        if seq != write_index {
            return None;
        }
        let frame_count = slot.frame_count as usize;
        if frame_count == 0 || frame_count > PASSTHROUGH_FRAMES_PER_CHUNK {
            return None;
        }
        Some((write_index, frame_count, slot.samples))
    }

    pub(crate) fn mark_consumed(&self, write_index: u64) {
        let current = self.read_index().load(Ordering::Relaxed);
        if write_index > current {
            self.read_index().store(write_index, Ordering::Release);
        }
    }

    fn write_index(&self) -> &AtomicU64 {
        unsafe { AtomicU64::from_ptr(std::ptr::addr_of_mut!((*self.mapped).header.write_index)) }
    }

    fn read_index(&self) -> &AtomicU64 {
        unsafe { AtomicU64::from_ptr(std::ptr::addr_of_mut!((*self.mapped).header.read_index)) }
    }

    fn slot_seq(&self, index: usize) -> &AtomicU64 {
        unsafe { AtomicU64::from_ptr(std::ptr::addr_of_mut!((*self.mapped).slots[index].seq)) }
    }
}

impl Drop for PassthroughRing {
    fn drop(&mut self) {
        let layout_size = std::mem::size_of::<PassthroughRingLayout>();
        unsafe {
            libc::munmap(self.mapped as *mut libc::c_void, layout_size);
        }
    }
}

unsafe impl Send for PassthroughRing {}
unsafe impl Sync for PassthroughRing {}

fn ring_path() -> Result<PathBuf, RustyJackError> {
    Ok(PathBuf::from(super::PASSTHROUGH_RING_PATH))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn test_ring_layout_sizes_match_driver_constants() {
        assert_eq!(PASSTHROUGH_RING_SAMPLES, 1024);
        assert!(std::mem::size_of::<PassthroughRingLayout>() > 64_000);
    }
}
