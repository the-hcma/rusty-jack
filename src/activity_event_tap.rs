//! Optional CoreGraphics event-tap activity monitor (macOS).
#![allow(unsafe_code)]

use crate::activity::ActivityMonitor;
use crate::config::Config;
use crate::RustyJackError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Build the daemon activity monitor selected by config.
#[must_use]
pub fn daemon_activity_monitor(config: &Config) -> Box<dyn ActivityMonitor> {
    #[cfg(target_os = "macos")]
    {
        if config.activity_monitor.eq_ignore_ascii_case("event_tap") {
            match EventTapActivityMonitor::try_new(config.activity_event_tap_include_mouse_move) {
                Ok(monitor) => {
                    tracing::info!(
                        target: "daemon",
                        "[activity] event tap active (keyboard/mouse events only; include_mouse_move={})",
                        config.activity_event_tap_include_mouse_move
                    );
                    return Box::new(monitor);
                }
                Err(err) => {
                    tracing::warn!(
                        target: "daemon",
                        "[activity] event tap unavailable ({err}); falling back to idle monitor"
                    );
                }
            }
        }
    }
    Box::new(crate::activity::PlatformActivityMonitor)
}

/// `CGEventMask` bit for one `CGEventType` value.
#[must_use]
pub const fn cg_event_mask_bit(event_type: u32) -> u64 {
    1_u64 << event_type
}

/// CoreGraphics event types observed for keyboard and pointer activity.
pub const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
pub const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
pub const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
pub const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
pub const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
pub const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
pub const K_CG_EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
pub const K_CG_EVENT_KEY_DOWN: u32 = 10;
pub const K_CG_EVENT_KEY_UP: u32 = 11;
pub const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;
pub const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;
pub const K_CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
pub const K_CG_EVENT_OTHER_MOUSE_UP: u32 = 26;
pub const K_CG_EVENT_OTHER_MOUSE_DRAGGED: u32 = 27;

/// Event mask for keyboard and pointer activity used by the event tap.
#[must_use]
pub fn keyboard_mouse_event_mask(include_mouse_move: bool) -> u64 {
    let keyboard = cg_event_mask_bit(K_CG_EVENT_KEY_DOWN)
        | cg_event_mask_bit(K_CG_EVENT_KEY_UP)
        | cg_event_mask_bit(K_CG_EVENT_FLAGS_CHANGED);

    let mut mouse = cg_event_mask_bit(K_CG_EVENT_LEFT_MOUSE_DOWN)
        | cg_event_mask_bit(K_CG_EVENT_LEFT_MOUSE_UP)
        | cg_event_mask_bit(K_CG_EVENT_RIGHT_MOUSE_DOWN)
        | cg_event_mask_bit(K_CG_EVENT_RIGHT_MOUSE_UP)
        | cg_event_mask_bit(K_CG_EVENT_LEFT_MOUSE_DRAGGED)
        | cg_event_mask_bit(K_CG_EVENT_RIGHT_MOUSE_DRAGGED)
        | cg_event_mask_bit(K_CG_EVENT_SCROLL_WHEEL)
        | cg_event_mask_bit(K_CG_EVENT_OTHER_MOUSE_DOWN)
        | cg_event_mask_bit(K_CG_EVENT_OTHER_MOUSE_UP)
        | cg_event_mask_bit(K_CG_EVENT_OTHER_MOUSE_DRAGGED);

    if include_mouse_move {
        mouse |= cg_event_mask_bit(K_CG_EVENT_MOUSE_MOVED);
    }

    keyboard | mouse
}

