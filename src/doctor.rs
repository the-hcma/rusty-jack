//! Privacy-permission diagnostics shared by `rusty-jack doctor` and `rusty-jack status`.
//!
//! macOS TCC grants are per binary and per process, so the foreground CLI and the
//! LaunchAgent daemon can disagree. The report therefore shows both: a live probe
//! for this CLI and the daemon's own verdict recovered from its log.

use crate::config::Config;
use crate::privacy_permissions::{check_privacy_permissions, PrivacyPermissionStatus};
use crate::RustyJackError;
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

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
}

impl CheckState {
    fn tag(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "FAIL",
            Self::Skipped => "skip",
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
    Missing {
        logged_at: String,
    },
    /// No `[privacy]`/`[activity]` Accessibility line found (daemon never ran, or log rotated).
    Unknown,
}

/// Find the daemon's latest Accessibility verdict in a log excerpt.
#[must_use]
pub fn parse_daemon_accessibility(log: &str) -> DaemonAccessibility {
    for line in log.lines().rev() {
        let granted =
            line.contains("[privacy]") && line.contains("Accessibility permission looks granted");
        let missing = (line.contains("[privacy]")
            && line.contains("Accessibility permission missing"))
            || line.contains("[activity] Accessibility not granted for daemon");
        if !(granted || missing) {
            continue;
        }
        let logged_at = line.split(" | ").next().unwrap_or("").trim().to_string();
        return if granted {
            DaemonAccessibility::Granted { logged_at }
        } else {
            DaemonAccessibility::Missing { logged_at }
        };
    }
    DaemonAccessibility::Unknown
}

/// Read the last [`LOG_TAIL_BYTES`] of the daemon log (empty when unreadable).
#[must_use]
pub fn read_log_tail(path: &Path) -> String {
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len > LOG_TAIL_BYTES && file.seek(SeekFrom::Start(len - LOG_TAIL_BYTES)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn current_binary_hint() -> String {
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
) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();

    let ax_fix = format!(
        "System Settings → Privacy & Security → Accessibility: enable {binary}\n\
         open \"{ACCESSIBILITY_PANE_URL}\"\n\
         then restart the daemon: rusty-jack upgrade --force"
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
        DoctorCheck {
            id: "accessibility_cli",
            label: "Accessibility (this CLI)",
            state: CheckState::Fail,
            detail: "missing (required for activity_monitor=event_tap)".into(),
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
                fix: Some(
                    "start the daemon (rusty-jack install / upgrade --force) and re-run doctor; \
                     it logs `[privacy] startup` on launch"
                        .into(),
                ),
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

/// Probe permissions for `config` and read the daemon's verdict from its log.
///
/// # Errors
///
/// Returns an error if the permission probe itself fails.
pub fn build_privacy_report(
    config: Option<&Config>,
    daemon_log: Option<&Path>,
) -> Result<PrivacyReport, RustyJackError> {
    let status = match config {
        Some(config) => check_privacy_permissions(config)?,
        None => PrivacyPermissionStatus {
            accessibility_required: false,
            accessibility_trusted: None,
            force_daemon_restart: false,
            local_network_ok: None,
            local_network_required: false,
            notes: vec!["no config loaded".into()],
        },
    };
    let daemon = daemon_log
        .map(|path| parse_daemon_accessibility(&read_log_tail(path)))
        .unwrap_or(DaemonAccessibility::Unknown);
    Ok(PrivacyReport {
        checks: privacy_checks(&status, &daemon, &current_binary_hint()),
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
        );
        assert!(checks.iter().all(|c| c.state == CheckState::Skipped));
    }

    #[test]
    fn test_cli_ok_but_daemon_missing_fails_daemon_only() {
        let daemon = DaemonAccessibility::Missing {
            logged_at: "20261122-10:11:53.037".into(),
        };
        let checks = privacy_checks(&status(true, Some(true), false, None), &daemon, "bin");
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
        );
        assert_eq!(state_of(&checks, "accessibility_cli"), CheckState::Fail);
        assert_eq!(state_of(&checks, "accessibility_daemon"), CheckState::Warn);
    }

    #[test]
    fn test_local_network_never_fails_when_inconclusive() {
        let checks = privacy_checks(
            &status(false, None, true, None),
            &DaemonAccessibility::Unknown,
            "bin",
        );
        assert_eq!(state_of(&checks, "local_network"), CheckState::Warn);
        let ok = privacy_checks(
            &status(false, None, true, Some(true)),
            &DaemonAccessibility::Unknown,
            "bin",
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
    fn test_parse_daemon_accessibility_recognizes_activity_fallback_line() {
        let log = "20261122-08:00:00.000 | WARN     | daemon       | [activity] Accessibility not granted for daemon; using idle monitor";
        assert!(matches!(
            parse_daemon_accessibility(log),
            DaemonAccessibility::Missing { .. }
        ));
    }

    #[test]
    fn test_read_log_tail_handles_missing_file() {
        assert_eq!(read_log_tail(Path::new("/nonexistent/rusty-jack.log")), "");
    }
}
