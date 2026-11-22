//! Privacy-permission diagnostics shared by `rusty-jack doctor` and `rusty-jack status`.
//!
//! macOS TCC grants are per binary and per process, so the foreground CLI and the
//! LaunchAgent daemon can disagree. The report therefore shows both: a live probe
//! for this CLI and the daemon's own verdict recovered from its log.

use crate::config::{Config, LoggingConfig};
use crate::privacy_permissions::{
    check_privacy_permissions, check_privacy_permissions_no_probe, PrivacyPermissionStatus,
};
use crate::RustyJackError;
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How much of the daemon log tail is scanned for the daemon's `[privacy]` verdict.
const LOG_TAIL_BYTES: u64 = 512 * 1024;

const ACCESSIBILITY_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const LOCAL_NETWORK_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_LocalNetwork";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    /// Permission is in place.
    Ok,
    /// Could not be confirmed; may or may not be a problem.
    Warn,
    /// Required by the current config and missing.
    Fail,
    /// Not needed by the current config.
    Skipped,
    /// Needed, but not probed in this mode (`status` does no SSDP scan).
    Unchecked,
}

impl CheckState {
    fn tag(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "FAIL",
            Self::Skipped => "skip",
            Self::Unchecked => "n/a",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    pub id: &'static str,
    pub label: &'static str,
    pub state: CheckState,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrivacyReport {
    pub checks: Vec<DoctorCheck>,
}

impl PrivacyReport {
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.state == CheckState::Fail)
    }

    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.checks.iter().any(|c| c.state == CheckState::Warn)
    }

    /// Render the report as an indented text block.
    #[must_use]
    pub fn format(&self, title: &str) -> String {
        let mut out = format!("{title}\n");
        for check in &self.checks {
            out.push_str(&format!(
                "  [{:<4}] {}: {}\n",
                check.state.tag(),
                check.label,
                check.detail
            ));
            if matches!(check.state, CheckState::Fail | CheckState::Warn) {
                if let Some(fix) = &check.fix {
                    for line in fix.lines() {
                        out.push_str(&format!("           → {line}\n"));
                    }
                }
            }
        }
        out.trim_end().to_string()
    }
}

/// The daemon's most recent Accessibility verdict, as logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonAccessibility {
    Granted {
        logged_at: String,
    },
    /// Granted since the daemon started, but the daemon chose its monitor at startup
    /// (without the grant), so it stays on the idle monitor until restarted.
    GrantedNeedsRestart {
        logged_at: String,
    },
    Missing {
        logged_at: String,
    },
    /// No `[privacy]`/`[activity]` Accessibility line found (daemon never ran, or log rotated).
    Unknown,
}

/// Find the daemon's current Accessibility state in a log excerpt.
///
/// The daemon picks event tap vs idle monitor once, at startup, but re-checks
/// Accessibility on every idle→active wake. A wake line saying "granted" that
/// follows a startup verdict of "missing" therefore means a restart is still needed.
#[must_use]
pub fn parse_daemon_accessibility(log: &str) -> DaemonAccessibility {
    // (granted, is_startup_decision, logged_at)
    let mut latest: Option<(bool, bool, String)> = None;
    let mut latest_startup: Option<(bool, String)> = None;
    for line in log.lines() {
        let granted =
            line.contains("[privacy]") && line.contains("Accessibility permission looks granted");
        let activity_missing = line.contains("[activity] Accessibility not granted for daemon");
        let missing = activity_missing
            || (line.contains("[privacy]") && line.contains("Accessibility permission missing"));
        if !(granted || missing) {
            continue;
        }
        let logged_at = line.split(" | ").next().unwrap_or("").trim().to_string();
        let is_startup = activity_missing || line.contains("[privacy] startup:");
        if is_startup {
            latest_startup = Some((granted, logged_at.clone()));
        }
        latest = Some((granted, is_startup, logged_at));
    }
    match latest {
        None => DaemonAccessibility::Unknown,
        Some((false, _, logged_at)) => DaemonAccessibility::Missing { logged_at },
        Some((true, is_startup, logged_at)) => {
            if !is_startup && matches!(latest_startup, Some((false, _))) {
                DaemonAccessibility::GrantedNeedsRestart { logged_at }
            } else {
                DaemonAccessibility::Granted { logged_at }
            }
        }
    }
}

