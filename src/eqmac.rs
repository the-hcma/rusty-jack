//! eqMac presence detection and auto-launch for HDMI software volume.

use crate::config::default_config_path;
use crate::output_device::OutputDevice;
use crate::RustyJackError;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const EQMAC_APP_NAME: &str = "eqMac";
const EQMAC_APP_PATH: &str = "/Applications/eqMac.app";
/// HAL driver shipped inside the eqMac app bundle (fallback when managed backup is gone).
pub const EQMAC_EMBEDDED_DRIVER_PATH: &str =
    "/Applications/eqMac.app/Contents/Resources/Embedded/eqMac.driver";
pub const EQMAC_HAL_DRIVER_PATH: &str = "/Library/Audio/Plug-Ins/HAL/eqMac.driver";
/// Minimum time between automatic eqMac restarts (unlock, scheduled health checks, startup).
pub const EQMAC_RESTART_COOLDOWN: Duration = Duration::from_secs(60);
const EQMAC_STARTUP_WAIT: Duration = Duration::from_millis(1500);
const EQMAC_STARTUP_POLL: Duration = Duration::from_millis(100);
const EQMAC_DRIVER_BACKUP_DIR_NAME: &str = "driver-backups";
const EQMAC_DRIVER_BACKUP_METADATA_NAME: &str = "eqMac.driver.json";
const EQMAC_LAST_RESTART_FILE_NAME: &str = "eqmac-last-restart";
const ENV_STATE_DIR: &str = "RUSTY_JACK_STATE_DIR";

/// Whether eqMac is present on the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EqMacInstallState {
    NotInstalled,
    Installed,
}

/// What `ensure_eqmac_running` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EqMacEnsureAction {
    /// Target route does not need eqMac (e.g. built-in only).
    NotNeeded,
    /// eqMac was already running.
    AlreadyRunning,
    /// eqMac was launched successfully.
    Launched,
    /// eqMac was restarted to recover a stale route.
    Restarted,
    /// HDMI-class route but eqMac is not installed.
    NotInstalled,
}

/// Outcome of ensuring eqMac is available for software volume on HDMI/DP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EqMacEnsureResult {
    pub action: EqMacEnsureAction,
}

/// Metadata for a managed backup of the eqMac HAL driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EqMacDriverBackupInfo {
    pub original_path: String,
    pub backup_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backed_up_at_unix: Option<u64>,
}

/// True when routing to `uid` needs a virtual volume router (HDMI-class outputs).
#[must_use]
pub fn routing_needs_eqmac(devices: &[OutputDevice], uid: &str) -> bool {
    devices
        .iter()
        .find(|d| d.uid == uid)
        .is_some_and(|d| d.transport.is_hdmi_class())
}

/// Detect whether the eqMac app is installed.
#[must_use]
pub fn eqmac_install_state() -> EqMacInstallState {
    if eqmac_app_path().is_some() {
        EqMacInstallState::Installed
    } else {
        EqMacInstallState::NotInstalled
    }
}

/// Path to the eqMac app bundle when present.
#[must_use]
pub fn eqmac_app_path() -> Option<String> {
    Path::new(EQMAC_APP_PATH)
        .exists()
        .then(|| EQMAC_APP_PATH.to_string())
}

/// Path to the eqMac HAL driver when present.
#[must_use]
pub fn eqmac_hal_driver_path() -> Option<String> {
    Path::new(EQMAC_HAL_DRIVER_PATH)
        .exists()
        .then(|| EQMAC_HAL_DRIVER_PATH.to_string())
}

/// Managed backup directory for temporary eqMac HAL driver swaps.
#[must_use]
pub fn eqmac_driver_backup_dir() -> Option<PathBuf> {
    default_config_path()
        .as_deref()
        .map(eqmac_driver_backup_dir_for_config_path)
}

