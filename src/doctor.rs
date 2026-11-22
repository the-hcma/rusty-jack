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
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";
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

    /// One-line conclusion for `doctor`, given whether a config was loaded.
    #[must_use]
    pub fn conclusion(&self, config_loaded: bool) -> &'static str {
        if self.has_failures() {
            "Fix the FAIL items above, then re-run `rusty-jack doctor`."
        } else if self.has_warnings() {
            "No required permission is confirmed missing; see warnings above."
        } else if !config_loaded {
            "Nothing was checked: no config is loaded, so no permissions are known to be required."
        } else if self.checks.iter().all(|c| c.state == CheckState::Skipped) {
            "This config does not require any privacy permissions."
        } else {
            "All required permissions are in place."
        }
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
    /// Accessibility is granted but the event tap failed to start, so the daemon is on the
    /// idle monitor.
    TapUnavailable {
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
/// A granted verdict is also not "ok" when the tap itself failed to start.
#[must_use]
pub fn parse_daemon_accessibility(log: &str) -> DaemonAccessibility {
    // (granted, is_startup_decision, logged_at)
    let mut latest: Option<(bool, bool, String)> = None;
    let mut latest_startup: Option<(bool, String)> = None;
    // Set by "event tap unavailable", cleared by "event tap active" (file order).
    let mut tap_unavailable: Option<String> = None;
    // The `[activity]` line is what actually picks the monitor for a launch. While a launch
    // that chose the idle monitor is current, a later startup "granted" line (a grant that
    // landed during the startup probes) must not be read as a clean startup.
    let mut launch_denied = false;
    for line in log.lines() {
        let logged_at = || line.split(" | ").next().unwrap_or("").trim().to_string();
        if line.contains("[activity] event tap unavailable") {
            tap_unavailable = Some(logged_at());
            launch_denied = false;
            continue;
        }
        if line.contains("[activity] event tap active") {
            tap_unavailable = None;
            launch_denied = false;
            continue;
        }
        let granted =
            line.contains("[privacy]") && line.contains("Accessibility permission looks granted");
        let activity_missing = line.contains("[activity] Accessibility not granted for daemon");
        let missing = activity_missing
            || (line.contains("[privacy]") && line.contains("Accessibility permission missing"));
        if !(granted || missing) {
            continue;
        }
        let logged_at = logged_at();
        if activity_missing {
            launch_denied = true;
        }
        let is_startup = activity_missing
            || (line.contains("[privacy] startup:") && !(granted && launch_denied));
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
            } else if let Some(tap_at) = tap_unavailable {
                DaemonAccessibility::TapUnavailable { logged_at: tap_at }
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

/// Environment facts that shape the wording and severity of the checks.
#[derive(Debug, Clone)]
pub struct CheckContext {
    /// Binary the LaunchAgent runs (what macOS attributes the daemon's grant to).
    pub binary: String,
    /// Terminal app this CLI runs in (what macOS attributes the CLI's grant to).
    pub terminal: String,
    /// Whether the Local Network SSDP probe ran (`doctor`) or not (`status`).
    pub local_network_probed: bool,
    /// Whether the daemon's `RUST_LOG` (from its LaunchAgent plist) records "granted" lines.
    /// When false, a stale "missing" line cannot be told apart from the current state.
    pub log_confirms_grants: bool,
}

/// Whether the daemon's `RUST_LOG` lets its info-level `[privacy]` lines reach the log.
///
/// The installed LaunchAgent pins `RUST_LOG=info`. Without it the daemon's filter is
/// `rusty_jack=<level>`, which drops the `daemon` log target entirely, and a directive
/// string (`info,daemon=warn`, `rusty_jack=debug`) can filter it per target. Only a plain
/// `trace`/`debug`/`info` is known to record the verdict; anything else is "can't tell".
#[must_use]
pub fn log_level_confirms_grants(rust_log: Option<&str>) -> bool {
    matches!(
        rust_log.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("trace" | "debug" | "info")
    )
}

fn daemon_log_confirms_grants() -> bool {
    log_level_confirms_grants(crate::launchd::installed_daemon_env_var("RUST_LOG").as_deref())
}

fn current_terminal_hint() -> String {
    match std::env::var("TERM_PROGRAM") {
        Ok(name) if !name.trim().is_empty() => match name.as_str() {
            "Apple_Terminal" => "Terminal".to_string(),
            "iTerm.app" => "iTerm".to_string(),
            "vscode" => "Visual Studio Code".to_string(),
            other => other.to_string(),
        },
        _ => "the terminal app you run this from".to_string(),
    }
}

/// Build checks from an already-probed permission snapshot (pure; unit-tested).
#[must_use]
pub fn privacy_checks(
    status: &PrivacyPermissionStatus,
    daemon: &DaemonAccessibility,
    ctx: &CheckContext,
) -> Vec<DoctorCheck> {
    let binary = ctx.binary.as_str();
    let terminal = ctx.terminal.as_str();
    let local_network_probed = ctx.local_network_probed;
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
            fix: Some(format!(
                "only if you want the CLI itself trusted: enable {} under System Settings → Privacy & Security → Accessibility\n\
                 open \"{ACCESSIBILITY_PANE_URL}\"\n\
                 this does not change the daemon's grant; see the daemon check below",
                ctx.terminal
            )),
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
            DaemonAccessibility::TapUnavailable { logged_at } => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Warn,
                detail: format!(
                    "granted, but the event tap failed to start ({logged_at}); the daemon is on the idle monitor"
                ),
                fix: Some(
                    "see `[activity] event tap unavailable` in the daemon log for the reason, then: rusty-jack upgrade --force"
                        .into(),
                ),
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
            DaemonAccessibility::Missing { logged_at } if ctx.log_confirms_grants => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Fail,
                detail: format!(
                    "missing (daemon log {logged_at}); keyboard/mouse wake falls back to the idle monitor"
                ),
                fix: Some(ax_fix),
            },
            DaemonAccessibility::Missing { logged_at } => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Warn,
                detail: format!(
                    "last logged as missing ({logged_at}); the daemon's log level hides \"granted\" lines, so this may be stale"
                ),
                fix: Some(format!(
                    "rusty-jack upgrade --force rewrites the LaunchAgent with RUST_LOG=info; then re-run doctor\n{ax_fix}"
                )),
            },
            DaemonAccessibility::Unknown => DoctorCheck {
                id: "accessibility_daemon",
                label: "Accessibility (daemon)",
                state: CheckState::Warn,
                detail: if ctx.log_confirms_grants {
                    "no daemon verdict in the log (the daemon logs one at startup and on wake; an old one may be beyond the log's recent tail or rotated out)".into()
                } else {
                    "no daemon verdict in the log (the daemon's RUST_LOG does not record \"granted\" lines; rusty-jack upgrade --force sets RUST_LOG=info)".into()
                },
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
            label: "Local Network (this CLI)",
            state: CheckState::Skipped,
            detail: "not required (ScalarWebAPI speaker not enabled)".into(),
            fix: None,
        }
    } else if !local_network_probed {
        DoctorCheck {
            id: "local_network",
            label: "Local Network (this CLI)",
            state: CheckState::Unchecked,
            detail: "not probed by status (no SSDP scan); run `rusty-jack doctor` to probe".into(),
            fix: None,
        }
    } else if status.local_network_ok == Some(true) {
        DoctorCheck {
            id: "local_network",
            label: "Local Network (this CLI)",
            state: CheckState::Ok,
            detail: "SSDP probe from this CLI heard a speaker (the daemon's own grant is separate)"
                .into(),
            fix: None,
        }
    } else {
        // Empty SSDP cannot distinguish a denial from an offline speaker, so never FAIL.
        DoctorCheck {
            id: "local_network",
            label: "Local Network (this CLI)",
            state: CheckState::Warn,
            detail: "could not be confirmed (no LAN route, or no speaker answered SSDP)".into(),
            fix: Some(format!(
                "if the speaker is on and discovery stays empty, enable {terminal} (this CLI) and \
                 {binary} (the daemon) under System Settings → Privacy & Security → Local Network\n\
                 open \"{LOCAL_NETWORK_PANE_URL}\"\n\
                 then: rusty-jack upgrade --force"
            )),
        }
    });

    checks
}