/// Read the last [`LOG_TAIL_BYTES`] of the daemon log (empty when unreadable).
#[must_use]
pub fn read_log_tail(path: &Path) -> String {
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    let truncated = len > LOG_TAIL_BYTES;
    if truncated && file.seek(SeekFrom::Start(len - LOG_TAIL_BYTES)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        // The seek likely landed mid-line; drop that partial first line.
        return text
            .split_once('\n')
            .map_or_else(String::new, |(_, rest)| rest.to_string());
    }
    text
}

/// The binary to grant: the one the LaunchAgent runs, else this executable.
fn current_binary_hint() -> String {
    if let Some(path) = crate::launchd::installed_daemon_binary_path() {
        return path;
    }
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_or_else(
            |_| "the rusty-jack binary".to_string(),
            |p| p.display().to_string(),
        )
}

/// Build checks from an already-probed permission snapshot (pure; unit-tested).
#[must_use]
pub fn privacy_checks(
    status: &PrivacyPermissionStatus,
    daemon: &DaemonAccessibility,
    binary: &str,
    local_network_probed: bool,
) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();

    let ax_fix = format!(
        "System Settings → Privacy & Security → Accessibility: enable {binary}\n\
         open \"{ACCESSIBILITY_PANE_URL}\"\n\
         then restart the daemon: rusty-jack upgrade --force"
    );

    let ax_fix_short = format!(
        "System Settings → Privacy & Security → Accessibility: enable {binary}\n\
         open \"{ACCESSIBILITY_PANE_URL}\""
    );

    // Accessibility, as seen by this CLI process.
    checks.push(if !status.accessibility_required {
        DoctorCheck {
            id: "accessibility_cli",
            label: "Accessibility (this CLI)",
            state: CheckState::Skipped,
            detail: "not required (activity_monitor is not event_tap)".into(),
            fix: None,
        }
    } else if status.accessibility_trusted == Some(true) {
        DoctorCheck {
            id: "accessibility_cli",
            label: "Accessibility (this CLI)",
            state: CheckState::Ok,
            detail: "granted".into(),
            fix: None,
        }
    } else {
        // From a terminal, macOS evaluates the terminal app's grant, not this binary's,
        // so this can differ from the daemon. Informational only; the daemon check decides.
        DoctorCheck {
            id: "accessibility_cli",
            label: "Accessibility (this CLI)",
            state: CheckState::Warn,
            detail: "not granted to this process or its terminal app (informational; the daemon check below is what matters)".into(),
            fix: Some(ax_fix.clone()),
        }
    });

    // Accessibility, as seen by the LaunchAgent daemon (what actually matters).
    checks.push(if !status.accessibility_required {
        DoctorCheck {
            id: "accessibility_daemon",
            label: "Accessibility (daemon)",
            state: CheckState::Skipped,
            detail: "not required (activity_monitor is not event_tap)".into(),
            fix: None,
        }
    } else {
        match daemon {
            DaemonAccessibility::Granted { logged_at } => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Ok,
                detail: format!("granted (daemon log {logged_at})"),
                fix: None,
            },
            DaemonAccessibility::GrantedNeedsRestart { logged_at } => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Warn,
                detail: format!(
                    "granted since the daemon started ({logged_at}), but it is still on the idle monitor until restarted"
                ),
                fix: Some("restart the daemon: rusty-jack upgrade --force".into()),
            },
            DaemonAccessibility::Missing { logged_at } => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Fail,
                detail: format!(
                    "missing (daemon log {logged_at}); keyboard/mouse wake falls back to the idle monitor"
                ),
                fix: Some(ax_fix),
            },
            DaemonAccessibility::Unknown => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Warn,
                detail: "no daemon verdict in the log yet".into(),
                fix: Some(format!(
                    "start the daemon (rusty-jack install / upgrade --force) and re-run doctor; \
                     it logs `[privacy] startup` on launch\n\
                     if Accessibility is not yet granted: {ax_fix_short}"
                )),
            },
        }
    });

    // Local Network (SSDP multicast), only when ScalarWebAPI is enabled.
    checks.push(if !status.local_network_required {
        DoctorCheck {
            id: "local_network",
            label: "Local Network",
            state: CheckState::Skipped,
            detail: "not required (ScalarWebAPI speaker not enabled)".into(),
            fix: None,
        }
    } else if !local_network_probed {
        DoctorCheck {
            id: "local_network",
            label: "Local Network",
            state: CheckState::Unchecked,
            detail: "not probed by status (no SSDP scan); run `rusty-jack doctor` to probe".into(),
            fix: None,
        }
    } else if status.local_network_ok == Some(true) {
        DoctorCheck {
            id: "local_network",
            label: "Local Network",
            state: CheckState::Ok,
            detail: "SSDP probe heard a speaker".into(),
            fix: None,
        }
    } else {
        // Empty SSDP cannot distinguish a denial from an offline speaker, so never FAIL.
        DoctorCheck {
            id: "local_network",
            label: "Local Network",
            state: CheckState::Warn,
            detail: "could not be confirmed (no LAN route, or no speaker answered SSDP)".into(),
            fix: Some(format!(
                "if the speaker is on and discovery stays empty, enable rusty-jack under \
                 System Settings → Privacy & Security → Local Network\n\
                 open \"{LOCAL_NETWORK_PANE_URL}\"\n\
                 then: rusty-jack upgrade --force"
            )),
        }
    });

    checks
}

