//! Optional CoreGraphics event-tap activity monitor (macOS).
#![allow(unsafe_code)]

use crate::activity::ActivityMonitor;
use crate::config::Config;
use crate::RustyJackError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Build the daemon activity monitor selected by config.
#[must_use]
pub fn daemon_activity_monitor(config: &Config) -> Box<dyn ActivityMonitor> {
    #[cfg(target_os = "macos")]
    {
        if config.activity_monitor.eq_ignore_ascii_case("event_tap") {
            if let Ok(monitor) = EventTapActivityMonitor::try_new() {
                return Box::new(monitor);
            }
            tracing::warn!(
                target: "daemon",
                "[activity] event tap unavailable (Accessibility permission may be required); falling back to idle monitor"
            );
        }
    }
    Box::new(crate::activity::PlatformActivityMonitor)
}

#[cfg(target_os = "macos")]
struct EventTapActivityMonitor {
    last_event_unix_nanos: Arc<AtomicU64>,
    _thread: JoinHandle<()>,
}

#[cfg(target_os = "macos")]
impl EventTapActivityMonitor {
    fn try_new() -> Result<Self, RustyJackError> {
        let last_event_unix_nanos = Arc::new(AtomicU64::new(now_unix_nanos()));
        let callback_state = Arc::clone(&last_event_unix_nanos);
        let thread = thread::Builder::new()
            .name("rusty-jack-event-tap".into())
            .spawn(move || event_tap_thread(callback_state))
            .map_err(|err| RustyJackError::AppLaunch(format!("event tap thread failed: {err}")))?;

        Ok(Self {
            last_event_unix_nanos,
            _thread: thread,
        })
    }
}

#[cfg(target_os = "macos")]
impl ActivityMonitor for EventTapActivityMonitor {
    fn idle_duration(&self) -> Result<Duration, RustyJackError> {
        let last = self.last_event_unix_nanos.load(Ordering::Relaxed);
        let now = now_unix_nanos();
        let idle_nanos = now.saturating_sub(last);
        Ok(Duration::from_nanos(idle_nanos))
    }
}

#[cfg(target_os = "macos")]
fn now_unix_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(target_os = "macos")]
fn event_tap_thread(last_event_unix_nanos: Arc<AtomicU64>) {
    if event_tap_run_loop(&last_event_unix_nanos).is_err() {
        tracing::warn!(target: "daemon", "[activity] event tap thread exited");
    }
}

#[cfg(target_os = "macos")]
fn event_tap_run_loop(last_event_unix_nanos: &Arc<AtomicU64>) -> Result<(), RustyJackError> {
    use core_foundation::base::{TCFType, TCFTypeRef};
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
    use std::ffi::c_void;
    use std::ptr;
    use std::time::Duration;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void,
            user_info: *mut c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: *mut c_void,
            order: i32,
        ) -> *mut c_void;
    }

    const K_CG_SESSION_EVENT_TAP: u32 = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
    const K_CG_EVENT_MASK_FOR_ALL_EVENTS: u64 = 0xFFFF_FFFF_FFFF_FFFF;

    extern "C" fn event_callback(
        user_info: *mut c_void,
        _event_type: u32,
        _event: *mut c_void,
        _user_data: *mut c_void,
    ) -> *mut c_void {
        // SAFETY: `user_info` is the `Arc` pointer installed by this module.
        if !user_info.is_null() {
            let state = unsafe { &*(user_info as *const AtomicU64) };
            state.store(now_unix_nanos(), Ordering::Relaxed);
        }
        _event
    }

    let state_ptr = Arc::as_ptr(last_event_unix_nanos) as *mut c_void;
    // SAFETY: CoreGraphics retains the tap for the run loop lifetime of this thread.
    let tap = unsafe {
        CGEventTapCreate(
            K_CG_SESSION_EVENT_TAP,
            K_CG_HEAD_INSERT_EVENT_TAP,
            K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
            K_CG_EVENT_MASK_FOR_ALL_EVENTS,
            event_callback,
            state_ptr,
        )
    };
    if tap.is_null() {
        return Err(RustyJackError::AppLaunch(
            "CGEventTapCreate returned null (grant Accessibility permission to rusty-jack)".into(),
        ));
    }

    // SAFETY: tap is non-null and owned for this thread.
    unsafe { CGEventTapEnable(tap, true) };

    let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
    if source.is_null() {
        return Err(RustyJackError::AppLaunch(
            "CFMachPortCreateRunLoopSource failed for event tap".into(),
        ));
    }

    let run_loop = CFRunLoop::get_current();
    // SAFETY: source is a valid run loop source for this thread.
    unsafe {
        extern "C" {
            fn CFRunLoopAddSource(
                rl: *mut c_void,
                source: *mut c_void,
                mode: *const c_void,
            );
        }
        CFRunLoopAddSource(
            run_loop.as_concrete_TypeRef() as *mut c_void,
            source,
            kCFRunLoopDefaultMode.as_void_ptr(),
        );
        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_secs(86_400), false);
    }

    Ok(())
}
