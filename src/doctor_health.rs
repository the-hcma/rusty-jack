//! Daemon, routing and eqMac health checks for `rusty-jack doctor` (issue #276).
//!
//! `doctor` used to look only at macOS privacy permissions, so a paused or stale daemon (the
//! most common reasons routing and HDMI/DisplayPort volume control stop working) produced a
//! clean result. These checks reuse the data `status` already computes. The decision logic is a
//! pure function of [`HealthInputs`] so every case is unit-testable without launchd or CoreAudio.

use crate::doctor::{CheckState, DoctorCheck};
use crate::hdmi_displayport_volume_control::HdmiDisplayPortVolumeControlStatus;
use crate::launchd::{daemon_process_running, DaemonPauseReason, DaemonStatus, DaemonVersionCheck};
use crate::status::PolicyStatus;

/// Maximum number of recent daemon warning/error lines shown.
const MAX_LOG_PROBLEMS: usize = 3;
/// Longest log line shown, in characters.
const MAX_LOG_LINE_CHARS: usize = 180;

/// Everything the health checks need, gathered by the caller.
#[derive(Debug)]
pub struct HealthInputs<'a> {
    /// A config was loaded (without one, auto-routing is not configured).
    pub config_loaded: bool,
    /// `auto_switch` from the config: the daemon is expected to keep audio routed.
    pub auto_switch: bool,
    /// `None` when the LaunchAgent state could not be inspected.
    pub daemon: Option<&'a DaemonStatus>,
    pub version: Option<&'a DaemonVersionCheck>,
    /// `None` when CoreAudio could not be queried.
    pub policy: Option<&'a PolicyStatus>,
    pub volume: Option<&'a HdmiDisplayPortVolumeControlStatus>,
    /// The selected route is an HDMI/DisplayPort output that needs volume control.
    pub volume_needed: bool,
    pub eqmac_running: bool,
    pub eqmac_virtual_default: bool,
    /// `rusty-jack install` offers the native driver (off by default until a signed bundle ships).
    pub native_driver_offered: bool,
    /// Why CoreAudio could not be queried (HAL creation or enumeration failed), if it could not.
    pub audio_error: Option<&'a str>,
    pub log: &'a LogSummary,
}

/// What the tail of the daemon log says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogSummary {
    /// Timestamp of the newest log line (as logged).
    pub last_entry_at: Option<String>,
    /// The newest warning/error lines, oldest first, trimmed. When `since_daemon_start` is set
    /// they are only those logged since the last recorded daemon start.
    pub recent_problems: Vec<String>,
    /// The tail contained the daemon's `[daemon] started` line, so `recent_problems` excludes
    /// earlier processes. A long-running daemon's start line is often beyond the tail read.
    pub since_daemon_start: bool,
}