/// Where the daemon would write its log given this CLI's config and environment:
/// `config.logging.file` (or the default), with the `RUSTY_JACK_LOG_FILE` override.
/// The daemon takes both from its LaunchAgent, so they must match this CLI's `--config`
/// and environment for the result to be the daemon's actual log.
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
            &CheckContext {
                binary: current_binary_hint(),
                terminal: current_terminal_hint(),
                local_network_probed: probe_local_network,
                log_confirms_grants: daemon_log_confirms_grants(),
            },
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

    fn ctx(probed: bool) -> CheckContext {
        CheckContext {
            binary: "/sentinel/daemon-bin-xyz".into(),
            terminal: "Terminal".into(),
            local_network_probed: probed,
            log_confirms_grants: true,
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
            &ctx(true),
        );
        assert!(checks.iter().all(|c| c.state == CheckState::Skipped));
    }

    #[test]
    fn test_cli_ok_but_daemon_missing_fails_daemon_only() {
        let daemon = DaemonAccessibility::Missing {
            logged_at: "20261122-10:11:53.037".into(),
        };
        let checks = privacy_checks(&status(true, Some(true), false, None), &daemon, &ctx(true));
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
            &ctx(true),
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
            &ctx(true),
        );
        assert_eq!(state_of(&checks, "local_network"), CheckState::Warn);
        let ok = privacy_checks(
            &status(false, None, true, Some(true)),
            &DaemonAccessibility::Unknown,
            &ctx(true),
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
        let checks = privacy_checks(&status(true, Some(true), false, None), &parsed, &ctx(true));
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
            &ctx(true),
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
            &ctx(false),
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
    fn test_missing_downgrades_to_warn_when_log_level_hides_granted() {
        let daemon = DaemonAccessibility::Missing {
            logged_at: "t".into(),
        };
        let quiet = CheckContext {
            log_confirms_grants: false,
            ..ctx(true)
        };
        let checks = privacy_checks(&status(true, Some(true), false, None), &daemon, &quiet);
        assert_eq!(state_of(&checks, "accessibility_daemon"), CheckState::Warn);
        assert!(!PrivacyReport { checks }.has_failures());

        let checks = privacy_checks(&status(true, Some(true), false, None), &daemon, &ctx(true));
        assert_eq!(state_of(&checks, "accessibility_daemon"), CheckState::Fail);
    }

    #[test]
    fn test_log_level_confirms_grants_only_for_plain_levels_from_rust_log() {
        for ok in ["trace", "debug", "info", " INFO "] {
            assert!(log_level_confirms_grants(Some(ok)), "{ok}");
        }
        // Unset RUST_LOG drops the `daemon` target; directives can filter it; quiet levels hide it.
        for bad in [
            None,
            Some(""),
            Some("warn"),
            Some("error"),
            Some("off"),
            Some("rusty_jack=debug"),
            Some("info,daemon=warn"),
        ] {
            assert!(!log_level_confirms_grants(bad), "{bad:?}");
        }
    }

    #[test]
    fn test_startup_granted_after_activity_denied_needs_restart() {
        // The grant landed in the ~1.5s between the monitor choice and the privacy probe.
        let log = "\
20261122-10:45:14.825 | WARN     | daemon       | [activity] Accessibility not granted for daemon; using idle monitor instead of event tap
20261122-10:45:16.336 | INFO     | daemon       | [privacy] startup: Accessibility permission looks granted";
        assert!(matches!(
            parse_daemon_accessibility(log),
            DaemonAccessibility::GrantedNeedsRestart { .. }
        ));
        // A fresh launch that starts the tap is clean again.
        let relaunched = format!(
            "{log}\n\
20261122-10:54:17.521 | INFO     | daemon       | [activity] event tap active (listen-only)\n\
20261122-10:54:19.051 | INFO     | daemon       | [privacy] startup: Accessibility permission looks granted"
        );
        assert!(matches!(
            parse_daemon_accessibility(&relaunched),
            DaemonAccessibility::Granted { .. }
        ));
    }

    fn report_of(states: &[CheckState]) -> PrivacyReport {
        PrivacyReport {
            checks: states
                .iter()
                .map(|&state| DoctorCheck {
                    id: "x",
                    label: "x",
                    state,
                    detail: String::new(),
                    fix: None,
                })
                .collect(),
        }
    }

    #[test]
    fn test_only_fail_makes_doctor_fail() {
        use CheckState::{Fail, Ok, Skipped, Unchecked, Warn};
        assert!(report_of(&[Ok, Fail]).has_failures());
        assert!(!report_of(&[Ok, Warn, Skipped, Unchecked]).has_failures());
    }

    #[test]
    fn test_conclusion_messages() {
        use CheckState::{Fail, Ok, Skipped, Warn};
        assert!(report_of(&[Fail])
            .conclusion(true)
            .starts_with("Fix the FAIL"));
        assert!(report_of(&[Warn]).conclusion(true).contains("see warnings"));
        assert!(report_of(&[Skipped])
            .conclusion(false)
            .starts_with("Nothing was checked"));
        assert!(report_of(&[Skipped])
            .conclusion(true)
            .contains("does not require"));
        assert_eq!(
            report_of(&[Ok, Skipped]).conclusion(true),
            "All required permissions are in place."
        );
    }

    #[test]
    fn test_check_state_json_values() {
        let value = serde_json::to_value(report_of(&[
            CheckState::Ok,
            CheckState::Warn,
            CheckState::Fail,
            CheckState::Skipped,
            CheckState::Unchecked,
        ]))
        .unwrap();
        let states: Vec<&str> = value["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["state"].as_str().unwrap())
            .collect();
        assert_eq!(states, ["ok", "warn", "fail", "skipped", "unchecked"]);
    }

    #[test]
    fn test_tap_unavailable_after_granted_startup_warns() {
        let log = "\
20261122-10:54:16.000 | WARN     | daemon       | [activity] event tap unavailable (CGEventTapCreate failed); falling back to idle monitor
20261122-10:54:19.051 | INFO     | daemon       | [privacy] startup: Accessibility permission looks granted";
        let parsed = parse_daemon_accessibility(log);
        assert!(matches!(parsed, DaemonAccessibility::TapUnavailable { .. }));
        let checks = privacy_checks(&status(true, Some(true), false, None), &parsed, &ctx(true));
        assert_eq!(state_of(&checks, "accessibility_daemon"), CheckState::Warn);

        // A later successful tap clears it.
        let recovered = format!(
            "{log}\n20261122-11:00:00.000 | INFO     | daemon       | [activity] event tap active (listen-only)"
        );
        assert!(matches!(
            parse_daemon_accessibility(&recovered),
            DaemonAccessibility::Granted { .. }
        ));
    }

    #[test]
    fn test_cli_fix_names_terminal_not_daemon_binary() {
        let checks = privacy_checks(
            &status(true, Some(false), false, None),
            &DaemonAccessibility::Unknown,
            &ctx(true),
        );
        let fix = checks
            .iter()
            .find(|c| c.id == "accessibility_cli")
            .unwrap()
            .fix
            .clone()
            .unwrap();
        assert!(fix.contains("Terminal"));
        assert!(
            !fix.contains("bin"),
            "CLI fix must not point at the daemon binary: {fix}"
        );
    }

    #[test]
    fn test_build_privacy_report_without_config_checks_nothing() {
        let report = build_privacy_report(None, true).unwrap();
        assert!(report.checks.iter().all(|c| c.state == CheckState::Skipped));
        assert!(!report.has_failures());
    }

    #[test]
    fn test_build_privacy_report_without_probe_leaves_local_network_unchecked() {
        let config = Config {
            scalar_webapi_device: Some(crate::config::ScalarWebApiDeviceConfig {
                enabled: true,
                model: "SRS-ZR5".into(),
                host: Some("192.0.2.1".into()),
                port: 54_480,
                path: "sony/av".into(),
                mac_output: crate::config::DeviceSelectorConfig {
                    name: None,
                    uid: Some("uid".into()),
                },
                triggers: vec!["output_selected".into()],
                wake_debounce_ms: 5_000,
                request_timeout_ms: 3_000,
                require_quick_start: true,
                speaker_input: None,
            }),
            ..Config::default()
        };
        let report = build_privacy_report(Some(&config), false).unwrap();
        let ln = report
            .checks
            .iter()
            .find(|c| c.id == "local_network")
            .unwrap();
        assert_eq!(ln.state, CheckState::Unchecked);
    }

    #[test]
    fn test_read_log_tail_handles_missing_file() {
        assert_eq!(read_log_tail(Path::new("/nonexistent/rusty-jack.log")), "");
    }
}
