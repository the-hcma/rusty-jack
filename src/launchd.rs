//! launchd LaunchAgent management (macOS).

use crate::RustyJackError;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// launchd job label (matches `launchd/com.example.rusty-jack.plist.template`).
pub const LAUNCH_AGENT_LABEL: &str = "com.example.rusty-jack";
const LAUNCH_AGENT_TEMPLATE: &str =
    include_str!("../launchd/com.example.rusty-jack.plist.template");

/// Result of `rusty-jack install` (install LaunchAgent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InstallResult {
    Installed {
        label: String,
        plist_path: String,
        binary_path: String,
        was_loaded: bool,
    },
}

/// Result of `rusty-jack upgrade` (refresh plist and restart LaunchAgent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpgradeResult {
    Upgraded {
        label: String,
        plist_path: String,
        binary_path: String,
        was_loaded: bool,
    },
    Installed {
        label: String,
        plist_path: String,
        binary_path: String,
    },
}

/// Result of `rusty-jack disable` (uninstall LaunchAgent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DisableResult {
    Uninstalled {
        label: String,
        plist_path: String,
        was_loaded: bool,
    },
    NotInstalled {
        plist_path: String,
    },
}

/// Result of `rusty-jack pause`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PauseResult {
    Paused {
        label: String,
        plist_path: String,
        was_loaded: bool,
    },
    NotInstalled {
        plist_path: String,
    },
}

/// Result of `rusty-jack resume`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResumeResult {
    Resumed { label: String, plist_path: String },
    NotInstalled { plist_path: String },
    AlreadyRunning { label: String, plist_path: String },
}

/// Current LaunchAgent status for `rusty-jack status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DaemonStatus {
    Running {
        label: String,
        plist_path: String,
        service: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
    },
    Paused {
        label: String,
        plist_path: String,
        service: String,
    },
    NotInstalled {
        plist_path: String,
    },
    Unknown {
        label: String,
        plist_path: String,
        message: String,
    },
}

/// Path to the user LaunchAgent plist (`~/Library/LaunchAgents/<label>.plist`).
#[must_use]
pub fn launch_agent_plist_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
    })
}

/// launchd GUI domain for the current user (`gui/<uid>`).
pub fn gui_domain() -> Result<String, RustyJackError> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .map_err(RustyJackError::Io)?;
    if !output.status.success() {
        return Err(RustyJackError::Launchd(
            "failed to resolve user id (id -u)".into(),
        ));
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uid.is_empty() {
        return Err(RustyJackError::Launchd("empty user id".into()));
    }
    Ok(format!("gui/{uid}"))
}

fn service_id(domain: &str, label: &str) -> String {
    format!("{domain}/{label}")
}

fn plist_path_or_err() -> Result<PathBuf, RustyJackError> {
    launch_agent_plist_path().ok_or_else(|| {
        RustyJackError::Launchd("HOME is not set; cannot locate LaunchAgents".into())
    })
}

fn home_dir_or_err() -> Result<PathBuf, RustyJackError> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| RustyJackError::Launchd("HOME is not set; cannot install LaunchAgent".into()))
}

fn plist_path_display(path: &Path) -> Result<String, RustyJackError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| RustyJackError::Launchd("plist path is not valid UTF-8".into()))
}

fn is_job_loaded(domain: &str, label: &str) -> Result<bool, RustyJackError> {
    Ok(launchctl_print_service(domain, label)?.is_some())
}

fn launchctl_print_service(domain: &str, label: &str) -> Result<Option<String>, RustyJackError> {
    let service = service_id(domain, label);
    let output = Command::new("launchctl")
        .args(["print", &service])
        .output()
        .map_err(RustyJackError::Io)?;
    if output.status.success() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
    } else {
        Ok(None)
    }
}

fn run_launchctl(args: &[&str]) -> Result<i32, RustyJackError> {
    let output = Command::new("launchctl")
        .args(args)
        .output()
        .map_err(RustyJackError::Io)?;
    Ok(output.status.code().unwrap_or(-1))
}

fn stop_job_best_effort(domain: &str, plist_path: &Path) {
    if let Ok(plist_arg) = plist_path_display(plist_path) {
        let _ = run_launchctl(&["bootout", domain, &plist_arg]);
    }
    let service = service_id(domain, LAUNCH_AGENT_LABEL);
    let _ = run_launchctl(&["disable", &service]);
}