/// Summarize a daemon log excerpt: its newest timestamp and the last few WARN/ERROR lines.
#[must_use]
pub fn summarize_daemon_log(tail: &str) -> LogSummary {
    let mut last_entry_at = None;
    let mut since_daemon_start = false;
    let mut problems: Vec<String> = Vec::new();
    for line in tail.lines() {
        let mut parts = line.splitn(4, " | ");
        let (Some(stamp), Some(level), Some(_target), Some(message)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let stamp = stamp.trim();
        if stamp.len() < 15 || !stamp.as_bytes()[0].is_ascii_digit() {
            continue;
        }
        last_entry_at = Some(stamp.to_string());
        // Problems from an earlier daemon process say nothing about this one: start over.
        if message.contains("[daemon] started version=") {
            problems.clear();
            since_daemon_start = true;
            continue;
        }
        if matches!(level.trim(), "WARN" | "ERROR") {
            let mut text = format!("{stamp} {}: {}", level.trim(), message.trim());
            if text.chars().count() > MAX_LOG_LINE_CHARS {
                text = text.chars().take(MAX_LOG_LINE_CHARS).collect::<String>() + "…";
            }
            problems.push(text);
        }
    }
    let skip = problems.len().saturating_sub(MAX_LOG_PROBLEMS);
    LogSummary {
        last_entry_at,
        recent_problems: problems.split_off(skip),
        since_daemon_start,
    }
}

fn check(
    id: &'static str,
    label: &'static str,
    state: CheckState,
    detail: impl Into<String>,
    fix: Option<&str>,
) -> DoctorCheck {
    DoctorCheck {
        id,
        label,
        state,
        detail: detail.into(),
        fix: fix.map(str::to_string),
    }
}

/// A daemon that should be keeping audio routed but is not is a failure; without
/// `auto_switch` it is only worth a warning.
fn not_running_state(auto_switch: bool) -> CheckState {
    if auto_switch {
        CheckState::Fail
    } else {
        CheckState::Warn
    }
}

fn daemon_state_check(inputs: &HealthInputs<'_>) -> DoctorCheck {
    const ID: &str = "daemon_state";
    const LABEL: &str = "Daemon";
    let bad = not_running_state(inputs.auto_switch);
    let suffix = if inputs.auto_switch {
        "; auto-routing is configured but nothing is running it"
    } else {
        ""
    };
    match inputs.daemon {
        None => check(
            ID,
            LABEL,
            CheckState::Warn,
            "could not inspect the LaunchAgent",
            Some("run `rusty-jack status` for the underlying error"),
        ),
        Some(DaemonStatus::NotInstalled { .. }) => check(
            ID,
            LABEL,
            bad,
            format!("LaunchAgent is not installed{suffix}"),
            Some("rusty-jack install"),
        ),
        Some(DaemonStatus::Paused { pause_reason, .. }) => {
            let why = match pause_reason {
                Some(DaemonPauseReason::PickerOverride { selected_label, .. }) => {
                    format!(" after a picker override to {selected_label}")
                }
                None => String::new(),
            };
            check(
                ID,
                LABEL,
                bad,
                format!("paused{why}, so audio is not being re-routed{suffix}"),
                Some("rusty-jack resume"),
            )
        }
        Some(status @ DaemonStatus::Running { .. }) if daemon_process_running(status) => {
            let pid = match status {
                DaemonStatus::Running { pid: Some(pid), .. } => *pid,
                _ => 0,
            };
            check(
                ID,
                LABEL,
                CheckState::Ok,
                format!("running (pid {pid})"),
                None,
            )
        }
        Some(DaemonStatus::Running {
            launch_job_state,
            last_exit_code,
            ..
        }) => {
            let mut detail = String::from("loaded in launchd but not running");
            if let Some(state) = launch_job_state {
                detail.push_str(&format!(" (launchd state={state}"));
                if let Some(code) = last_exit_code {
                    detail.push_str(&format!(", last exit code={code}"));
                }
                detail.push(')');
            }
            detail.push_str(suffix);
            check(
                ID,
                LABEL,
                bad,
                detail,
                Some("rusty-jack upgrade --force (or rusty-jack resume if it was paused)"),
            )
        }
        Some(DaemonStatus::Unknown { message, .. }) => check(
            ID,
            LABEL,
            CheckState::Warn,
            format!("state unknown: {message}"),
            Some("rusty-jack upgrade --force"),
        ),
    }
}

fn daemon_binary_check(inputs: &HealthInputs<'_>) -> DoctorCheck {
    const ID: &str = "daemon_binary";
    const LABEL: &str = "Daemon binary";
    let Some(daemon) = inputs
        .daemon
        .filter(|daemon| !matches!(daemon, DaemonStatus::NotInstalled { .. }))
    else {
        return check(
            ID,
            LABEL,
            CheckState::Skipped,
            "no installed daemon to compare",
            None,
        );
    };
    let _ = daemon;
    let Some(version) = inputs.version else {
        // The daemon is installed, but its plist or binary could not be read.
        return check(
            ID,
            LABEL,
            CheckState::Warn,
            "the installed daemon's version could not be read, so it cannot be compared with the CLI",
            Some("rusty-jack upgrade --force"),
        );
    };
    let cli = format!(
        "{} ({})",
        version.cli_version.version, version.cli_version.commit
    );
    // A real version/commit mismatch is the failure. `stale` also trips on a different binary
    // path or a missing version stamp, which are worth a warning, not a non-zero exit.
    let mismatched = version
        .running_binary_version
        .iter()
        .chain(version.plist_binary_version.iter())
        .find(|v| !v.matches(&version.cli_version));
    if let Some(found) = mismatched {
        return check(
            ID,
            LABEL,
            CheckState::Fail,
            format!(
                "daemon runs {} ({}) but the CLI is {cli}",
                found.version, found.commit
            ),
            Some(version.refresh_command),
        );
    }
    if version.needs_version_stamp_refresh {
        return check(
            ID,
            LABEL,
            CheckState::Warn,
            "the LaunchAgent has no version stamp, so staleness cannot be checked",
            Some(version.refresh_command),
        );
    }
    if let Some(path) = version
        .plist_binary_path
        .as_deref()
        .filter(|path| *path != version.cli_binary_path)
    {
        return check(
            ID,
            LABEL,
            CheckState::Warn,
            format!(
                "same version {cli}, but the daemon runs {path} while this CLI is {}",
                version.cli_binary_path
            ),
            Some(version.refresh_command),
        );
    }
    check(
        ID,
        LABEL,
        CheckState::Ok,
        format!("matches the CLI ({cli})"),
        None,
    )
}

fn eqmac_check(inputs: &HealthInputs<'_>) -> DoctorCheck {
    const ID: &str = "eqmac_readiness";
    const LABEL: &str = "HDMI/DisplayPort volume control";
    let Some(volume) = inputs.volume.filter(|_| inputs.volume_needed) else {
        return check(
            ID,
            LABEL,
            CheckState::Skipped,
            "the selected output does not need software volume control",
            None,
        );
    };
    // An installed driver that reports a warning (null/skeleton stage, or CoreAudio has not
    // published its output) does not provide volume control: fall through to eqMac.
    let driver_note = volume
        .native_driver_installed
        .then_some(volume.native_driver_warning.as_deref())
        .flatten();
    if volume.native_driver_installed && driver_note.is_none() {
        return check(
            ID,
            LABEL,
            CheckState::Ok,
            "the Rusty Jack native driver provides volume control",
            None,
        );
    }
    if !volume.eqmac_installed {
        if let Some(note) = driver_note {
            return check(
                ID,
                LABEL,
                CheckState::Warn,
                format!(
                    "the Rusty Jack native driver is installed but not working ({note}) and eqMac is not installed"
                ),
                Some("see `rusty-jack status` for the driver details, or install eqMac (https://eqmac.app)"),
            );
        }
        let detail = volume
            .recommendation
            .clone()
            .unwrap_or_else(|| "neither the Rusty Jack driver nor eqMac is installed".into());
        return check(
            ID,
            LABEL,
            CheckState::Warn,
            detail,
            // `install` only installs the native driver when it is offered; otherwise it
            // does nothing for volume control, so point at eqMac instead.
            Some(if inputs.native_driver_offered {
                "rusty-jack install (installs the Rusty Jack native driver)"
            } else {
                "install eqMac (https://eqmac.app); the Rusty Jack native driver is not offered yet"
            }),
        );
    }
    if !inputs.eqmac_running {
        return check(
            ID,
            LABEL,
            CheckState::Warn,
            "eqMac is installed but not running, so the volume keys cannot control this output",
            Some("start eqMac (the daemon does this when it is running; see `rusty-jack resume`)"),
        );
    }
    if !inputs.eqmac_virtual_default {
        return check(
            ID,
            LABEL,
            CheckState::Warn,
            "eqMac is running but its virtual output is not the system default",
            Some("restart eqMac, or let the running daemon restart it"),
        );
    }
    check(
        ID,
        LABEL,
        CheckState::Ok,
        "eqMac is running and owns the default output",
        None,
    )
}

fn routing_check(inputs: &HealthInputs<'_>) -> DoctorCheck {
    const ID: &str = "routing";
    const LABEL: &str = "Audio routing";
    let (true, Some(policy)) = (inputs.config_loaded, inputs.policy) else {
        return check(
            ID,
            LABEL,
            CheckState::Skipped,
            "no preferred output is configured",
            None,
        );
    };
    // An unplugged preferred output reports `matches_preferred: Some(false)`, but nothing can
    // re-route to it, so a running or stopped daemon is beside the point.
    if policy.preferred_present == Some(false) {
        return check(
            ID,
            LABEL,
            CheckState::Skipped,
            format!(
                "the preferred output ({}) is not connected",
                policy
                    .preferred_device_label
                    .as_deref()
                    .or(policy.preferred_device_name.as_deref())
                    .unwrap_or("unknown")
            ),
            None,
        );
    }
    // Listed but not alive: the daemon's target selection skips it and falls back, so it will
    // not "fix" this, and `apply` cannot either.
    if policy.preferred_alive == Some(false) {
        return check(
            ID,
            LABEL,
            CheckState::Skipped,
            format!(
                "the preferred output ({}) is listed but not available",
                policy
                    .preferred_device_label
                    .as_deref()
                    .or(policy.preferred_device_name.as_deref())
                    .unwrap_or("unknown")
            ),
            None,
        );
    }
    match policy.matches_preferred {
        Some(true) => check(
            ID,
            LABEL,
            CheckState::Ok,
            "the default output is the preferred one",
            None,
        ),
        Some(false) if inputs.auto_switch => {
            let daemon_working = inputs.daemon.is_some_and(daemon_process_running);
            if daemon_working {
                // A running daemon will correct this on its next poll: a transient state.
                check(
                    ID,
                    LABEL,
                    CheckState::Ok,
                    "the default output differs from the preferred one; the running daemon will switch it",
                    None,
                )
            } else {
                check(
                    ID,
                    LABEL,
                    CheckState::Warn,
                    format!(
                        "the default output ({}) is not the preferred one ({}) and the daemon is not running to fix it",
                        policy.active_device_label.as_deref().unwrap_or("unknown"),
                        policy.preferred_device_label.as_deref().unwrap_or("unknown"),
                    ),
                    Some("rusty-jack apply (once), or fix the daemon above"),
                )
            }
        }
        Some(false) => check(
            ID,
            LABEL,
            CheckState::Skipped,
            "the default output differs from the preferred one, and auto_switch is off",
            None,
        ),
        None => check(
            ID,
            LABEL,
            CheckState::Skipped,
            "no preferred output could be resolved",
            None,
        ),
    }
}

fn log_check(inputs: &HealthInputs<'_>) -> DoctorCheck {
    const ID: &str = "daemon_log";
    const LABEL: &str = "Daemon log";
    let log = inputs.log;
    let Some(last) = log.last_entry_at.as_deref() else {
        return check(
            ID,
            LABEL,
            CheckState::Skipped,
            "no daemon log entries found",
            None,
        );
    };
    // Only claim "since the last recorded daemon start" when its start line was actually in the tail.
    let scope = if log.since_daemon_start {
        "since the last recorded daemon start"
    } else {
        "in the part of the log read (the daemon's start is older, so some may predate it)"
    };
    if log.recent_problems.is_empty() {
        return check(
            ID,
            LABEL,
            CheckState::Ok,
            format!("no warnings or errors {scope} (last entry {last})"),
            None,
        );
    }
    let lines = log.recent_problems.join("\n");
    check(
        ID,
        LABEL,
        CheckState::Warn,
        format!("warnings/errors {scope} (last entry {last}):\n{lines}"),
        Some("see `rusty-jack status` for the log path"),
    )
}

/// All health checks, in display order.
#[must_use]
pub fn health_checks(inputs: &HealthInputs<'_>) -> Vec<DoctorCheck> {
    let mut checks = vec![
        daemon_state_check(inputs),
        daemon_binary_check(inputs),
        eqmac_check(inputs),
        routing_check(inputs),
        log_check(inputs),
    ];
    // Without audio device data the routing and volume checks skip silently, which must not
    // read as healthy: say why.
    if let Some(error) = inputs.audio_error {
        checks.insert(
            2,
            check(
                "audio_devices",
                "Audio devices",
                CheckState::Warn,
                format!("could not query CoreAudio, so routing and volume control were not checked: {error}"),
                Some("run `rusty-jack list` to see the underlying error"),
            ),
        );
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::BinaryVersion;

    fn running() -> DaemonStatus {
        DaemonStatus::Running {
            label: "l".into(),
            plist_path: "p".into(),
            service: "s".into(),
            pid: Some(42),
            launch_job_state: Some("running".into()),
            last_exit_code: None,
        }
    }

    fn paused() -> DaemonStatus {
        DaemonStatus::Paused {
            label: "l".into(),
            plist_path: "p".into(),
            service: "s".into(),
            pause_reason: None,
        }
    }

    fn loaded_not_running() -> DaemonStatus {
        DaemonStatus::Running {
            label: "l".into(),
            plist_path: "p".into(),
            service: "s".into(),
            pid: None,
            launch_job_state: Some("spawn scheduled".into()),
            last_exit_code: Some("78".into()),
        }
    }

    fn version(stale: bool, stamp_refresh: bool) -> DaemonVersionCheck {
        let v = |version: &str, commit: &str| BinaryVersion {
            version: version.into(),
            commit: commit.into(),
        };
        DaemonVersionCheck {
            cli_binary_path: "/cli".into(),
            cli_version: v("0.13.0", "ba05438"),
            plist_binary_path: Some("/cli".into()),
            plist_binary_version: Some(v(
                if stale { "0.12.1" } else { "0.13.0" },
                if stale { "9597053" } else { "ba05438" },
            )),
            running_binary_version: None,
            needs_version_stamp_refresh: stamp_refresh,
            stale,
            refresh_command: "rusty-jack upgrade --force",
        }
    }

    fn policy(matches: Option<bool>) -> PolicyStatus {
        PolicyStatus {
            configured: true,
            config_path: None,
            preferred_device_name: None,
            preferred_device_label: Some("Monitor".into()),
            preferred_device_uid: Some("hdmi".into()),
            active_device_label: Some("Speakers".into()),
            active_device_uid: Some("spk".into()),
            matches_preferred: matches,
            preferred_present: Some(true),
            preferred_alive: Some(true),
            auto_switch: Some(true),
            config_volume: None,
            message: String::new(),
        }
    }

    fn volume(native: bool, eqmac: bool) -> HdmiDisplayPortVolumeControlStatus {
        HdmiDisplayPortVolumeControlStatus {
            connected_output_present: true,
            native_driver_installed: native,
            native_driver_recommended: false,
            native_driver_recommendation_reason: None,
            native_driver_install_path: None,
            native_driver_scope: None,
            native_driver_stage: None,
            native_driver_warning: None,
            native_driver: None,
            eqmac_installed: eqmac,
            eqmac_app_path: None,
            eqmac_hal_driver_path: None,
            orphaned_eqmac_hal_driver_path: None,
            eqmac_driver_backup: None,
            recommendation: None,
        }
    }

    struct Fixture {
        daemon: Option<DaemonStatus>,
        version: Option<DaemonVersionCheck>,
        policy: Option<PolicyStatus>,
        volume: Option<HdmiDisplayPortVolumeControlStatus>,
        log: LogSummary,
        auto_switch: bool,
        volume_needed: bool,
        eqmac_running: bool,
        eqmac_virtual_default: bool,
        native_driver_offered: bool,
        audio_error: Option<String>,
    }

    impl Fixture {
        fn healthy() -> Self {
            Self {
                daemon: Some(running()),
                version: Some(version(false, false)),
                policy: Some(policy(Some(true))),
                volume: Some(volume(false, true)),
                log: LogSummary::default(),
                auto_switch: true,
                volume_needed: true,
                eqmac_running: true,
                eqmac_virtual_default: true,
                native_driver_offered: false,
                audio_error: None,
            }
        }

        fn checks(&self) -> Vec<DoctorCheck> {
            health_checks(&HealthInputs {
                config_loaded: true,
                auto_switch: self.auto_switch,
                daemon: self.daemon.as_ref(),
                version: self.version.as_ref(),
                policy: self.policy.as_ref(),
                volume: self.volume.as_ref(),
                volume_needed: self.volume_needed,
                eqmac_running: self.eqmac_running,
                eqmac_virtual_default: self.eqmac_virtual_default,
                native_driver_offered: self.native_driver_offered,
                audio_error: self.audio_error.as_deref(),
                log: &self.log,
            })
        }

        fn state(&self, id: &str) -> CheckState {
            self.checks().iter().find(|c| c.id == id).unwrap().state
        }

        fn fix(&self, id: &str) -> String {
            self.checks()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .fix
                .clone()
                .unwrap_or_default()
        }
    }

    #[test]
    fn test_healthy_setup_has_no_warnings_or_failures() {
        let checks = Fixture::healthy().checks();
        assert!(
            checks
                .iter()
                .all(|c| matches!(c.state, CheckState::Ok | CheckState::Skipped)),
            "{checks:#?}"
        );
        assert_eq!(checks.len(), 5);
    }

    #[test]
    fn test_paused_daemon_with_auto_switch_fails_and_suggests_resume() {
        let f = Fixture {
            daemon: Some(paused()),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_state"), CheckState::Fail);
        assert_eq!(f.fix("daemon_state"), "rusty-jack resume");
    }

    #[test]
    fn test_paused_daemon_without_auto_switch_only_warns() {
        let f = Fixture {
            daemon: Some(paused()),
            auto_switch: false,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_state"), CheckState::Warn);
    }

    #[test]
    fn test_not_installed_daemon_suggests_install() {
        let f = Fixture {
            daemon: Some(DaemonStatus::NotInstalled {
                plist_path: "p".into(),
            }),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_state"), CheckState::Fail);
        assert_eq!(f.fix("daemon_state"), "rusty-jack install");
        // And there is no installed daemon to compare versions with.
        assert_eq!(f.state("daemon_binary"), CheckState::Skipped);
    }

    #[test]
    fn test_loaded_but_not_running_daemon_fails_with_launchd_detail() {
        let f = Fixture {
            daemon: Some(loaded_not_running()),
            ..Fixture::healthy()
        };
        let checks = f.checks();
        let c = checks.iter().find(|c| c.id == "daemon_state").unwrap();
        assert_eq!(c.state, CheckState::Fail);
        assert!(c.detail.contains("spawn scheduled") && c.detail.contains("last exit code=78"));
    }

    #[test]
    fn test_stale_daemon_binary_fails_with_refresh_command() {
        let f = Fixture {
            version: Some(version(true, false)),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_binary"), CheckState::Fail);
        assert_eq!(f.fix("daemon_binary"), "rusty-jack upgrade --force");
        let checks = f.checks();
        let c = checks.iter().find(|c| c.id == "daemon_binary").unwrap();
        assert!(
            c.detail.contains("0.12.1") && c.detail.contains("0.13.0"),
            "{}",
            c.detail
        );
    }

    #[test]
    fn test_same_version_from_a_different_binary_path_only_warns() {
        let mut v = version(false, false);
        v.plist_binary_path = Some("/opt/homebrew/bin/rusty-jack".into());
        let f = Fixture {
            version: Some(v),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_binary"), CheckState::Warn);
        let checks = f.checks();
        let c = checks.iter().find(|c| c.id == "daemon_binary").unwrap();
        assert!(c.detail.contains("same version"), "{}", c.detail);
    }

    #[test]
    fn test_a_running_daemon_older_than_the_plist_is_still_stale() {
        let mut v = version(false, false);
        v.running_binary_version = Some(BinaryVersion {
            version: "0.12.1".into(),
            commit: "9597053".into(),
        });
        let f = Fixture {
            version: Some(v),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_binary"), CheckState::Fail);
    }

    #[test]
    fn test_missing_version_stamp_warns() {
        let f = Fixture {
            version: Some(version(false, true)),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_binary"), CheckState::Warn);
    }

    #[test]
    fn test_eqmac_installed_but_not_running_warns() {
        let f = Fixture {
            eqmac_running: false,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("eqmac_readiness"), CheckState::Warn);
    }

    #[test]
    fn test_eqmac_running_without_virtual_default_warns() {
        let f = Fixture {
            eqmac_virtual_default: false,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("eqmac_readiness"), CheckState::Warn);
    }

    #[test]
    fn test_a_failed_audio_query_is_a_warning_not_a_clean_skip() {
        let f = Fixture {
            policy: None,
            volume: None,
            audio_error: Some("no CoreAudio device list".into()),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("audio_devices"), CheckState::Warn);
        let checks = f.checks();
        let c = checks.iter().find(|c| c.id == "audio_devices").unwrap();
        assert!(
            c.detail.contains("no CoreAudio device list"),
            "{}",
            c.detail
        );
        assert_eq!(
            c.fix.as_deref(),
            Some("run `rusty-jack list` to see the underlying error")
        );
        // Without the error the check is absent, so a healthy setup keeps five checks.
        assert_eq!(Fixture::healthy().checks().len(), 5);
    }

    #[test]
    fn test_an_installed_daemon_with_unreadable_version_info_warns() {
        let f = Fixture {
            version: None,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_binary"), CheckState::Warn);
        // An absent daemon is still just "nothing to compare".
        let absent = Fixture {
            daemon: Some(DaemonStatus::NotInstalled {
                plist_path: "p".into(),
            }),
            version: None,
            ..Fixture::healthy()
        };
        assert_eq!(absent.state("daemon_binary"), CheckState::Skipped);
    }

    #[test]
    fn test_a_native_driver_that_warns_does_not_count_as_volume_control() {
        let mut broken = volume(true, false);
        broken.native_driver_warning =
            Some("CoreAudio has not published the Rusty Jack output".into());

        // No eqMac fallback: warn, say the driver is installed but not working, and do not
        // tell the user to install what is already installed.
        let none = Fixture {
            volume: Some(broken.clone()),
            ..Fixture::healthy()
        };
        assert_eq!(none.state("eqmac_readiness"), CheckState::Warn);
        let checks = none.checks();
        let c = checks.iter().find(|c| c.id == "eqmac_readiness").unwrap();
        assert!(
            c.detail.contains("installed but not working"),
            "{}",
            c.detail
        );
        assert!(!c
            .fix
            .clone()
            .unwrap_or_default()
            .contains("rusty-jack install"));

        // A working eqMac is a fine fallback.
        let mut with_eqmac = broken.clone();
        with_eqmac.eqmac_installed = true;
        let ok = Fixture {
            volume: Some(with_eqmac),
            ..Fixture::healthy()
        };
        assert_eq!(ok.state("eqmac_readiness"), CheckState::Ok);

        // A native driver without a warning is still ok.
        let healthy_driver = Fixture {
            volume: Some(volume(true, false)),
            ..Fixture::healthy()
        };
        assert_eq!(healthy_driver.state("eqmac_readiness"), CheckState::Ok);
    }

    #[test]
    fn test_a_listed_but_unavailable_preferred_output_is_not_promised_a_fix() {
        let mut dead = policy(Some(false));
        dead.preferred_alive = Some(false);
        let f = Fixture {
            policy: Some(dead),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("routing"), CheckState::Skipped);
        let checks = f.checks();
        let c = checks.iter().find(|c| c.id == "routing").unwrap();
        assert!(c.detail.contains("not available"), "{}", c.detail);
    }

    #[test]
    fn test_no_volume_control_layer_points_at_eqmac_unless_the_native_driver_is_offered() {
        let missing = |offered| Fixture {
            volume: Some(volume(false, false)),
            native_driver_offered: offered,
            ..Fixture::healthy()
        };
        let not_offered = missing(false);
        assert_eq!(not_offered.state("eqmac_readiness"), CheckState::Warn);
        let fix = not_offered.fix("eqmac_readiness");
        assert!(
            fix.contains("eqMac") && !fix.contains("rusty-jack install"),
            "{fix}"
        );

        let offered = missing(true).fix("eqmac_readiness");
        assert!(offered.contains("rusty-jack install"), "{offered}");
    }

    #[test]
    fn test_eqmac_check_is_skipped_unless_volume_control_is_needed() {
        let not_needed = Fixture {
            volume_needed: false,
            eqmac_running: false,
            ..Fixture::healthy()
        };
        assert_eq!(not_needed.state("eqmac_readiness"), CheckState::Skipped);
    }

    #[test]
    fn test_native_driver_makes_eqmac_irrelevant() {
        let f = Fixture {
            volume: Some(volume(true, true)),
            eqmac_running: false,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("eqmac_readiness"), CheckState::Ok);
    }

    #[test]
    fn test_routing_mismatch_warns_only_when_the_daemon_is_not_running() {
        let mismatch = Some(policy(Some(false)));
        let stuck = Fixture {
            policy: mismatch.clone(),
            daemon: Some(paused()),
            ..Fixture::healthy()
        };
        assert_eq!(stuck.state("routing"), CheckState::Warn);

        // With a running daemon the mismatch is a normal transient state.
        let transient = Fixture {
            policy: mismatch,
            ..Fixture::healthy()
        };
        assert_eq!(transient.state("routing"), CheckState::Ok);
    }

    #[test]
    fn test_unplugged_preferred_output_is_not_a_routing_failure() {
        // evaluate_policy reports `matches_preferred: Some(false)` when the preferred device
        // is unplugged, but no daemon can route to it, so it is neither ok nor a warning.
        let mut unplugged = policy(Some(false));
        unplugged.preferred_present = Some(false);
        for daemon in [running(), paused()] {
            let f = Fixture {
                policy: Some(unplugged.clone()),
                daemon: Some(daemon),
                ..Fixture::healthy()
            };
            assert_eq!(f.state("routing"), CheckState::Skipped);
            let checks = f.checks();
            let c = checks.iter().find(|c| c.id == "routing").unwrap();
            assert!(
                c.detail.contains("not connected") && c.detail.contains("Monitor"),
                "{}",
                c.detail
            );
        }
    }

    #[test]
    fn test_routing_mismatch_is_ignored_without_auto_switch() {
        let f = Fixture {
            policy: Some(policy(Some(false))),
            daemon: Some(paused()),
            auto_switch: false,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("routing"), CheckState::Skipped);
    }

    #[test]
    fn test_summarize_daemon_log_keeps_last_three_problems_and_newest_stamp() {
        let log = "\
20261122-10:00:00.000 | INFO     | daemon       | [daemon] started
20261122-10:00:01.000 | WARN     | daemon       | first problem
20261122-10:00:02.000 | ERROR    | daemon       | second problem
20261122-10:00:03.000 | WARN     | daemon       | third problem
20261122-10:00:04.000 | WARN     | daemon       | fourth problem
not a log line
20261122-10:00:05.000 | INFO     | daemon       | all quiet";
        let s = summarize_daemon_log(log);
        assert_eq!(s.last_entry_at.as_deref(), Some("20261122-10:00:05.000"));
        assert_eq!(s.recent_problems.len(), 3);
        assert!(s.recent_problems[0].contains("second problem"));
        assert!(s.recent_problems[2].contains("fourth problem"));
        assert_eq!(summarize_daemon_log(""), LogSummary::default());
    }

    #[test]
    fn test_an_old_warning_before_the_last_daemon_start_does_not_count() {
        let log = "\
20261120-09:00:00.000 | WARN     | daemon       | old trouble from an earlier process
20261122-10:00:00.000 | INFO     | daemon       | [daemon] started version=0.13.1 commit=x poll=2000ms activity_poll=1000ms activity_monitor=idle
20261122-10:00:05.000 | INFO     | daemon       | all quiet";
        let s = summarize_daemon_log(log);
        assert!(s.recent_problems.is_empty(), "{:?}", s.recent_problems);
        assert_eq!(s.last_entry_at.as_deref(), Some("20261122-10:00:05.000"));
        let f = Fixture {
            log: s,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_log"), CheckState::Ok);

        // A warning after the start still counts.
        let after = format!("{log}\n20261122-11:00:00.000 | WARN     | daemon       | new trouble");
        let f = Fixture {
            log: summarize_daemon_log(&after),
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_log"), CheckState::Warn);
    }

    #[test]
    fn test_without_a_start_line_in_the_tail_the_log_check_does_not_claim_since_start() {
        let log = "20261122-10:00:00.000 | WARN     | daemon       | trouble";
        let s = summarize_daemon_log(log);
        assert!(!s.since_daemon_start);
        let f = Fixture {
            log: s,
            ..Fixture::healthy()
        };
        let checks = f.checks();
        let c = checks.iter().find(|c| c.id == "daemon_log").unwrap();
        assert_eq!(c.state, CheckState::Warn);
        assert!(
            !c.detail.contains("since the last recorded daemon start"),
            "{}",
            c.detail
        );
        assert!(c.detail.contains("may predate it"), "{}", c.detail);

        // With the start line in the tail, the claim is accurate.
        let with_start = format!(
            "20261122-09:00:00.000 | INFO     | daemon       | [daemon] started version=0.13.1 commit=x poll=2000ms activity_poll=1000ms activity_monitor=idle\n{log}"
        );
        let s = summarize_daemon_log(&with_start);
        assert!(s.since_daemon_start);
        let f = Fixture {
            log: s,
            ..Fixture::healthy()
        };
        let checks = f.checks();
        let c = checks.iter().find(|c| c.id == "daemon_log").unwrap();
        assert!(
            c.detail.contains("since the last recorded daemon start"),
            "{}",
            c.detail
        );
    }

    #[test]
    fn test_log_check_reports_problems_and_a_quiet_log() {
        let quiet = Fixture {
            log: summarize_daemon_log("20261122-10:00:00.000 | INFO     | daemon       | hi"),
            ..Fixture::healthy()
        };
        assert_eq!(quiet.state("daemon_log"), CheckState::Ok);

        let noisy = Fixture {
            log: summarize_daemon_log("20261122-10:00:00.000 | WARN     | daemon       | trouble"),
            ..Fixture::healthy()
        };
        assert_eq!(noisy.state("daemon_log"), CheckState::Warn);
        assert_eq!(Fixture::healthy().state("daemon_log"), CheckState::Skipped);
    }

    #[test]
    fn test_unknown_daemon_state_warns_not_fails() {
        let f = Fixture {
            daemon: None,
            ..Fixture::healthy()
        };
        assert_eq!(f.state("daemon_state"), CheckState::Warn);
    }

    #[test]
    fn test_health_check_json_shape_matches_the_permission_checks() {
        let value = serde_json::to_value(
            Fixture {
                daemon: Some(paused()),
                ..Fixture::healthy()
            }
            .checks(),
        )
        .unwrap();
        let first = &value[0];
        assert_eq!(first["id"], "daemon_state");
        assert_eq!(first["state"], "fail");
        assert!(first["detail"].is_string() && first["fix"].is_string());
    }
}