#[must_use]
pub fn eqmac_driver_backup_dir_for_config_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(EQMAC_DRIVER_BACKUP_DIR_NAME)
}

/// Managed backup path for the eqMac HAL driver bundle.
#[must_use]
pub fn eqmac_driver_backup_path() -> Option<PathBuf> {
    eqmac_driver_backup_dir().map(|dir| dir.join("eqMac.driver"))
}

/// Managed metadata path for the eqMac HAL driver backup.
#[must_use]
pub fn eqmac_driver_backup_metadata_path() -> Option<PathBuf> {
    eqmac_driver_backup_dir().map(|dir| dir.join(EQMAC_DRIVER_BACKUP_METADATA_NAME))
}

/// Current managed eqMac HAL driver backup, using metadata when available.
#[must_use]
pub fn eqmac_driver_backup_info() -> Option<EqMacDriverBackupInfo> {
    if let Some(path) = eqmac_driver_backup_metadata_path() {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if let Ok(info) = serde_json::from_str::<EqMacDriverBackupInfo>(&contents) {
                return Some(info);
            }
        }
    }

    let backup_path = eqmac_driver_backup_path()?;
    backup_path.exists().then(|| EqMacDriverBackupInfo {
        original_path: EQMAC_HAL_DRIVER_PATH.into(),
        backup_path: backup_path.to_string_lossy().into_owned(),
        version: crate::hal_plugin::driver_bundle_info(&backup_path).and_then(|info| info.version),
        backed_up_at_unix: None,
    })
}

pub fn write_eqmac_driver_backup_info(
    backup_path: &Path,
    version: Option<String>,
) -> Result<EqMacDriverBackupInfo, RustyJackError> {
    let metadata_path = eqmac_driver_backup_metadata_path().ok_or_else(|| {
        RustyJackError::Config("HOME is not set; cannot locate eqMac driver backup metadata".into())
    })?;
    if let Some(parent) = metadata_path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }

    let info = EqMacDriverBackupInfo {
        original_path: EQMAC_HAL_DRIVER_PATH.into(),
        backup_path: backup_path.to_string_lossy().into_owned(),
        version,
        backed_up_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs()),
    };
    let json = serde_json::to_string_pretty(&info).map_err(|err| {
        RustyJackError::Config(format!("backup metadata serialization failed: {err}"))
    })?;
    std::fs::write(metadata_path, format!("{json}\n")).map_err(RustyJackError::Io)?;
    Ok(info)
}

pub fn remove_eqmac_driver_backup_info() -> Result<(), RustyJackError> {
    let Some(path) = eqmac_driver_backup_metadata_path() else {
        return Ok(());
    };
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(RustyJackError::Io(err)),
    }
}

/// A leftover eqMac HAL driver without the app bundle is not a usable fallback.
#[must_use]
pub fn orphaned_eqmac_hal_driver_path() -> Option<String> {
    if eqmac_app_path().is_none() {
        eqmac_hal_driver_path()
    } else {
        None
    }
}

/// True when the eqMac application process is running.
#[must_use]
pub fn eqmac_is_running() -> bool {
    crate::process_detect::any_process_with_exact_name(EQMAC_APP_NAME)
}

/// True when CoreAudio's default output is eqMac's virtual volume router.
#[must_use]
pub fn eqmac_virtual_default_is_active() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::coreaudio::volume::default_output_is_virtual_router()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// True when eqMac is running and owns the system default (volume keys can work).
#[must_use]
pub fn eqmac_volume_control_is_healthy(running: bool, virtual_default_active: bool) -> bool {
    running && virtual_default_active
}

/// Start eqMac if installed, not running, and the target route needs software volume.
///
/// When eqMac is already running but no longer owns the CoreAudio virtual default
/// (common after unlock / sleep), restarts it subject to [`EQMAC_RESTART_COOLDOWN`].
///
/// # Errors
///
/// Returns an error when eqMac is installed but `open` fails to launch the app.
pub fn ensure_eqmac_for_target(
    devices: &[OutputDevice],
    target_uid: &str,
) -> Result<EqMacEnsureResult, RustyJackError> {
    if !routing_needs_eqmac(devices, target_uid) {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotNeeded,
        });
    }

    ensure_eqmac_running()
}