fn current_exe_display() -> Result<String, RustyJackError> {
    std::env::current_exe()
        .map_err(RustyJackError::Io)
        .and_then(|path| {
            path.to_str()
                .map(str::to_string)
                .ok_or_else(|| RustyJackError::Launchd("binary path is not valid UTF-8".into()))
        })
}

fn render_launch_agent_plist(binary_path: &str, home: &str) -> String {
    LAUNCH_AGENT_TEMPLATE
        .replace("@BINARY_PATH@", &escape_xml(binary_path))
        .replace("@HOME@", &escape_xml(home))
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadDaemonResult {
    label: String,
    plist_path: String,
    binary_path: String,
    was_loaded: bool,
    had_plist: bool,
}

fn write_and_load_daemon() -> Result<LoadDaemonResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    let home = home_dir_or_err()?;
    let binary_path = current_exe_display()?;
    let home_display = plist_path_display(&home)?;
    let domain = gui_domain()?;
    let was_loaded = is_job_loaded(&domain, LAUNCH_AGENT_LABEL)?;
    let had_plist = plist_path.exists();

    if had_plist || was_loaded {
        stop_job_best_effort(&domain, &plist_path);
    }

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    std::fs::create_dir_all(home.join("Library/Logs")).map_err(RustyJackError::Io)?;

    let plist = render_launch_agent_plist(&binary_path, &home_display);
    std::fs::write(&plist_path, plist).map_err(RustyJackError::Io)?;

    let service = service_id(&domain, LAUNCH_AGENT_LABEL);
    let enable_status = run_launchctl(&["enable", &service])?;
    if enable_status != 0 {
        return Err(RustyJackError::Launchd(format!(
            "launchctl enable {service} failed (status {enable_status})"
        )));
    }

    let plist_display = plist_path_display(&plist_path)?;
    let bootstrap_status = run_launchctl(&["bootstrap", &domain, &plist_display])?;
    if bootstrap_status != 0 {
        return Err(RustyJackError::Launchd(format!(
            "launchctl bootstrap {domain} {plist_display} failed (status {bootstrap_status})"
        )));
    }

    Ok(LoadDaemonResult {
        label: LAUNCH_AGENT_LABEL.into(),
        plist_path: plist_display,
        binary_path,
        was_loaded,
        had_plist,
    })
}

/// Install or reinstall the LaunchAgent for the current user.
pub fn install_daemon() -> Result<InstallResult, RustyJackError> {
    let result = write_and_load_daemon()?;
    Ok(InstallResult::Installed {
        label: result.label,
        plist_path: result.plist_path,
        binary_path: result.binary_path,
        was_loaded: result.was_loaded,
    })
}

/// Refresh the LaunchAgent plist to the current binary and restart the daemon.
pub fn upgrade_daemon() -> Result<UpgradeResult, RustyJackError> {
    let result = write_and_load_daemon()?;
    if result.had_plist {
        Ok(UpgradeResult::Upgraded {
            label: result.label,
            plist_path: result.plist_path,
            binary_path: result.binary_path,
            was_loaded: result.was_loaded,
        })
    } else {
        Ok(UpgradeResult::Installed {
            label: result.label,
            plist_path: result.plist_path,
            binary_path: result.binary_path,
        })
    }
}

/// Inspect whether the per-user LaunchAgent is installed, running, or paused.
pub fn daemon_status() -> Result<DaemonStatus, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    let plist_display = plist_path_display(&plist_path)?;
    if !plist_path.exists() {
        return Ok(DaemonStatus::NotInstalled {
            plist_path: plist_display,
        });
    }

    let domain = gui_domain()?;
    let service = service_id(&domain, LAUNCH_AGENT_LABEL);
    match launchctl_print_service(&domain, LAUNCH_AGENT_LABEL) {
        Ok(Some(output)) => Ok(DaemonStatus::Running {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: plist_display,
            service,
            pid: parse_launchctl_pid(&output),
        }),
        Ok(None) => Ok(DaemonStatus::Paused {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: plist_display,
            service,
        }),
        Err(err) => Ok(DaemonStatus::Unknown {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: plist_display,
            message: err.to_string(),
        }),
    }
}

fn parse_launchctl_pid(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("pid = ")
            .and_then(|value| value.parse::<u32>().ok())
    })
}