/// Where the daemon writes its log: `config.logging.file` (or the default), honoring
/// the log-file environment override, exactly as the daemon resolves it.
#[must_use]
pub fn daemon_log_path(config: Option<&Config>) -> Option<PathBuf> {
    let file = config.map_or_else(|| LoggingConfig::default().file, |c| c.logging.file.clone());
    crate::logging::resolve_log_file_path(Path::new(&file)).ok()
}

/// Probe permissions for `config` and read the daemon's verdict from its log.
///
/// `probe_local_network` runs the SSDP probe (`doctor`); `status` passes `false`
/// so it stays free of LAN scans.
///
/// # Errors
///
/// Returns an error if the permission probe itself fails.
pub fn build_privacy_report(
    config: Option<&Config>,
    probe_local_network: bool,
) -> Result<PrivacyReport, RustyJackError> {
    let status = match config {
        Some(config) if probe_local_network => check_privacy_permissions(config)?,
        Some(config) => check_privacy_permissions_no_probe(config)?,
        None => PrivacyPermissionStatus {
            accessibility_required: false,
            accessibility_trusted: None,
            force_daemon_restart: false,
            local_network_ok: None,
            local_network_required: false,
            notes: vec!["no config loaded".into()],
        },
    };
    let daemon = daemon_log_path(config)
        .map(|path| parse_daemon_accessibility(&read_log_tail(&path)))
        .unwrap_or(DaemonAccessibility::Unknown);
    Ok(PrivacyReport {
        checks: privacy_checks(
            &status,
            &daemon,
            &current_binary_hint(),
            probe_local_network,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(
        ax_req: bool,
        ax: Option<bool>,
        ln_req: bool,
        ln: Option<bool>,
    ) -> PrivacyPermissionStatus {
        PrivacyPermissionStatus {
            accessibility_required: ax_req,
            accessibility_trusted: ax,
            force_daemon_restart: false,
            local_network_ok: ln,
            local_network_required: ln_req,
            notes: vec![],
        }
    }

    fn state_of(checks: &[DoctorCheck], id: &str) -> CheckState {
        checks.iter().find(|c| c.id == id).unwrap().state
    }

    #[test]
    fn test_nothing_required_is_all_skipped() {
        let checks = privacy_checks(
            &status(false, None, false, None),
            &DaemonAccessibility::Unknown,
            "bin",
            true,
        );
        assert!(checks.iter().all(|c| c.state == CheckState::Skipped));
    }

    #[test]
    fn test_cli_ok_but_daemon_missing_fails_daemon_only() {
        let daemon = DaemonAccessibility::Missing {
            logged_at: "20261122-10:11:53.037".into(),
        };
        let checks = privacy_checks(&status(true, Some(true), false, None), &daemon, "bin", true);
        assert_eq!(state_of(&checks, "accessibility_cli"), CheckState::Ok);
        assert_eq!(state_of(&checks, "accessibility_daemon"), CheckState::Fail);
        let report = PrivacyReport { checks };
        assert!(report.has_failures());
        assert!(report.format("Privacy").contains("→ System Settings"));
    }

    #[test]
    fn test_unknown_daemon_verdict_warns() {
        let checks = privacy_checks(
            &status(true, Some(false), false, None),
            &DaemonAccessibility::Unknown,
            "bin",
            true,
        );
        assert_eq!(state_of(&checks, "accessibility_cli"), CheckState::Warn);
        assert_eq!(state_of(&checks, "accessibility_daemon"), CheckState::Warn);
        assert!(!PrivacyReport { checks }.has_failures());
    }

    #[test]
    fn test_local_network_never_fails_when_inconclusive() {
        let checks = privacy_checks(
            &status(false, None, true, None),
            &DaemonAccessibility::Unknown,
            "bin",
            true,
        );
        assert_eq!(state_of(&checks, "local_network"), CheckState::Warn);
        let ok = privacy_checks(
            &status(false, None, true, Some(true)),
            &DaemonAccessibility::Unknown,
            "bin",
            true,
        );
        assert_eq!(state_of(&ok, "local_network"), CheckState::Ok);
    }

    #[test]
    fn test_parse_daemon_accessibility_uses_latest_line() {
        let log = "\
20261122-09:00:00.000 | INFO     | daemon       | [privacy] startup: Accessibility permission looks granted
20261122-10:11:53.037 | WARN     | daemon       | [privacy] idle→active wake: Accessibility permission missing (required for activity_monitor=event_tap)
20261122-10:11:53.038 | INFO     | daemon       | [privacy] idle→active wake: Local Network not confirmed";
        assert_eq!(
            parse_daemon_accessibility(log),
            DaemonAccessibility::Missing {
                logged_at: "20261122-10:11:53.037".into()
            }
        );
        assert_eq!(
            parse_daemon_accessibility("nothing here"),
            DaemonAccessibility::Unknown
        );
    }

    #[test]
    fn test_wake_granted_after_startup_missing_needs_restart() {
        let log = "\
20261122-10:45:14.825 | WARN     | daemon       | [activity] Accessibility not granted for daemon; using idle monitor instead of event tap
20261122-10:45:16.336 | WARN     | daemon       | [privacy] startup: Accessibility permission missing (required for activity_monitor=event_tap)
20261122-11:30:00.000 | INFO     | daemon       | [privacy] idle→active wake: Accessibility permission looks granted";
        let parsed = parse_daemon_accessibility(log);
        assert_eq!(
            parsed,
            DaemonAccessibility::GrantedNeedsRestart {
                logged_at: "20261122-11:30:00.000".into()
            }
        );
        let checks = privacy_checks(&status(true, Some(true), false, None), &parsed, "bin", true);
        assert_eq!(state_of(&checks, "accessibility_daemon"), CheckState::Warn);
        assert!(checks
            .iter()
            .find(|c| c.id == "accessibility_daemon")
            .unwrap()
            .fix
            .as_deref()
            .unwrap()
            .contains("upgrade --force"));
    }

    #[test]
    fn test_startup_granted_is_ok_even_after_earlier_missing() {
        let log = "\
20261122-10:00:00.000 | WARN     | daemon       | [privacy] startup: Accessibility permission missing
20261122-11:45:00.000 | INFO     | daemon       | [privacy] startup: Accessibility permission looks granted
20261122-12:00:00.000 | INFO     | daemon       | [privacy] idle→active wake: Accessibility permission looks granted";
        assert!(matches!(
            parse_daemon_accessibility(log),
            DaemonAccessibility::Granted { .. }
        ));
    }

    #[test]
    fn test_daemon_log_path_follows_config_logging_file() {
        let mut config = Config::default();
        config.logging.file = "/var/tmp/custom-rusty-jack.log".into();
        // The env override wins over config; only assert when it is unset.
        if std::env::var_os("RUSTY_JACK_LOG_FILE").is_none() {
            assert_eq!(
                daemon_log_path(Some(&config)),
                Some(PathBuf::from("/var/tmp/custom-rusty-jack.log"))
            );
        }
    }

    #[test]
    fn test_unknown_daemon_fix_includes_settings_step() {
        let checks = privacy_checks(
            &status(true, Some(true), false, None),
            &DaemonAccessibility::Unknown,
            "bin",
            true,
        );
        let fix = checks
            .iter()
            .find(|c| c.id == "accessibility_daemon")
            .unwrap()
            .fix
            .clone()
            .unwrap();
        assert!(fix.contains("x-apple.systempreferences"));
    }

    #[test]
    fn test_parse_daemon_accessibility_recognizes_activity_fallback_line() {
        let log = "20261122-08:00:00.000 | WARN     | daemon       | [activity] Accessibility not granted for daemon; using idle monitor";
        assert!(matches!(
            parse_daemon_accessibility(log),
            DaemonAccessibility::Missing { .. }
        ));
    }

    #[test]
    fn test_local_network_unchecked_when_not_probed() {
        let checks = privacy_checks(
            &status(false, None, true, None),
            &DaemonAccessibility::Unknown,
            "bin",
            false,
        );
        assert_eq!(state_of(&checks, "local_network"), CheckState::Unchecked);
        assert!(!PrivacyReport { checks }.has_warnings());
    }

    #[test]
    fn test_read_log_tail_reads_only_tail_and_drops_partial_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rusty-jack.log");
        // Old verdict first, then filler past the tail window, then a newer verdict.
        let old = "20200101-00:00:00.000 | INFO | daemon | [privacy] startup: Accessibility permission looks granted\n";
        let filler_line = "20260101-00:00:00.000 | INFO | daemon | filler line for padding\n";
        let mut content = String::from(old);
        while (content.len() as u64) < LOG_TAIL_BYTES + 4096 {
            content.push_str(filler_line);
        }
        content.push_str("20261122-10:11:53.037 | WARN | daemon | [privacy] wake: Accessibility permission missing\n");
        std::fs::write(&path, &content).unwrap();

        let tail = read_log_tail(&path);
        assert!(tail.len() as u64 <= LOG_TAIL_BYTES);
        assert!(!tail.contains("20200101"), "old head must be excluded");
        assert!(
            tail.starts_with("20"),
            "partial first line must be dropped, got: {:?}",
            &tail[..tail.len().min(40)]
        );
        assert!(matches!(
            parse_daemon_accessibility(&tail),
            DaemonAccessibility::Missing { .. }
        ));
    }

    #[test]
    fn test_read_log_tail_handles_missing_file() {
        assert_eq!(read_log_tail(Path::new("/nonexistent/rusty-jack.log")), "");
    }
}