/// Start eqMac when installed but not running; restart when running but stale.
///
/// # Errors
///
/// Returns an error when eqMac is installed but `open` fails to launch the app
/// for reasons other than the app being unavailable.
pub fn ensure_eqmac_running() -> Result<EqMacEnsureResult, RustyJackError> {
    if eqmac_install_state() == EqMacInstallState::NotInstalled {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        });
    }

    if eqmac_is_running() {
        if eqmac_volume_control_is_healthy(true, eqmac_virtual_default_is_active()) {
            return Ok(EqMacEnsureResult {
                action: EqMacEnsureAction::AlreadyRunning,
            });
        }
        return with_eqmac_restart_lock(|_lock| {
            // Another process may have recovered while we waited for the lock.
            if eqmac_volume_control_is_healthy(true, eqmac_virtual_default_is_active()) {
                return Ok(EqMacEnsureResult {
                    action: EqMacEnsureAction::AlreadyRunning,
                });
            }
            if !eqmac_restart_cooldown_elapsed() {
                return Ok(EqMacEnsureResult {
                    action: EqMacEnsureAction::AlreadyRunning,
                });
            }
            restart_eqmac_app()
        });
    }

    match launch_eqmac_app()? {
        EqMacLaunchAction::Launched => {
            let _ = wait_for_eqmac_running(EQMAC_STARTUP_WAIT);
            let _ = wait_for_eqmac_virtual_default(EQMAC_STARTUP_WAIT);
            Ok(EqMacEnsureResult {
                action: EqMacEnsureAction::Launched,
            })
        }
        EqMacLaunchAction::NotInstalled => Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        }),
    }
}

/// Restart eqMac for a target route that needs HDMI/DP software volume.
///
/// # Errors
///
/// Returns an error when eqMac is installed but cannot be relaunched.
pub fn restart_eqmac_for_target(
    devices: &[OutputDevice],
    target_uid: &str,
) -> Result<EqMacEnsureResult, RustyJackError> {
    if !routing_needs_eqmac(devices, target_uid) {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotNeeded,
        });
    }
    if eqmac_install_state() == EqMacInstallState::NotInstalled {
        return Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        });
    }

    with_eqmac_restart_lock(|_lock| restart_eqmac_app())
}

/// Human-readable lines for stderr after ensuring eqMac.
#[must_use]
pub fn format_ensure_messages(result: EqMacEnsureResult) -> Vec<String> {
    match result.action {
        EqMacEnsureAction::NotNeeded | EqMacEnsureAction::AlreadyRunning => vec![],
        EqMacEnsureAction::Launched => {
            vec!["Started eqMac (software volume for HDMI/DisplayPort).".into()]
        }
        EqMacEnsureAction::Restarted => {
            vec![
                "Restarted eqMac to restore HDMI/DisplayPort volume control (virtual default was missing)."
                    .into(),
            ]
        }
        EqMacEnsureAction::NotInstalled => vec![],
    }
}

fn classify_eqmac_launch(success: bool, stderr: &str) -> Result<EqMacLaunchAction, RustyJackError> {
    if success {
        return Ok(EqMacLaunchAction::Launched);
    }
    if stderr.contains("Unable to find application named") {
        return Ok(EqMacLaunchAction::NotInstalled);
    }

    Err(RustyJackError::AppLaunch(format!(
        "failed to launch eqMac: {stderr}"
    )))
}

fn eqmac_last_restart_path() -> Option<PathBuf> {
    eqmac_state_dir().map(|dir| dir.join(EQMAC_LAST_RESTART_FILE_NAME))
}