/// Stop the daemon and prevent launchd from restarting it (`pause`). Keeps the plist installed.
pub fn pause_daemon() -> Result<PauseResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    if !plist_path.exists() {
        return Ok(PauseResult::NotInstalled {
            plist_path: plist_path_display(&plist_path)?,
        });
    }

    let domain = gui_domain()?;
    let was_loaded = is_job_loaded(&domain, LAUNCH_AGENT_LABEL)?;
    stop_job_best_effort(&domain, &plist_path);

    Ok(PauseResult::Paused {
        label: LAUNCH_AGENT_LABEL.into(),
        plist_path: plist_path_display(&plist_path)?,
        was_loaded,
    })
}

/// Re-enable and load a paused LaunchAgent (`resume`).
pub fn resume_daemon() -> Result<ResumeResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    if !plist_path.exists() {
        return Ok(ResumeResult::NotInstalled {
            plist_path: plist_path_display(&plist_path)?,
        });
    }

    let domain = gui_domain()?;
    let plist_display = plist_path_display(&plist_path)?;
    if is_job_loaded(&domain, LAUNCH_AGENT_LABEL)? {
        return Ok(ResumeResult::AlreadyRunning {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: plist_display,
        });
    }

    let service = service_id(&domain, LAUNCH_AGENT_LABEL);
    let enable_status = run_launchctl(&["enable", &service])?;
    if enable_status != 0 {
        return Err(RustyJackError::Launchd(format!(
            "launchctl enable {service} failed (status {enable_status})"
        )));
    }

    let bootstrap_status = run_launchctl(&["bootstrap", &domain, &plist_display])?;
    if bootstrap_status != 0 {
        return Err(RustyJackError::Launchd(format!(
            "launchctl bootstrap {domain} {plist_display} failed (status {bootstrap_status})"
        )));
    }

    Ok(ResumeResult::Resumed {
        label: LAUNCH_AGENT_LABEL.into(),
        plist_path: plist_display,
    })
}

/// Uninstall the LaunchAgent: stop the job, disable it, and remove the plist (`disable`).
pub fn uninstall_daemon() -> Result<DisableResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    if !plist_path.exists() {
        return Ok(DisableResult::NotInstalled {
            plist_path: plist_path_display(&plist_path)?,
        });
    }

    let domain = gui_domain()?;
    let was_loaded = is_job_loaded(&domain, LAUNCH_AGENT_LABEL)?;
    stop_job_best_effort(&domain, &plist_path);
    std::fs::remove_file(&plist_path).map_err(RustyJackError::Io)?;

    Ok(DisableResult::Uninstalled {
        label: LAUNCH_AGENT_LABEL.into(),
        plist_path: plist_path_display(&plist_path)?,
        was_loaded,
    })
}

/// Human-readable output for [`InstallResult`].
pub fn print_install_result(result: &InstallResult) {
    match result {
        InstallResult::Installed {
            label,
            plist_path,
            binary_path,
            was_loaded,
        } => {
            println!("Installed rusty-jack daemon ({label})");
            println!("  binary:      {binary_path}");
            println!("  plist:       {plist_path}");
            println!("  restarted:   {}", if *was_loaded { "yes" } else { "no" });
            println!("  auto-routing active");
        }
    }
}

/// Human-readable output for [`UpgradeResult`].
pub fn print_upgrade_result(result: &UpgradeResult) {
    match result {
        UpgradeResult::Upgraded {
            label,
            plist_path,
            binary_path,
            was_loaded,
        } => {
            println!("Upgraded rusty-jack daemon LaunchAgent ({label})");
            println!("  binary:      {binary_path}");
            println!("  plist:       {plist_path}");
            println!("  was running: {}", if *was_loaded { "yes" } else { "no" });
            println!("  auto-routing active");
        }
        UpgradeResult::Installed {
            label,
            plist_path,
            binary_path,
        } => {
            println!("Daemon was not installed; installed it now ({label})");
            println!("  binary: {binary_path}");
            println!("  plist:  {plist_path}");
        }
    }
}

/// Human-readable output for [`DisableResult`].
pub fn print_disable_result(result: &DisableResult) {
    match result {
        DisableResult::Uninstalled {
            label,
            plist_path,
            was_loaded,
        } => {
            println!("Uninstalled rusty-jack daemon ({label})");
            println!("  removed plist: {plist_path}");
            println!(
                "  was running:   {}",
                if *was_loaded { "yes" } else { "no" }
            );
        }
        DisableResult::NotInstalled { plist_path } => {
            println!("Daemon not installed");
            println!("  expected plist: {plist_path}");
        }
    }
}