/// Human-readable label for a CoreGraphics `CGEventType` value.
#[must_use]
pub fn cg_event_type_label(event_type: u32) -> &'static str {
    match event_type {
        K_CG_EVENT_LEFT_MOUSE_DOWN => "LeftMouseDown",
        K_CG_EVENT_LEFT_MOUSE_UP => "LeftMouseUp",
        K_CG_EVENT_RIGHT_MOUSE_DOWN => "RightMouseDown",
        K_CG_EVENT_RIGHT_MOUSE_UP => "RightMouseUp",
        K_CG_EVENT_MOUSE_MOVED => "MouseMoved",
        K_CG_EVENT_LEFT_MOUSE_DRAGGED => "LeftMouseDragged",
        K_CG_EVENT_RIGHT_MOUSE_DRAGGED => "RightMouseDragged",
        K_CG_EVENT_KEY_DOWN => "KeyDown",
        K_CG_EVENT_KEY_UP => "KeyUp",
        K_CG_EVENT_FLAGS_CHANGED => "FlagsChanged",
        K_CG_EVENT_SCROLL_WHEEL => "ScrollWheel",
        K_CG_EVENT_OTHER_MOUSE_DOWN => "OtherMouseDown",
        K_CG_EVENT_OTHER_MOUSE_UP => "OtherMouseUp",
        K_CG_EVENT_OTHER_MOUSE_DRAGGED => "OtherMouseDragged",
        _ => "Unknown",
    }
}

#[cfg(target_os = "macos")]
struct EventTapActivityMonitor {
    last_event_unix_nanos: Arc<AtomicU64>,
    last_event_label: Arc<Mutex<String>>,
    _thread: JoinHandle<()>,
}

#[cfg(target_os = "macos")]
impl EventTapActivityMonitor {
    fn try_new(include_mouse_move: bool) -> Result<Self, RustyJackError> {
        let last_event_unix_nanos = Arc::new(AtomicU64::new(now_unix_nanos()));
        let last_event_label = Arc::new(Mutex::new(String::new()));
        let callback_state = EventTapCallbackState {
            last_event_unix_nanos: Arc::clone(&last_event_unix_nanos),
            last_event_label: Arc::clone(&last_event_label),
        };
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("rusty-jack-event-tap".into())
            .spawn(move || event_tap_thread(callback_state, include_mouse_move, ready_tx))
            .map_err(|err| RustyJackError::AppLaunch(format!("event tap thread failed: {err}")))?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                last_event_unix_nanos,
                last_event_label,
                _thread: thread,
            }),
            Ok(Err(err)) => Err(err),
            Err(RecvTimeoutError::Timeout) => Err(RustyJackError::AppLaunch(
                "event tap setup timed out".into(),
            )),
            Err(RecvTimeoutError::Disconnected) => Err(RustyJackError::AppLaunch(
                "event tap thread exited before setup completed".into(),
            )),
        }
    }
}

#[cfg(target_os = "macos")]
struct EventTapCallbackState {
    last_event_unix_nanos: Arc<AtomicU64>,
    last_event_label: Arc<Mutex<String>>,
}

#[cfg(target_os = "macos")]
impl ActivityMonitor for EventTapActivityMonitor {
    fn idle_duration(&self) -> Result<Duration, RustyJackError> {
        let last = self.last_event_unix_nanos.load(Ordering::Relaxed);
        let now = now_unix_nanos();
        let idle_nanos = now.saturating_sub(last);
        Ok(Duration::from_nanos(idle_nanos))
    }