fn eqmac_restart_cooldown_elapsed() -> bool {
    let Some(path) = eqmac_last_restart_path() else {
        return true;
    };
    let mut raw = String::new();
    let Ok(mut file) = File::open(&path) else {
        return true;
    };
    if file.read_to_string(&mut raw).is_err() {
        return true;
    }
    let Ok(secs) = raw.trim().parse::<u64>() else {
        return true;
    };
    let Some(at) = UNIX_EPOCH.checked_add(Duration::from_secs(secs)) else {
        return true;
    };
    match SystemTime::now().duration_since(at) {
        Ok(elapsed) => elapsed >= EQMAC_RESTART_COOLDOWN,
        Err(_) => true,
    }
}

fn eqmac_state_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(ENV_STATE_DIR) {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    if cfg!(test) {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local/state/rusty-jack"))
}

fn kill_eqmac_app() {
    crate::process_detect::kill_processes_with_exact_name(EQMAC_APP_NAME);
}

fn launch_eqmac_app() -> Result<EqMacLaunchAction, RustyJackError> {
    let output = std::process::Command::new("open")
        .args(["-a", EQMAC_APP_NAME])
        .output()
        .map_err(RustyJackError::Io)?;

    classify_eqmac_launch(
        output.status.success(),
        &String::from_utf8_lossy(&output.stderr),
    )
}

fn quit_eqmac_app() {
    let _ = std::process::Command::new("osascript")
        .args(["-e", "tell application \"eqMac\" to quit"])
        .output();
}

fn record_eqmac_restart() {
    let Some(path) = eqmac_last_restart_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let _ = std::fs::write(path, format!("{secs}\n"));
}

fn restart_eqmac_app() -> Result<EqMacEnsureResult, RustyJackError> {
    if eqmac_is_running() {
        quit_eqmac_app();
        let _ = wait_for_eqmac_shutdown(EQMAC_STARTUP_WAIT);
        if eqmac_is_running() {
            kill_eqmac_app();
            let _ = wait_for_eqmac_shutdown(EQMAC_STARTUP_WAIT);
        }
    }

    match launch_eqmac_app()? {
        EqMacLaunchAction::Launched => {
            record_eqmac_restart();
            let _ = wait_for_eqmac_running(EQMAC_STARTUP_WAIT);
            let _ = wait_for_eqmac_virtual_default(EQMAC_STARTUP_WAIT);
            Ok(EqMacEnsureResult {
                action: EqMacEnsureAction::Restarted,
            })
        }
        EqMacLaunchAction::NotInstalled => Ok(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        }),
    }
}

fn wait_for_eqmac_running(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if eqmac_is_running() {
            return true;
        }
        thread::sleep(EQMAC_STARTUP_POLL);
    }
    eqmac_is_running()
}

fn wait_for_eqmac_shutdown(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !eqmac_is_running() {
            return true;
        }
        thread::sleep(EQMAC_STARTUP_POLL);
    }
    !eqmac_is_running()
}

fn wait_for_eqmac_virtual_default(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if eqmac_virtual_default_is_active() {
            return true;
        }
        thread::sleep(EQMAC_STARTUP_POLL);
    }
    eqmac_virtual_default_is_active()
}

