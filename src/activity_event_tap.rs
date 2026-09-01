//! Optional CoreGraphics event-tap activity monitor (macOS).
#![allow(unsafe_code)]

use crate::activity::ActivityMonitor;
use crate::config::Config;
use crate::RustyJackError;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// What the listen-only event tap records (for logs and permission hints).
pub const EVENT_TAP_PRIVACY_NOTE: &str =
    "listen-only tap records event timing and type labels (e.g. KeyDown); it does not log or record keystrokes";

/// Shown when `CGEventTapCreate` fails or the tap is disabled (Accessibility permission).
pub const EVENT_TAP_PERMISSION_HINT: &str =
    "grant Accessibility permission to rusty-jack (listen-only: timing and event-type labels only, no keystroke logging); restart the daemon after granting permission";

/// Whether the daemon should attempt an event-tap activity monitor for this config.
#[must_use]
pub fn should_try_event_tap_activity_monitor(config: &Config, accessibility_trusted: bool) -> bool {
    config.activity_monitor.eq_ignore_ascii_case("event_tap") && accessibility_trusted
}

/// Action to take when the event tap looks silent while the session is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilentTapIdleAction {
    RecreateTap,
    UsePlatformIdleUntrustedAccessibility,
    UsePlatformIdleWithMouseMove,
}

/// Decide how to handle a silent event tap (pure logic for tests and `idle_duration`).
#[must_use]
pub fn silent_tap_idle_action(
    include_mouse_move: bool,
    accessibility_trusted: bool,
) -> SilentTapIdleAction {
    if include_mouse_move {
        SilentTapIdleAction::UsePlatformIdleWithMouseMove
    } else if !accessibility_trusted {
        SilentTapIdleAction::UsePlatformIdleUntrustedAccessibility
    } else {
        SilentTapIdleAction::RecreateTap
    }
}