    fn last_activity_event(&self) -> Option<String> {
        self.last_event_label
            .lock()
            .ok()
            .filter(|label| !label.is_empty())
            .map(|label| label.clone())
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
fn event_tap_thread(
    state: EventTapCallbackState,
    include_mouse_move: bool,
    ready_tx: SyncSender<Result<(), RustyJackError>>,
) {
    if let Err(err) = event_tap_run_loop(&state, include_mouse_move, ready_tx) {
        tracing::warn!(target: "daemon", "[activity] event tap thread exited: {err}");
    }
}

#[cfg(target_os = "macos")]
fn event_tap_run_loop(
    state: &EventTapCallbackState,
    include_mouse_move: bool,
    ready_tx: SyncSender<Result<(), RustyJackError>>,
) -> Result<(), RustyJackError> {
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

    extern "C" fn event_callback(
        _proxy: *mut c_void,
        event_type: u32,
        event: *mut c_void,
        user_info: *mut c_void,
    ) -> *mut c_void {
        // SAFETY: `user_info` is the `EventTapCallbackState` pointer installed by this module.
        if !user_info.is_null() {
            let state = unsafe { &*(user_info as *const EventTapCallbackState) };
            state
                .last_event_unix_nanos
                .store(now_unix_nanos(), Ordering::Relaxed);
            if let Ok(mut label) = state.last_event_label.lock() {
                *label = cg_event_type_label(event_type).into();
            }
        }
        event
    }

    let state_ptr = state as *const EventTapCallbackState as *mut c_void;
    let event_mask = keyboard_mouse_event_mask(include_mouse_move);
    // SAFETY: CoreGraphics retains the tap for the run loop lifetime of this thread.
    let tap = unsafe {
        CGEventTapCreate(
            K_CG_SESSION_EVENT_TAP,
            K_CG_HEAD_INSERT_EVENT_TAP,
            K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
            event_mask,
            event_callback,
            state_ptr,
        )
    };
    if tap.is_null() {
        let message: String =
            "CGEventTapCreate returned null (grant Accessibility permission to rusty-jack)".into();
        let _ = ready_tx.send(Err(RustyJackError::AppLaunch(message.clone())));
        return Err(RustyJackError::AppLaunch(message));
    }

    // SAFETY: tap is non-null and owned for this thread.
    unsafe { CGEventTapEnable(tap, true) };

    let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
    if source.is_null() {
        let message: String = "CFMachPortCreateRunLoopSource failed for event tap".into();
        let _ = ready_tx.send(Err(RustyJackError::AppLaunch(message.clone())));
        return Err(RustyJackError::AppLaunch(message));
    }

    let _ = ready_tx.send(Ok(()));

    let run_loop = CFRunLoop::get_current();
    // SAFETY: source is a valid run loop source for this thread.
    unsafe {
        extern "C" {
            fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_mouse_event_mask_includes_key_and_click_types() {
        let mask = keyboard_mouse_event_mask(false);
        assert_ne!(mask, 0);
        assert_eq!(
            mask & cg_event_mask_bit(K_CG_EVENT_KEY_DOWN),
            cg_event_mask_bit(K_CG_EVENT_KEY_DOWN)
        );
        assert_eq!(
            mask & cg_event_mask_bit(K_CG_EVENT_LEFT_MOUSE_DOWN),
            cg_event_mask_bit(K_CG_EVENT_LEFT_MOUSE_DOWN)
        );
        assert_eq!(mask & cg_event_mask_bit(K_CG_EVENT_MOUSE_MOVED), 0);
    }

    #[test]
    fn keyboard_mouse_event_mask_can_include_mouse_move() {
        let mask = keyboard_mouse_event_mask(true);
        assert_eq!(
            mask & cg_event_mask_bit(K_CG_EVENT_MOUSE_MOVED),
            cg_event_mask_bit(K_CG_EVENT_MOUSE_MOVED)
        );
    }

    #[test]
    fn keyboard_mouse_event_mask_excludes_tablet_events() {
        let mask = keyboard_mouse_event_mask(true);
        assert_eq!(mask & cg_event_mask_bit(23), 0);
        assert_eq!(mask & cg_event_mask_bit(24), 0);
        assert!(mask < u64::MAX);
    }

    #[test]
    fn cg_event_type_label_maps_known_types() {
        assert_eq!(cg_event_type_label(K_CG_EVENT_KEY_DOWN), "KeyDown");
        assert_eq!(cg_event_type_label(K_CG_EVENT_SCROLL_WHEEL), "ScrollWheel");
    }
}