fn with_eqmac_restart_lock<T>(
    f: impl FnOnce(Option<&File>) -> Result<T, RustyJackError>,
) -> Result<T, RustyJackError> {
    let Some(path) = eqmac_last_restart_path() else {
        return f(None);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(RustyJackError::Io)?;
    flock_exclusive(&file)?;
    let result = f(Some(&file));
    let _ = flock_unlock(&file);
    result
}

fn flock_exclusive(file: &File) -> Result<(), RustyJackError> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        #[allow(unsafe_code)]
        // SAFETY: `file` remains open for the duration of this flock call.
        let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if status != 0 {
            return Err(RustyJackError::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

fn flock_unlock(file: &File) -> Result<(), RustyJackError> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        #[allow(unsafe_code)]
        // SAFETY: `file` remains open for the duration of this flock call.
        let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        if status != 0 {
            return Err(RustyJackError::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EqMacLaunchAction {
    Launched,
    NotInstalled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn device(uid: &str, transport: TransportKind) -> OutputDevice {
        OutputDevice {
            id: 1,
            uid: uid.into(),
            name: "Out".into(),
            transport,
            is_alive: true,
            is_default: false,
            is_active: false,
        }
    }

    #[test]
    fn test_routing_needs_eqmac_for_hdmi() {
        let devices = vec![device("hdmi", TransportKind::Hdmi)];
        assert!(routing_needs_eqmac(&devices, "hdmi"));
    }

    #[test]
    fn test_routing_needs_eqmac_not_for_builtin() {
        let devices = vec![device("builtin", TransportKind::BuiltIn)];
        assert!(!routing_needs_eqmac(&devices, "builtin"));
    }

    #[test]
    fn test_format_ensure_launched_message() {
        let lines = format_ensure_messages(EqMacEnsureResult {
            action: EqMacEnsureAction::Launched,
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Started eqMac"));
    }

    #[test]
    fn test_format_ensure_restarted_message() {
        let lines = format_ensure_messages(EqMacEnsureResult {
            action: EqMacEnsureAction::Restarted,
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Restarted eqMac"));
        assert!(lines[0].contains("virtual default was missing"));
    }

    #[test]
    fn test_format_ensure_not_installed_stays_quiet() {
        let lines = format_ensure_messages(EqMacEnsureResult {
            action: EqMacEnsureAction::NotInstalled,
        });
        assert!(lines.is_empty());
    }

    #[test]
    fn test_missing_eqmac_app_is_not_fatal() {
        let result =
            classify_eqmac_launch(false, "Unable to find application named 'eqMac'\n").unwrap();
        assert_eq!(result, EqMacLaunchAction::NotInstalled);
    }

    #[test]
    fn test_eqmac_driver_backup_dir_uses_config_parent() {
        let backup_dir = eqmac_driver_backup_dir_for_config_path(Path::new(
            "/Users/example/.config/rusty-jack/config.json",
        ));
        assert_eq!(
            backup_dir,
            PathBuf::from("/Users/example/.config/rusty-jack/driver-backups")
        );
    }

    #[test]
    fn test_eqmac_volume_control_healthy_requires_running_and_virtual_default() {
        assert!(eqmac_volume_control_is_healthy(true, true));
        assert!(!eqmac_volume_control_is_healthy(true, false));
        assert!(!eqmac_volume_control_is_healthy(false, true));
        assert!(!eqmac_volume_control_is_healthy(false, false));
    }

    #[test]
    fn test_eqmac_restart_cooldown_reads_persisted_timestamp() {
        with_state_dir(|| {
            let path = eqmac_last_restart_path().expect("state dir set");
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            std::fs::write(&path, format!("{now}\n")).unwrap();
            assert!(!eqmac_restart_cooldown_elapsed());
            let old = now.saturating_sub(EQMAC_RESTART_COOLDOWN.as_secs() + 5);
            std::fs::write(&path, format!("{old}\n")).unwrap();
            assert!(eqmac_restart_cooldown_elapsed());
        });
    }

    fn with_state_dir<T>(f: impl FnOnce() -> T) -> T {
        use std::sync::{Mutex, MutexGuard};
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard: MutexGuard<'_, ()> = LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(ENV_STATE_DIR, dir.path());
        let result = f();
        std::env::remove_var(ENV_STATE_DIR);
        result
    }

    #[test]
    fn test_other_eqmac_launch_failure_stays_fatal() {
        let err = classify_eqmac_launch(false, "permission denied").unwrap_err();
        assert!(matches!(err, RustyJackError::AppLaunch(_)));
    }
}