/// Build the daemon activity monitor selected by config.
#[must_use]
pub fn daemon_activity_monitor(config: &Config) -> Box<dyn ActivityMonitor> {
    #[cfg(target_os = "macos")]
    {
        if config.activity_monitor.eq_ignore_ascii_case("event_tap") {
            if !should_try_event_tap_activity_monitor(
                config,
                crate::privacy_permissions::accessibility_is_trusted(),
            ) {
                tracing::warn!(
                    target: "daemon",
                    "[activity] Accessibility not granted for daemon; using idle monitor instead of event tap ({EVENT_TAP_PERMISSION_HINT})"
                );
                return Box::new(crate::activity::PlatformActivityMonitor);
            }
            match EventTapActivityMonitor::try_new(config.activity_event_tap_include_mouse_move) {
                Ok(monitor) => {
                    tracing::info!(
                        target: "daemon",
                        "[activity] event tap active ({EVENT_TAP_PRIVACY_NOTE}; include_mouse_move={})",
                        config.activity_event_tap_include_mouse_move
                    );
                    return Box::new(monitor);
                }
                Err(err) => {
                    tracing::warn!(
                        target: "daemon",
                        "[activity] event tap unavailable ({err}); falling back to idle monitor ({EVENT_TAP_PRIVACY_NOTE})"
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

/// macOS notifies the tap callback when CoreGraphics disables it.
pub const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
pub const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

/// Return true for CoreGraphics tap-disabled notifications.
#[must_use]
pub const fn is_event_tap_disabled_notification(event_type: u32) -> bool {
    event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
        || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
}

/// Minimum time between event-tap recreations when the tap appears silent.
pub const TAP_RECREATE_COOLDOWN: Duration = Duration::from_secs(600);

const TAP_RECREATE_FAILURE_BACKOFF: Duration = Duration::from_secs(30);

/// Minimum interval between `AXIsProcessTrusted()` probes on the silent-tap path.
pub const ACCESSIBILITY_TRUST_PROBE_COOLDOWN: Duration = Duration::from_secs(60);

/// Whether a cached Accessibility-trust value may be reused without re-probing.
#[must_use]
pub fn accessibility_trust_probe_cache_hit(
    now_unix_nanos: u64,
    last_probe_unix_nanos: u64,
    probe_cooldown: Duration,
) -> bool {
    now_unix_nanos.saturating_sub(last_probe_unix_nanos) < probe_cooldown.as_nanos() as u64
}

/// Resolve cached vs freshly probed Accessibility trust (pure logic for tests and caching).
#[must_use]
pub fn resolve_accessibility_trust_probe(
    now_unix_nanos: u64,
    last_probe_unix_nanos: u64,
    cached_trusted: bool,
    probe_cooldown: Duration,
    probed_trusted: bool,
) -> (bool, u64) {
    if accessibility_trust_probe_cache_hit(now_unix_nanos, last_probe_unix_nanos, probe_cooldown) {
        return (cached_trusted, last_probe_unix_nanos);
    }
    (probed_trusted, now_unix_nanos)
}

/// When mouse-move is excluded, the same signature can mean either a healthy tap ignoring pointer
/// jitter or a deaf tap; callers fall back to platform idle when Accessibility is missing, or
/// recreate the tap when it is granted.
#[must_use]
pub fn event_tap_appears_silent(tap_idle: Duration, platform_idle: Duration) -> bool {
    const SILENT_TAP_IDLE_FLOOR: Duration = Duration::from_secs(90);
    const PLATFORM_IDLE_CEILING: Duration = Duration::from_secs(30);
    const MIN_IDLE_GAP: Duration = Duration::from_secs(30);

    tap_idle >= SILENT_TAP_IDLE_FLOOR
        && platform_idle <= PLATFORM_IDLE_CEILING
        && tap_idle > platform_idle.saturating_add(MIN_IDLE_GAP)
}

/// Return true when a silent tap should be recreated (cooldown elapsed).
#[must_use]
pub fn should_request_tap_recreate(
    tap_idle: Duration,
    platform_idle: Duration,
    since_last_recreate: Duration,
) -> bool {
    event_tap_appears_silent(tap_idle, platform_idle)
        && since_last_recreate >= TAP_RECREATE_COOLDOWN
}

/// Poll `flag` until it becomes true or `attempts` is exhausted.
#[must_use]
pub fn poll_atomic_flag_while(
    flag: &AtomicBool,
    attempts: usize,
    mut between_attempts: impl FnMut(),
) -> bool {
    for _ in 0..attempts.max(1) {
        if flag.load(Ordering::Acquire) {
            return true;
        }
        between_attempts();
    }
    false
}

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
    use_platform_idle: Arc<AtomicBool>,
    logged_platform_fallback: Arc<AtomicBool>,
    recreate_requested: Arc<AtomicBool>,
    last_recreate_unix_nanos: Arc<AtomicU64>,
    accessibility_trusted_cache: Arc<AtomicBool>,
    accessibility_trust_probed_at_unix_nanos: Arc<AtomicU64>,
    include_mouse_move: bool,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl EventTapActivityMonitor {
    fn try_new(include_mouse_move: bool) -> Result<Self, RustyJackError> {
        let last_event_unix_nanos = Arc::new(AtomicU64::new(now_unix_nanos()));
        let last_event_label = Arc::new(Mutex::new(String::new()));
        let use_platform_idle = Arc::new(AtomicBool::new(false));
        let logged_platform_fallback = Arc::new(AtomicBool::new(false));
        let recreate_requested = Arc::new(AtomicBool::new(false));
        let last_recreate_unix_nanos = Arc::new(AtomicU64::new(0));
        let accessibility_trusted_cache = Arc::new(AtomicBool::new(true));
        let accessibility_trust_probed_at_unix_nanos = Arc::new(AtomicU64::new(now_unix_nanos()));
        let fallback_on_disable = Arc::new(AtomicBool::new(include_mouse_move));
        let startup_failed = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let callback_state = EventTapCallbackState {
            last_event_unix_nanos: Arc::clone(&last_event_unix_nanos),
            last_event_label: Arc::clone(&last_event_label),
            tap_port: AtomicUsize::new(0),
            use_platform_idle: Arc::clone(&use_platform_idle),
            logged_disabled: Arc::new(AtomicBool::new(false)),
            recreate_requested: Arc::clone(&recreate_requested),
            last_recreate_unix_nanos: Arc::clone(&last_recreate_unix_nanos),
            fallback_on_disable: Arc::clone(&fallback_on_disable),
            startup_failed: Arc::clone(&startup_failed),
            shutdown: Arc::clone(&shutdown),
        };
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("rusty-jack-event-tap".into())
            .spawn(move || event_tap_thread(callback_state, include_mouse_move, ready_tx))
            .map_err(|err| RustyJackError::AppLaunch(format!("event tap thread failed: {err}")))?;

        let mut monitor = Self {
            last_event_unix_nanos,
            last_event_label,
            use_platform_idle,
            logged_platform_fallback,
            recreate_requested,
            last_recreate_unix_nanos,
            accessibility_trusted_cache,
            accessibility_trust_probed_at_unix_nanos,
            include_mouse_move,
            shutdown,
            thread: Some(thread),
        };

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                if wait_for_event_tap_disabled(&startup_failed, Duration::from_millis(250)) {
                    monitor.stop_event_tap_thread();
                    return Err(RustyJackError::AppLaunch(EVENT_TAP_PERMISSION_HINT.into()));
                }
                Ok(monitor)
            }
            Ok(Err(err)) => {
                monitor.stop_event_tap_thread();
                Err(err)
            }
            Err(RecvTimeoutError::Timeout) => {
                monitor.stop_event_tap_thread();
                Err(RustyJackError::AppLaunch(
                    "event tap setup timed out".into(),
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                monitor.stop_event_tap_thread();
                Err(RustyJackError::AppLaunch(
                    "event tap thread exited before setup completed".into(),
                ))
            }
        }
    }

    fn stop_event_tap_thread(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn log_platform_fallback(&self, reason: &str) {
        if self
            .logged_platform_fallback
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            tracing::warn!(
                target: "daemon",
                "[activity] event tap using idle monitor fallback ({reason}; {EVENT_TAP_PRIVACY_NOTE})"
            );
        }
    }

    fn request_tap_recreate_if_allowed(&self, tap_idle: Duration, platform_idle: Duration) {
        let now = now_unix_nanos();
        let last = self.last_recreate_unix_nanos.load(Ordering::Acquire);
        let since_last_recreate = Duration::from_nanos(now.saturating_sub(last));
        if !should_request_tap_recreate(tap_idle, platform_idle, since_last_recreate) {
            return;
        }
        if !arm_tap_recreate(&self.last_recreate_unix_nanos) {
            return;
        }
        self.recreate_requested.store(true, Ordering::Release);
        tracing::warn!(
            target: "daemon",
            "[activity] event tap appears silent (tap_idle={:.1}s platform_idle={:.1}s); requesting tap recreate ({EVENT_TAP_PRIVACY_NOTE})",
            tap_idle.as_secs_f64(),
            platform_idle.as_secs_f64(),
        );
    }

    fn cached_accessibility_trusted(&self) -> bool {
        let now = now_unix_nanos();
        let last_probe = self
            .accessibility_trust_probed_at_unix_nanos
            .load(Ordering::Acquire);
        let cached = self.accessibility_trusted_cache.load(Ordering::Acquire);
        if accessibility_trust_probe_cache_hit(now, last_probe, ACCESSIBILITY_TRUST_PROBE_COOLDOWN)
        {
            return cached;
        }

        let trusted = crate::privacy_permissions::accessibility_is_trusted();
        self.accessibility_trusted_cache
            .store(trusted, Ordering::Release);
        self.accessibility_trust_probed_at_unix_nanos
            .store(now, Ordering::Release);
        trusted
    }

    fn latch_platform_idle_fallback(&self, reason: &str, platform_idle: Duration) -> Duration {
        self.use_platform_idle.store(true, Ordering::Release);
        self.log_platform_fallback(reason);
        platform_idle
    }
}

#[cfg(target_os = "macos")]
impl Drop for EventTapActivityMonitor {
    fn drop(&mut self) {
        self.stop_event_tap_thread();
    }
}

#[cfg(target_os = "macos")]
fn wait_for_event_tap_disabled(disabled: &AtomicBool, timeout: Duration) -> bool {
    let step = Duration::from_millis(25);
    let attempts = timeout.div_duration_f64(step).ceil() as usize;
    poll_atomic_flag_while(disabled, attempts, || thread::sleep(step))
}

#[cfg(target_os = "macos")]
struct EventTapCallbackState {
    last_event_unix_nanos: Arc<AtomicU64>,
    last_event_label: Arc<Mutex<String>>,
    tap_port: AtomicUsize,
    use_platform_idle: Arc<AtomicBool>,
    logged_disabled: Arc<AtomicBool>,
    recreate_requested: Arc<AtomicBool>,
    last_recreate_unix_nanos: Arc<AtomicU64>,
    fallback_on_disable: Arc<AtomicBool>,
    startup_failed: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

#[cfg(target_os = "macos")]
impl ActivityMonitor for EventTapActivityMonitor {
    fn idle_duration(&self) -> Result<Duration, RustyJackError> {
        if self.use_platform_idle.load(Ordering::Acquire) {
            return crate::activity::macos_platform_idle_duration();
        }

        let last = self.last_event_unix_nanos.load(Ordering::Acquire);
        let now = now_unix_nanos();
        let tap_idle = Duration::from_nanos(now.saturating_sub(last));

        if let Ok(platform_idle) = crate::activity::macos_platform_idle_duration() {
            if event_tap_appears_silent(tap_idle, platform_idle) {
                let accessibility_trusted = if self.include_mouse_move {
                    true
                } else {
                    self.cached_accessibility_trusted()
                };
                match silent_tap_idle_action(self.include_mouse_move, accessibility_trusted) {
                    SilentTapIdleAction::UsePlatformIdleWithMouseMove => {
                        return Ok(self.latch_platform_idle_fallback(
                            "tap stopped receiving events while the session is active (grant Accessibility permission and restart the daemon to restore event-tap mode)",
                            platform_idle,
                        ));
                    }
                    SilentTapIdleAction::UsePlatformIdleUntrustedAccessibility => {
                        return Ok(self.latch_platform_idle_fallback(
                            "tap silent while Accessibility is missing for the daemon (using platform idle until permission is granted and the daemon is restarted)",
                            platform_idle,
                        ));
                    }
                    SilentTapIdleAction::RecreateTap => {
                        self.request_tap_recreate_if_allowed(tap_idle, platform_idle);
                    }
                }
            }
        }

        Ok(tap_idle)
    }

    fn last_activity_event(&self) -> Option<String> {
        if self.use_platform_idle.load(Ordering::Acquire) {
            return None;
        }
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

/// Record a recreate attempt when the cooldown has elapsed.
#[must_use]
fn arm_tap_recreate(last_recreate_unix_nanos: &AtomicU64) -> bool {
    let now = now_unix_nanos();
    let last = last_recreate_unix_nanos.load(Ordering::Acquire);
    let since_last_recreate = Duration::from_nanos(now.saturating_sub(last));
    if since_last_recreate < TAP_RECREATE_COOLDOWN {
        return false;
    }
    last_recreate_unix_nanos
        .compare_exchange(last, now, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

fn request_tap_recreate_from_callback(state: &EventTapCallbackState, reason: &str) {
    if !arm_tap_recreate(&state.last_recreate_unix_nanos) {
        return;
    }
    state.recreate_requested.store(true, Ordering::Release);
    if !reason.is_empty() {
        tracing::warn!(target: "daemon", "[activity] {reason}");
    }
}

/// Wait until shutdown is requested or the recreate backoff elapses.
#[cfg(target_os = "macos")]
fn wait_for_shutdown_or_timeout(shutdown: &AtomicBool, timeout: Duration) {
    let step = Duration::from_millis(100);
    let attempts = timeout.div_duration_f64(step).ceil() as usize;
    let _ = poll_atomic_flag_while(shutdown, attempts, || thread::sleep(step));
}

#[cfg(target_os = "macos")]
fn event_tap_thread(
    state: EventTapCallbackState,
    include_mouse_move: bool,
    ready_tx: SyncSender<Result<(), RustyJackError>>,
) {
    let mut first_setup = true;
    while !state.shutdown.load(Ordering::Acquire) {
        state.recreate_requested.store(false, Ordering::Release);
        state.logged_disabled.store(false, Ordering::Release);
        let setup_tx = if first_setup {
            first_setup = false;
            Some(ready_tx.clone())
        } else {
            tracing::warn!(
                target: "daemon",
                "[activity] event tap attempting recreate after silent stall ({EVENT_TAP_PRIVACY_NOTE}; include_mouse_move={include_mouse_move})"
            );
            None
        };

        let is_recreate_attempt = setup_tx.is_none();
        if let Err(err) = event_tap_run_loop(&state, include_mouse_move, setup_tx) {
            if state.shutdown.load(Ordering::Acquire) {
                break;
            }
            if is_recreate_attempt {
                tracing::warn!(
                    target: "daemon",
                    "[activity] event tap recreate failed: {err}"
                );
                if include_mouse_move {
                    state.use_platform_idle.store(true, Ordering::Release);
                    tracing::warn!(
                        target: "daemon",
                        "[activity] event tap using idle monitor fallback after recreate failure ({EVENT_TAP_PRIVACY_NOTE})"
                    );
                    break;
                }
                wait_for_shutdown_or_timeout(&state.shutdown, TAP_RECREATE_FAILURE_BACKOFF);
                continue;
            }
            if !state.recreate_requested.load(Ordering::Acquire) {
                tracing::warn!(target: "daemon", "[activity] event tap thread exited: {err}");
                break;
            }
        }

        if state.shutdown.load(Ordering::Acquire) {
            break;
        }
        if !state.recreate_requested.load(Ordering::Acquire) {
            break;
        }
    }
}

#[cfg(target_os = "macos")]
fn event_tap_run_loop(
    state: &EventTapCallbackState,
    include_mouse_move: bool,
    ready_tx: Option<SyncSender<Result<(), RustyJackError>>>,
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
        fn CFMachPortInvalidate(port: *mut c_void);
        fn CFRelease(cf: *const c_void);
    }

    const K_CG_SESSION_EVENT_TAP: u32 = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
    const RUN_LOOP_SLICE: Duration = Duration::from_millis(250);

    extern "C" fn event_callback(
        _proxy: *mut c_void,
        event_type: u32,
        event: *mut c_void,
        user_info: *mut c_void,
    ) -> *mut c_void {
        // SAFETY: `user_info` is the `EventTapCallbackState` pointer installed by this module.
        if user_info.is_null() {
            return event;
        }
        let state = unsafe { &*(user_info as *const EventTapCallbackState) };
        if is_event_tap_disabled_notification(event_type) {
            state.startup_failed.store(true, Ordering::Release);
            if state.fallback_on_disable.load(Ordering::Acquire) {
                state.use_platform_idle.store(true, Ordering::Release);
                if state
                    .logged_disabled
                    .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    tracing::warn!(
                        target: "daemon",
                        "[activity] event tap disabled by macOS ({EVENT_TAP_PERMISSION_HINT})"
                    );
                }
            } else {
                let reason = if state
                    .logged_disabled
                    .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    format!(
                        "event tap disabled by macOS; requesting tap recreate ({EVENT_TAP_PERMISSION_HINT})"
                    )
                } else {
                    String::new()
                };
                request_tap_recreate_from_callback(state, &reason);
            }
            let tap = state.tap_port.load(Ordering::Relaxed) as *mut c_void;
            if !tap.is_null() {
                // SAFETY: `tap` is the port installed by this module.
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }

        state
            .last_event_unix_nanos
            .store(now_unix_nanos(), Ordering::Release);
        state.use_platform_idle.store(false, Ordering::Release);
        if let Ok(mut label) = state.last_event_label.lock() {
            *label = cg_event_type_label(event_type).into();
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
        let message = format!("CGEventTapCreate returned null ({EVENT_TAP_PERMISSION_HINT})");
        if let Some(ready_tx) = ready_tx.as_ref() {
            let _ = ready_tx.send(Err(RustyJackError::AppLaunch(message.clone())));
        }
        return Err(RustyJackError::AppLaunch(message));
    }

    state.tap_port.store(tap as usize, Ordering::Relaxed);

    // SAFETY: tap is non-null and owned for this thread.
    unsafe { CGEventTapEnable(tap, true) };

    let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
    if source.is_null() {
        let message: String = "CFMachPortCreateRunLoopSource failed for event tap".into();
        state.tap_port.store(0, Ordering::Relaxed);
        unsafe {
            CFMachPortInvalidate(tap);
            CFRelease(tap);
        }
        if let Some(ready_tx) = ready_tx.as_ref() {
            let _ = ready_tx.send(Err(RustyJackError::AppLaunch(message.clone())));
        }
        return Err(RustyJackError::AppLaunch(message));
    }

    if let Some(ready_tx) = ready_tx.as_ref() {
        let _ = ready_tx.send(Ok(()));
    } else {
        tracing::info!(
            target: "daemon",
            "[activity] event tap recreated after silent stall ({EVENT_TAP_PRIVACY_NOTE}; include_mouse_move={include_mouse_move})"
        );
    }

    let run_loop = CFRunLoop::get_current();
    // SAFETY: source is a valid run loop source for this thread.
    unsafe {
        extern "C" {
            fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
            fn CFRunLoopRemoveSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        }
        CFRunLoopAddSource(
            run_loop.as_concrete_TypeRef() as *mut c_void,
            source,
            kCFRunLoopDefaultMode.as_void_ptr(),
        );
        while !state.shutdown.load(Ordering::Acquire)
            && !state.recreate_requested.load(Ordering::Acquire)
        {
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, RUN_LOOP_SLICE, false);
        }
        CFRunLoopRemoveSource(
            run_loop.as_concrete_TypeRef() as *mut c_void,
            source,
            kCFRunLoopDefaultMode.as_void_ptr(),
        );
    }

    state.tap_port.store(0, Ordering::Relaxed);
    unsafe {
        CFMachPortInvalidate(tap);
        CFRelease(source);
        CFRelease(tap);
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

    #[test]
    fn is_event_tap_disabled_notification_matches_coregraphics_values() {
        assert!(is_event_tap_disabled_notification(
            K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
        ));
        assert!(is_event_tap_disabled_notification(
            K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        ));
        assert!(!is_event_tap_disabled_notification(K_CG_EVENT_KEY_DOWN));
    }

    #[test]
    fn accessibility_trust_probe_cache_hit_respects_cooldown_boundary() {
        let cooldown = ACCESSIBILITY_TRUST_PROBE_COOLDOWN;
        let last_probe = 100_000_000_000;
        assert!(accessibility_trust_probe_cache_hit(
            last_probe + cooldown.as_nanos() as u64 - 1,
            last_probe,
            cooldown,
        ));
        assert!(!accessibility_trust_probe_cache_hit(
            last_probe + cooldown.as_nanos() as u64,
            last_probe,
            cooldown,
        ));
    }

    #[test]
    fn resolve_accessibility_trust_probe_returns_cached_value_within_cooldown() {
        let cooldown = ACCESSIBILITY_TRUST_PROBE_COOLDOWN;
        let last_probe = 100_000_000_000;
        assert_eq!(
            resolve_accessibility_trust_probe(last_probe + 1, last_probe, true, cooldown, false,),
            (true, last_probe)
        );
    }

    #[test]
    fn resolve_accessibility_trust_probe_refreshes_after_cooldown() {
        let cooldown = ACCESSIBILITY_TRUST_PROBE_COOLDOWN;
        let last_probe = 100_000_000_000;
        let now = last_probe + cooldown.as_nanos() as u64;
        assert_eq!(
            resolve_accessibility_trust_probe(now, last_probe, true, cooldown, false),
            (false, now)
        );
        assert_eq!(
            resolve_accessibility_trust_probe(now, last_probe, true, cooldown, true),
            (true, now)
        );
    }

    #[test]
    fn should_try_event_tap_activity_monitor_requires_event_tap_and_trust() {
        let event_tap = Config {
            activity_monitor: "event_tap".into(),
            ..Default::default()
        };
        assert!(should_try_event_tap_activity_monitor(&event_tap, true));
        assert!(!should_try_event_tap_activity_monitor(&event_tap, false));
        let idle = Config {
            activity_monitor: "idle".into(),
            ..Default::default()
        };
        assert!(!should_try_event_tap_activity_monitor(&idle, true));
    }

    #[test]
    fn silent_tap_idle_action_selects_platform_idle_or_recreate() {
        assert_eq!(
            silent_tap_idle_action(true, true),
            SilentTapIdleAction::UsePlatformIdleWithMouseMove
        );
        assert_eq!(
            silent_tap_idle_action(false, false),
            SilentTapIdleAction::UsePlatformIdleUntrustedAccessibility
        );
        assert_eq!(
            silent_tap_idle_action(false, true),
            SilentTapIdleAction::RecreateTap
        );
    }

    #[test]
    fn silent_tap_idle_action_latches_platform_idle_for_untrusted_accessibility() {
        let use_platform_idle = AtomicBool::new(false);
        let action = silent_tap_idle_action(false, false);
        assert_eq!(
            action,
            SilentTapIdleAction::UsePlatformIdleUntrustedAccessibility
        );
        use_platform_idle.store(
            matches!(
                action,
                SilentTapIdleAction::UsePlatformIdleUntrustedAccessibility
                    | SilentTapIdleAction::UsePlatformIdleWithMouseMove
            ),
            Ordering::Release,
        );
        assert!(use_platform_idle.load(Ordering::Acquire));
    }

    #[test]
    fn event_tap_appears_silent_when_tap_idle_diverges_from_platform_idle() {
        // Synthetic idle inputs only; this compares durations and does not sleep.
        assert!(event_tap_appears_silent(
            Duration::from_secs(120),
            Duration::from_secs(1)
        ));
        assert!(!event_tap_appears_silent(
            Duration::from_secs(30),
            Duration::from_secs(1)
        ));
        assert!(!event_tap_appears_silent(
            Duration::from_secs(120),
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn poll_atomic_flag_while_detects_flag_without_sleeping() {
        let flag = AtomicBool::new(false);
        assert!(!poll_atomic_flag_while(&flag, 3, || {}));
        flag.store(true, Ordering::Release);
        assert!(poll_atomic_flag_while(&flag, 3, || {}));
    }

    #[test]
    fn should_request_tap_recreate_requires_silence_and_cooldown() {
        // Synthetic idle/cooldown inputs only; should_request_tap_recreate is pure logic.
        assert!(should_request_tap_recreate(
            Duration::from_secs(120),
            Duration::from_secs(1),
            TAP_RECREATE_COOLDOWN,
        ));
        assert!(!should_request_tap_recreate(
            Duration::from_secs(120),
            Duration::from_secs(1),
            Duration::from_secs(30),
        ));
        assert!(!should_request_tap_recreate(
            Duration::from_secs(30),
            Duration::from_secs(1),
            TAP_RECREATE_COOLDOWN,
        ));
    }

    #[test]
    fn arm_tap_recreate_respects_cooldown() {
        // Two back-to-back calls; the second fails immediately (no 10-minute wait).
        let last_recreate = AtomicU64::new(0);
        assert!(arm_tap_recreate(&last_recreate));
        assert!(!arm_tap_recreate(&last_recreate));
    }

    #[test]
    fn poll_atomic_flag_while_stops_on_flag_set() {
        let flag = AtomicBool::new(false);
        let attempts = AtomicUsize::new(0);
        assert!(poll_atomic_flag_while(&flag, 4, || {
            if attempts.fetch_add(1, Ordering::AcqRel) == 1 {
                flag.store(true, Ordering::Release);
            }
        }));
        assert_eq!(attempts.load(Ordering::Acquire), 2);
    }
}