/// Human-readable output for [`PauseResult`].
pub fn print_pause_result(result: &PauseResult) {
    match result {
        PauseResult::Paused {
            label,
            plist_path,
            was_loaded,
        } => {
            println!("Paused rusty-jack daemon ({label})");
            println!("  plist kept:    {plist_path}");
            println!(
                "  was running:   {}",
                if *was_loaded { "yes" } else { "no" }
            );
            println!("  auto-routing stopped until `rusty-jack resume`");
        }
        PauseResult::NotInstalled { plist_path } => {
            println!("Daemon not installed (nothing to pause)");
            println!("  expected plist: {plist_path}");
        }
    }
}

/// Human-readable output for [`ResumeResult`].
pub fn print_resume_result(result: &ResumeResult) {
    match result {
        ResumeResult::Resumed { label, plist_path } => {
            println!("Resumed rusty-jack daemon ({label})");
            println!("  plist: {plist_path}");
            println!("  auto-routing active");
        }
        ResumeResult::AlreadyRunning { label, plist_path } => {
            println!("Daemon already running ({label})");
            println!("  plist: {plist_path}");
        }
        ResumeResult::NotInstalled { plist_path } => {
            println!("Daemon not installed (nothing to resume)");
            println!("  expected plist: {plist_path}");
            println!("  install the LaunchAgent first, then run resume");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launch_agent_plist_path_ends_with_label() {
        let path = launch_agent_plist_path().unwrap();
        assert!(path.ends_with("Library/LaunchAgents/com.example.rusty-jack.plist"));
    }

    #[test]
    fn test_service_id_format() {
        assert_eq!(
            service_id("gui/501", LAUNCH_AGENT_LABEL),
            "gui/501/com.example.rusty-jack"
        );
    }

    #[test]
    fn test_render_launch_agent_plist_replaces_placeholders() {
        let plist = render_launch_agent_plist("/tmp/rusty-jack", "/Users/example");
        assert!(plist.contains("<string>/tmp/rusty-jack</string>"));
        assert!(
            plist.contains("<string>/Users/example/Library/Logs/rusty-jack.stdout.log</string>")
        );
        assert!(!plist.contains("@BINARY_PATH@"));
        assert!(!plist.contains("@HOME@"));
    }

    #[test]
    fn test_render_launch_agent_plist_escapes_xml() {
        let plist = render_launch_agent_plist("/tmp/rusty&jack", "/Users/a<b");
        assert!(plist.contains("/tmp/rusty&amp;jack"));
        assert!(plist.contains("/Users/a&lt;b/Library/Logs"));
    }

    #[test]
    fn test_parse_launchctl_pid() {
        let output = r#"
	state = running
	pid = 12345
"#;
        assert_eq!(parse_launchctl_pid(output), Some(12_345));
    }

    #[test]
    fn test_install_result_serializes() {
        let json = serde_json::to_string(&InstallResult::Installed {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: "/tmp/test.plist".into(),
            binary_path: "/tmp/rusty-jack".into(),
            was_loaded: false,
        })
        .unwrap();
        assert!(json.contains("\"status\":\"installed\""));
    }

    #[test]
    fn test_upgrade_result_serializes() {
        let json = serde_json::to_string(&UpgradeResult::Upgraded {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: "/tmp/test.plist".into(),
            binary_path: "/tmp/rusty-jack".into(),
            was_loaded: true,
        })
        .unwrap();
        assert!(json.contains("\"status\":\"upgraded\""));
    }

    #[test]
    fn test_disable_result_serializes() {
        let json = serde_json::to_string(&DisableResult::NotInstalled {
            plist_path: "/tmp/test.plist".into(),
        })
        .unwrap();
        assert!(json.contains("\"status\":\"not_installed\""));
    }

    #[test]
    fn test_pause_result_serializes() {
        let json = serde_json::to_string(&PauseResult::NotInstalled {
            plist_path: "/tmp/test.plist".into(),
        })
        .unwrap();
        assert!(json.contains("\"status\":\"not_installed\""));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_gui_domain_looks_valid() {
        let domain = gui_domain().unwrap();
        assert!(domain.starts_with("gui/"));
    }
}
