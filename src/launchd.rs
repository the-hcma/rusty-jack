//! launchd LaunchAgent management (macOS).

use crate::version::BinaryVersion;
use crate::RustyJackError;
use serde::{Deserialize, Serialize};
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
    UpToDate {
        label: String,
        plist_path: String,
        binary_path: String,
        binary_version: BinaryVersion,
        was_loaded: bool,
    },
    Upgraded {
        label: String,
        plist_path: String,
        binary_path: String,
        binary_version: BinaryVersion,
        previous_binary_path: Option<String>,
        previous_binary_version: Option<BinaryVersion>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_running_daemon_version: Option<BinaryVersion>,
        was_loaded: bool,
        resumed_after_upgrade: bool,
    },
    Installed {
        label: String,
        plist_path: String,
        binary_path: String,
        binary_version: BinaryVersion,
    },
}

/// Command users should run after installing or upgrading the CLI binary.
pub const DAEMON_REFRESH_COMMAND: &str = "rusty-jack upgrade --force";

/// LaunchAgent env vars stamped at install/upgrade and read from the running daemon PID.
pub const DAEMON_PKG_VERSION_ENV: &str = "RUSTY_JACK_DAEMON_PKG_VERSION";
pub const DAEMON_GIT_COMMIT_ENV: &str = "RUSTY_JACK_DAEMON_GIT_COMMIT";

/// Compare the LaunchAgent/daemon binary against the current CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DaemonVersionCheck {
    pub cli_binary_path: String,
    pub cli_version: BinaryVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plist_binary_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plist_binary_version: Option<BinaryVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_binary_version: Option<BinaryVersion>,
    /// LaunchAgent or running daemon is missing stamped version env vars.
    pub needs_version_stamp_refresh: bool,
    pub stale: bool,
    pub refresh_command: &'static str,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        pause_reason: Option<DaemonPauseReason>,
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
        /// launchd `state` from `launchctl print` (for example `running`, `spawn scheduled`).
        #[serde(skip_serializing_if = "Option::is_none")]
        launch_job_state: Option<String>,
        /// launchd `last exit code` when the job is loaded but the process is not running.
        #[serde(skip_serializing_if = "Option::is_none")]
        last_exit_code: Option<String>,
    },
    Paused {
        label: String,
        plist_path: String,
        service: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pause_reason: Option<DaemonPauseReason>,
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

/// Why the LaunchAgent is intentionally paused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DaemonPauseReason {
    PickerOverride {
        selected_uid: String,
        selected_label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        preferred_uid: Option<String>,
    },
}

impl DaemonPauseReason {
    #[must_use]
    pub fn picker_override(
        selected_uid: String,
        selected_label: String,
        preferred_uid: Option<String>,
    ) -> Self {
        Self::PickerOverride {
            selected_uid,
            selected_label,
            preferred_uid,
        }
    }

    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::PickerOverride { .. } => "picker override",
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::PickerOverride { selected_label, .. } => {
                format!("user picked {selected_label}; daemon is paused until `rusty-jack resume`")
            }
        }
    }
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

fn pause_reason_path() -> Option<PathBuf> {
    crate::config::default_config_path().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("daemon-pause-reason.json"))
    })
}

fn write_pause_reason(reason: Option<&DaemonPauseReason>) -> Result<(), RustyJackError> {
    let Some(path) = pause_reason_path() else {
        return Ok(());
    };

    if let Some(reason) = reason {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
        }
        let raw = serde_json::to_string_pretty(reason)
            .map_err(|err| RustyJackError::Launchd(format!("pause reason JSON: {err}")))?;
        std::fs::write(path, format!("{raw}\n")).map_err(RustyJackError::Io)?;
    } else if path.exists() {
        std::fs::remove_file(path).map_err(RustyJackError::Io)?;
    }

    Ok(())
}

fn read_pause_reason() -> Result<Option<DaemonPauseReason>, RustyJackError> {
    let Some(path) = pause_reason_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(path).map_err(RustyJackError::Io)?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|err| RustyJackError::Launchd(format!("pause reason JSON: {err}")))
}

fn clear_pause_reason() {
    let _ = write_pause_reason(None);
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

fn render_launch_agent_plist(
    binary_path: &str,
    home: &str,
    daemon_version: &BinaryVersion,
) -> String {
    LAUNCH_AGENT_TEMPLATE
        .replace("@BINARY_PATH@", &escape_xml(binary_path))
        .replace("@HOME@", &escape_xml(home))
        .replace("@DAEMON_PKG_VERSION@", &escape_xml(&daemon_version.version))
        .replace("@DAEMON_GIT_COMMIT@", &escape_xml(&daemon_version.commit))
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
    binary_version: BinaryVersion,
    previous_binary_path: Option<String>,
    previous_binary_version: Option<BinaryVersion>,
    previous_running_daemon_version: Option<BinaryVersion>,
    was_loaded: bool,
    had_plist: bool,
    changed: bool,
    resumed_after_upgrade: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadMode {
    Install,
    Upgrade,
}

fn write_and_load_daemon(
    mode: LoadMode,
    force_reload: bool,
) -> Result<LoadDaemonResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    let home = home_dir_or_err()?;
    let binary_path = current_exe_display()?;
    let home_display = plist_path_display(&home)?;
    let domain = gui_domain()?;
    let was_loaded = is_job_loaded(&domain, LAUNCH_AGENT_LABEL)?;
    let had_plist = plist_path.exists();
    let existing_plist = if had_plist {
        std::fs::read_to_string(&plist_path).ok()
    } else {
        None
    };
    let previous_binary_path = existing_plist
        .as_deref()
        .and_then(launch_agent_binary_path_from_plist);
    let (previous_binary_version, previous_running_daemon_version) =
        previous_daemon_versions_before_upgrade(
            &domain,
            was_loaded,
            existing_plist.as_deref(),
            previous_binary_path.as_deref(),
        );
    let binary_version = BinaryVersion::current();
    let plist = render_launch_agent_plist(&binary_path, &home_display, &binary_version);
    let plist_display = plist_path_display(&plist_path)?;

    if matches!(mode, LoadMode::Upgrade)
        && daemon_upgrade_is_current(
            force_reload,
            existing_plist.as_deref(),
            &plist,
            previous_binary_path.as_deref(),
            previous_binary_version.as_ref(),
            &binary_path,
            &binary_version,
        )
    {
        return Ok(LoadDaemonResult {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: plist_display,
            binary_path,
            binary_version,
            previous_binary_path,
            previous_binary_version,
            previous_running_daemon_version,
            was_loaded,
            had_plist,
            changed: false,
            resumed_after_upgrade: false,
        });
    }

    let load_after_write = should_load_after_write(mode, had_plist, was_loaded);

    if should_stop_before_write(mode, had_plist, was_loaded) {
        stop_job_best_effort(&domain, &plist_path);
    }

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    std::fs::create_dir_all(home.join("Library/Logs")).map_err(RustyJackError::Io)?;

    std::fs::write(&plist_path, plist).map_err(RustyJackError::Io)?;

    let service = service_id(&domain, LAUNCH_AGENT_LABEL);
    if load_after_write {
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
        clear_pause_reason();
    }

    Ok(LoadDaemonResult {
        label: LAUNCH_AGENT_LABEL.into(),
        plist_path: plist_display,
        binary_path,
        binary_version,
        previous_binary_path,
        previous_binary_version,
        previous_running_daemon_version,
        was_loaded,
        had_plist,
        changed: true,
        resumed_after_upgrade: matches!(mode, LoadMode::Upgrade) && had_plist && was_loaded,
    })
}

fn daemon_upgrade_is_current(
    force_reload: bool,
    existing_plist: Option<&str>,
    rendered_plist: &str,
    previous_binary_path: Option<&str>,
    previous_binary_version: Option<&BinaryVersion>,
    binary_path: &str,
    binary_version: &BinaryVersion,
) -> bool {
    !force_reload
        && existing_plist.is_some_and(|plist| plist == rendered_plist)
        && previous_binary_path == Some(binary_path)
        && previous_binary_version == Some(binary_version)
}

fn should_stop_before_write(mode: LoadMode, had_plist: bool, was_loaded: bool) -> bool {
    match mode {
        LoadMode::Install => had_plist || was_loaded,
        LoadMode::Upgrade => was_loaded,
    }
}

fn should_load_after_write(mode: LoadMode, had_plist: bool, was_loaded: bool) -> bool {
    match mode {
        LoadMode::Install => true,
        LoadMode::Upgrade => !had_plist || was_loaded,
    }
}

fn binary_path_from_pid(pid: u32) -> Option<String> {
    crate::process_detect::process_exe_path(pid)
}

fn binary_version_from_path(path: &str) -> Option<BinaryVersion> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_binary_version_output(&String::from_utf8_lossy(&output.stdout))
}

fn previous_binary_version_from_env_or_path(path: Option<&str>) -> Option<BinaryVersion> {
    if let Ok(raw) = std::env::var("RUSTY_JACK_UPGRADE_PREVIOUS_VERSION") {
        if let Some(version) = previous_binary_version_from_snapshot(Some(raw.as_str())) {
            return Some(version);
        }
    }
    path.and_then(binary_version_from_path)
}

fn previous_binary_version_from_snapshot(raw: Option<&str>) -> Option<BinaryVersion> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let output = raw.strip_prefix("rusty-jack ").unwrap_or(raw);
    parse_binary_version_output(output)
}

fn parse_binary_version_output(output: &str) -> Option<BinaryVersion> {
    let output = output.trim();
    let output = output.strip_prefix("rusty-jack ").unwrap_or(output);
    let (version, commit) = output.split_once(" (commit ")?;
    let commit = commit.strip_suffix(')')?;
    if version.is_empty() || commit.is_empty() {
        return None;
    }
    Some(BinaryVersion {
        version: version.into(),
        commit: commit.into(),
    })
}

fn launch_agent_binary_path_from_plist(plist: &str) -> Option<String> {
    let (_, after_key) = plist.split_once("<key>ProgramArguments</key>")?;
    let (_, after_array) = after_key.split_once("<array>")?;
    let (_, after_string) = after_array.split_once("<string>")?;
    let (value, _) = after_string.split_once("</string>")?;
    Some(unescape_xml(value))
}

fn launch_agent_env_var_from_plist(plist: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let (_, after_key) = plist.split_once(&marker)?;
    let (_, after_string) = after_key.split_once("<string>")?;
    let (value, _) = after_string.split_once("</string>")?;
    Some(unescape_xml(value))
}

fn daemon_version_from_env_pair(
    version: Option<String>,
    commit: Option<String>,
) -> Option<BinaryVersion> {
    Some(BinaryVersion {
        version: version?,
        commit: commit?,
    })
}

fn daemon_version_from_plist(plist: &str) -> Option<BinaryVersion> {
    daemon_version_from_env_pair(
        launch_agent_env_var_from_plist(plist, DAEMON_PKG_VERSION_ENV),
        launch_agent_env_var_from_plist(plist, DAEMON_GIT_COMMIT_ENV),
    )
}

fn plist_has_daemon_version_stamp(plist: &str) -> bool {
    daemon_version_from_plist(plist).is_some()
}

fn process_environ(pid: u32) -> Option<Vec<std::ffi::OsString>> {
    crate::process_detect::process_environ(pid)
}

fn env_var_from_environ(entries: &[std::ffi::OsString], key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    entries.iter().find_map(|entry| {
        entry
            .to_str()
            .and_then(|entry| entry.strip_prefix(&prefix))
            .map(str::to_string)
    })
}

fn daemon_version_from_process_env(pid: u32) -> Option<BinaryVersion> {
    let environ = process_environ(pid)?;
    daemon_version_from_env_pair(
        env_var_from_environ(&environ, DAEMON_PKG_VERSION_ENV),
        env_var_from_environ(&environ, DAEMON_GIT_COMMIT_ENV),
    )
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

/// Install or reinstall the LaunchAgent for the current user.
pub fn install_daemon() -> Result<InstallResult, RustyJackError> {
    let result = write_and_load_daemon(LoadMode::Install, false)?;
    Ok(InstallResult::Installed {
        label: result.label,
        plist_path: result.plist_path,
        binary_path: result.binary_path,
        was_loaded: result.was_loaded,
    })
}

/// Paths where the daemon writes structured logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DaemonLogPaths {
    pub file: String,
}

/// Resolve the per-user daemon log path used by the daemon logger.
pub fn daemon_log_paths() -> Result<DaemonLogPaths, RustyJackError> {
    let path = crate::logging::resolve_log_file_path(std::path::Path::new(
        "~/Library/Logs/rusty-jack.log",
    ))?;
    Ok(DaemonLogPaths {
        file: path.display().to_string(),
    })
}

/// Refresh the LaunchAgent plist to the current binary and restart the daemon.
pub fn upgrade_daemon(force_reload: bool) -> Result<UpgradeResult, RustyJackError> {
    let result = write_and_load_daemon(LoadMode::Upgrade, force_reload)?;
    if result.had_plist && !result.changed {
        Ok(UpgradeResult::UpToDate {
            label: result.label,
            plist_path: result.plist_path,
            binary_path: result.binary_path,
            binary_version: result.binary_version,
            was_loaded: result.was_loaded,
        })
    } else if result.had_plist {
        Ok(UpgradeResult::Upgraded {
            label: result.label,
            plist_path: result.plist_path,
            binary_path: result.binary_path,
            binary_version: result.binary_version,
            previous_binary_path: result.previous_binary_path,
            previous_binary_version: result.previous_binary_version,
            previous_running_daemon_version: result.previous_running_daemon_version,
            was_loaded: result.was_loaded,
            resumed_after_upgrade: result.resumed_after_upgrade,
        })
    } else {
        Ok(UpgradeResult::Installed {
            label: result.label,
            plist_path: result.plist_path,
            binary_path: result.binary_path,
            binary_version: result.binary_version,
        })
    }
}

/// Compare the installed LaunchAgent binary with the current CLI.
pub fn daemon_version_check(
    running_pid: Option<u32>,
) -> Result<DaemonVersionCheck, RustyJackError> {
    let cli_binary_path = current_exe_display()?;
    let cli_version = BinaryVersion::current();
    let plist_path = plist_path_or_err()?;

    let (plist_binary_path, plist_binary_version, plist_has_version_stamp) = if plist_path.exists()
    {
        let plist = std::fs::read_to_string(&plist_path).map_err(RustyJackError::Io)?;
        let path = launch_agent_binary_path_from_plist(&plist);
        let version = daemon_version_from_plist(&plist)
            .or_else(|| path.as_deref().and_then(binary_version_from_path));
        (path, version, plist_has_daemon_version_stamp(&plist))
    } else {
        (None, None, false)
    };

    let running_binary_version = running_pid
        .and_then(daemon_version_from_process_env)
        .or_else(|| {
            running_pid
                .and_then(binary_path_from_pid)
                .as_deref()
                .and_then(binary_version_from_path)
        });
    let running_has_version_stamp =
        running_pid.is_some_and(|pid| daemon_version_from_process_env(pid).is_some());
    let needs_version_stamp_refresh = plist_binary_path.is_some()
        && (!plist_has_version_stamp || running_pid.is_some_and(|_| !running_has_version_stamp));

    let stale = plist_binary_path.is_some()
        && (needs_version_stamp_refresh
            || plist_binary_path.as_deref() != Some(cli_binary_path.as_str())
            || plist_binary_version
                .as_ref()
                .is_some_and(|version| !version.matches(&cli_version))
            || running_binary_version
                .as_ref()
                .is_some_and(|version| !version.matches(&cli_version)));

    Ok(DaemonVersionCheck {
        cli_binary_path,
        cli_version,
        plist_binary_path,
        plist_binary_version,
        running_binary_version,
        needs_version_stamp_refresh,
        stale,
        refresh_command: DAEMON_REFRESH_COMMAND,
    })
}

/// True when launchd reports a live daemon PID (not merely a loaded job).
#[must_use]
pub fn daemon_process_running(status: &DaemonStatus) -> bool {
    matches!(status, DaemonStatus::Running { pid: Some(_), .. })
}

/// Human-readable remediation when the supervisor is not healthy.
#[must_use]
pub fn daemon_supervisor_error_message(status: &DaemonStatus) -> Option<String> {
    if daemon_process_running(status) {
        return None;
    }
    Some(match status {
        DaemonStatus::NotInstalled { .. } => {
            "daemon not installed; run `rusty-jack install`".into()
        }
        DaemonStatus::Paused { .. } => "daemon paused; run `rusty-jack resume`".into(),
        DaemonStatus::Unknown { message, .. } => {
            format!("daemon state unknown: {message}")
        }
        DaemonStatus::Running {
            last_exit_code,
            launch_job_state,
            ..
        } => {
            let mut detail = String::from("daemon not running");
            if let Some(state) = launch_job_state {
                detail.push_str(&format!(" (launchd state={state})"));
            }
            if let Some(code) = last_exit_code
                .as_ref()
                .filter(|code| *code != "(never exited)")
            {
                detail.push_str(&format!(" (last exit code={code})"));
            }
            detail.push_str("; run `rusty-jack upgrade --force` or `rusty-jack resume`");
            detail
        }
    })
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
            launch_job_state: parse_launchctl_field(&output, "state ="),
            last_exit_code: parse_launchctl_field(&output, "last exit code ="),
        }),
        Ok(None) => Ok(DaemonStatus::Paused {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: plist_display,
            service,
            pause_reason: read_pause_reason().ok().flatten(),
        }),
        Err(err) => Ok(DaemonStatus::Unknown {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: plist_display,
            message: err.to_string(),
        }),
    }
}

fn parse_launchctl_field(output: &str, prefix: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix(prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn parse_launchctl_pid(output: &str) -> Option<u32> {
    parse_launchctl_field(output, "pid =").and_then(|value| value.parse::<u32>().ok())
}

fn running_daemon_pid(domain: &str) -> Option<u32> {
    let output = launchctl_print_service(domain, LAUNCH_AGENT_LABEL).ok()??;
    parse_launchctl_pid(&output)
}

fn previous_daemon_versions_before_upgrade(
    domain: &str,
    was_loaded: bool,
    existing_plist: Option<&str>,
    previous_binary_path: Option<&str>,
) -> (Option<BinaryVersion>, Option<BinaryVersion>) {
    let previous_plist_version = existing_plist.and_then(daemon_version_from_plist);
    let previous_running_daemon_version = if was_loaded {
        running_daemon_pid(domain).and_then(daemon_version_from_process_env)
    } else {
        None
    };
    let previous_binary_version = previous_plist_version
        .or_else(|| previous_binary_version_from_env_or_path(previous_binary_path));
    (previous_binary_version, previous_running_daemon_version)
}

/// Stop the daemon and prevent launchd from restarting it (`pause`). Keeps the plist installed.
pub fn pause_daemon() -> Result<PauseResult, RustyJackError> {
    pause_daemon_with_reason(None)
}

/// Stop the daemon and record why it is paused.
pub fn pause_daemon_with_reason(
    pause_reason: Option<DaemonPauseReason>,
) -> Result<PauseResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    if !plist_path.exists() {
        clear_pause_reason();
        return Ok(PauseResult::NotInstalled {
            plist_path: plist_path_display(&plist_path)?,
        });
    }

    let domain = gui_domain()?;
    let was_loaded = is_job_loaded(&domain, LAUNCH_AGENT_LABEL)?;
    stop_job_best_effort(&domain, &plist_path);
    write_pause_reason(pause_reason.as_ref())?;

    Ok(PauseResult::Paused {
        label: LAUNCH_AGENT_LABEL.into(),
        plist_path: plist_path_display(&plist_path)?,
        was_loaded,
        pause_reason,
    })
}

/// Re-enable and load a paused LaunchAgent (`resume`).
pub fn resume_daemon() -> Result<ResumeResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    if !plist_path.exists() {
        clear_pause_reason();
        return Ok(ResumeResult::NotInstalled {
            plist_path: plist_path_display(&plist_path)?,
        });
    }

    let domain = gui_domain()?;
    let plist_display = plist_path_display(&plist_path)?;
    if is_job_loaded(&domain, LAUNCH_AGENT_LABEL)? {
        clear_pause_reason();
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

    let result = ResumeResult::Resumed {
        label: LAUNCH_AGENT_LABEL.into(),
        plist_path: plist_display,
    };
    clear_pause_reason();
    Ok(result)
}

/// Uninstall the LaunchAgent: stop the job, disable it, and remove the plist (`disable`).
pub fn uninstall_daemon() -> Result<DisableResult, RustyJackError> {
    let plist_path = plist_path_or_err()?;
    if !plist_path.exists() {
        clear_pause_reason();
        return Ok(DisableResult::NotInstalled {
            plist_path: plist_path_display(&plist_path)?,
        });
    }

    let domain = gui_domain()?;
    let was_loaded = is_job_loaded(&domain, LAUNCH_AGENT_LABEL)?;
    stop_job_best_effort(&domain, &plist_path);
    std::fs::remove_file(&plist_path).map_err(RustyJackError::Io)?;
    clear_pause_reason();

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
        UpgradeResult::UpToDate {
            label,
            plist_path,
            binary_path,
            binary_version,
            was_loaded,
        } => {
            println!("rusty-jack daemon LaunchAgent is up to date ({label})");
            println!("  version:      {}", binary_version.display());
            println!("  binary:       {binary_path}");
            println!("  plist:        {plist_path}");
            println!(
                "  auto-routing: {}",
                if *was_loaded {
                    "active"
                } else {
                    "remains paused"
                }
            );
        }
        UpgradeResult::Upgraded {
            label,
            plist_path,
            binary_path,
            binary_version,
            previous_binary_path,
            previous_binary_version,
            previous_running_daemon_version,
            was_loaded,
            resumed_after_upgrade,
        } => {
            println!("Upgraded rusty-jack daemon LaunchAgent ({label})");
            if *was_loaded {
                println!(
                    "  daemon before: {}",
                    version_display(previous_running_daemon_version.as_ref())
                );
            }
            if let Some(configured) = previous_binary_version.as_ref() {
                if !*was_loaded {
                    println!("  before:     {}", configured.display());
                } else if previous_running_daemon_version.as_ref() != Some(configured) {
                    println!("  plist before:  {}", configured.display());
                }
            } else if !*was_loaded {
                println!("  before:     {}", version_display(None));
            }
            if let Some(path) = previous_binary_path {
                println!("  old binary: {path}");
            }
            println!("  after:      {}", binary_version.display());
            println!("  binary:     {binary_path}");
            println!("  plist:      {plist_path}");
            println!("  was running: {}", if *was_loaded { "yes" } else { "no" });
            println!(
                "  paused during upgrade: {}",
                if *was_loaded { "yes" } else { "no" }
            );
            println!(
                "  resumed after upgrade: {}",
                if *resumed_after_upgrade { "yes" } else { "no" }
            );
            println!(
                "  auto-routing: {}",
                if *resumed_after_upgrade {
                    "active"
                } else {
                    "remains paused"
                }
            );
        }
        UpgradeResult::Installed {
            label,
            plist_path,
            binary_path,
            binary_version,
        } => {
            println!("Daemon was not installed; installed it now ({label})");
            println!("  before: not installed");
            println!("  after:  {}", binary_version.display());
            println!("  binary: {binary_path}");
            println!("  plist:  {plist_path}");
        }
    }
}

fn version_display(version: Option<&BinaryVersion>) -> String {
    version
        .map(|version| version.display())
        .unwrap_or_else(|| "unknown".into())
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
            pause_reason,
        } => {
            println!("Paused rusty-jack daemon ({label})");
            println!("  plist kept:    {plist_path}");
            println!(
                "  was running:   {}",
                if *was_loaded { "yes" } else { "no" }
            );
            if let Some(reason) = pause_reason {
                println!("  reason:        {}", reason.message());
            }
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
    fn test_parse_launchctl_field_reads_job_metadata() {
        let output = r#"
	state = spawn scheduled
	pid = 20890
	last exit code = 78: EX_CONFIG
"#;
        assert_eq!(
            parse_launchctl_field(output, "state =").as_deref(),
            Some("spawn scheduled")
        );
        assert_eq!(parse_launchctl_pid(output), Some(20_890));
        assert_eq!(
            parse_launchctl_field(output, "last exit code =").as_deref(),
            Some("78: EX_CONFIG")
        );
    }

    #[test]
    fn test_daemon_process_running_requires_pid() {
        let running = DaemonStatus::Running {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: "/tmp/test.plist".into(),
            service: "gui/501/com.example.rusty-jack".into(),
            pid: Some(42),
            launch_job_state: None,
            last_exit_code: None,
        };
        let loaded = DaemonStatus::Running {
            pid: None,
            launch_job_state: Some("spawn scheduled".into()),
            last_exit_code: Some("78: EX_CONFIG".into()),
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: "/tmp/test.plist".into(),
            service: "gui/501/com.example.rusty-jack".into(),
        };
        assert!(daemon_process_running(&running));
        assert!(!daemon_process_running(&loaded));
        assert!(daemon_supervisor_error_message(&loaded).is_some());
    }

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
    fn test_daemon_log_paths_under_home() {
        let home = std::env::var("HOME").expect("HOME should be set in tests");
        let paths = daemon_log_paths().unwrap();
        assert!(paths.file.ends_with("rusty-jack.log"));
        assert!(paths.file.starts_with(&home));
    }

    fn sample_daemon_version() -> BinaryVersion {
        BinaryVersion {
            version: "0.6.0".into(),
            commit: "abc1234".into(),
        }
    }

    #[test]
    fn test_render_launch_agent_plist_replaces_placeholders() {
        let version = sample_daemon_version();
        let plist = render_launch_agent_plist("/tmp/rusty-jack", "/Users/example", &version);
        assert!(plist.contains("<string>/tmp/rusty-jack</string>"));
        assert!(plist.contains("<string>daemon</string>"));
        assert!(plist.contains("<string>0.6.0</string>"));
        assert!(plist.contains("<string>abc1234</string>"));
        assert!(!plist.contains("@BINARY_PATH@"));
        assert!(!plist.contains("@DAEMON_PKG_VERSION@"));
        assert!(!plist.contains("StandardOutPath"));
    }

    #[test]
    fn test_render_launch_agent_plist_escapes_xml() {
        let version = sample_daemon_version();
        let plist = render_launch_agent_plist("/tmp/rusty&jack", "/Users/a<b", &version);
        assert!(plist.contains("/tmp/rusty&amp;jack"));
    }

    #[test]
    fn test_launch_agent_binary_path_from_plist() {
        let version = sample_daemon_version();
        let plist = render_launch_agent_plist("/tmp/rusty&jack", "/Users/example", &version);
        assert_eq!(
            launch_agent_binary_path_from_plist(&plist).as_deref(),
            Some("/tmp/rusty&jack")
        );
    }

    #[test]
    fn test_daemon_version_from_plist_reads_stamped_env() {
        let version = sample_daemon_version();
        let plist = render_launch_agent_plist("/tmp/rusty-jack", "/Users/example", &version);
        assert_eq!(daemon_version_from_plist(&plist).as_ref(), Some(&version));
    }

    #[test]
    fn test_env_var_from_environ() {
        use std::ffi::OsString;

        let entries = [
            OsString::from("RUST_LOG=info"),
            OsString::from("RUSTY_JACK_DAEMON_PKG_VERSION=0.6.0"),
            OsString::from("RUSTY_JACK_DAEMON_GIT_COMMIT=abc1234"),
        ];
        assert_eq!(
            env_var_from_environ(&entries, DAEMON_PKG_VERSION_ENV).as_deref(),
            Some("0.6.0")
        );
        assert_eq!(
            env_var_from_environ(&entries, DAEMON_GIT_COMMIT_ENV).as_deref(),
            Some("abc1234")
        );
    }

    #[test]
    fn test_binary_version_matches_compares_version_and_commit() {
        let left = BinaryVersion {
            version: "0.4.1".into(),
            commit: "abc1234".into(),
        };
        let right = BinaryVersion {
            version: "0.4.1".into(),
            commit: "abc1234".into(),
        };
        let other = BinaryVersion {
            version: "0.4.2".into(),
            commit: "abc1234".into(),
        };
        assert!(left.matches(&right));
        assert!(!left.matches(&other));
    }

    #[test]
    fn test_parse_binary_version_output() {
        assert_eq!(
            parse_binary_version_output("rusty-jack 0.1.0 (commit abc1234)\n"),
            Some(BinaryVersion {
                version: "0.1.0".into(),
                commit: "abc1234".into(),
            })
        );
    }

    #[test]
    fn test_previous_binary_version_from_snapshot() {
        assert_eq!(
            previous_binary_version_from_snapshot(Some("rusty-jack 0.1.0 (commit old1234)")),
            Some(BinaryVersion {
                version: "0.1.0".into(),
                commit: "old1234".into(),
            })
        );
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
    fn test_install_always_loads_after_write() {
        assert!(should_stop_before_write(LoadMode::Install, true, false));
        assert!(should_stop_before_write(LoadMode::Install, false, true));
        assert!(should_load_after_write(LoadMode::Install, true, false));
        assert!(should_load_after_write(LoadMode::Install, false, false));
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
            binary_version: BinaryVersion {
                version: "0.1.0".into(),
                commit: "new1234".into(),
            },
            previous_binary_path: Some("/tmp/old-rusty-jack".into()),
            previous_binary_version: Some(BinaryVersion {
                version: "0.1.0".into(),
                commit: "plist1234".into(),
            }),
            previous_running_daemon_version: Some(BinaryVersion {
                version: "0.1.0".into(),
                commit: "old1234".into(),
            }),
            was_loaded: true,
            resumed_after_upgrade: true,
        })
        .unwrap();
        assert!(json.contains("\"status\":\"upgraded\""));
        assert!(json.contains("\"resumed_after_upgrade\":true"));
        assert!(json.contains("\"previous_running_daemon_version\""));
    }

    #[test]
    fn test_upgrade_up_to_date_result_serializes() {
        let json = serde_json::to_string(&UpgradeResult::UpToDate {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: "/tmp/test.plist".into(),
            binary_path: "/tmp/rusty-jack".into(),
            binary_version: BinaryVersion {
                version: "0.1.0".into(),
                commit: "abc1234".into(),
            },
            was_loaded: true,
        })
        .unwrap();
        assert!(json.contains("\"status\":\"up_to_date\""));
        assert!(json.contains("\"was_loaded\":true"));
    }

    #[test]
    fn test_upgrade_pauses_only_running_daemon() {
        assert!(should_stop_before_write(LoadMode::Upgrade, true, true));
        assert!(!should_stop_before_write(LoadMode::Upgrade, true, false));
        assert!(should_load_after_write(LoadMode::Upgrade, true, true));
        assert!(!should_load_after_write(LoadMode::Upgrade, true, false));
    }

    #[test]
    fn test_daemon_upgrade_is_current_when_plist_binary_and_version_match() {
        let version = BinaryVersion {
            version: "0.1.0".into(),
            commit: "abc1234".into(),
        };
        let plist = render_launch_agent_plist("/tmp/rusty-jack", "/Users/example", &version);

        assert!(daemon_upgrade_is_current(
            false,
            Some(&plist),
            &plist,
            Some("/tmp/rusty-jack"),
            Some(&version),
            "/tmp/rusty-jack",
            &version,
        ));
    }

    #[test]
    fn test_daemon_upgrade_force_bypasses_current_check() {
        let version = BinaryVersion {
            version: "0.1.0".into(),
            commit: "abc1234".into(),
        };
        let plist = render_launch_agent_plist("/tmp/rusty-jack", "/Users/example", &version);

        assert!(!daemon_upgrade_is_current(
            true,
            Some(&plist),
            &plist,
            Some("/tmp/rusty-jack"),
            Some(&version),
            "/tmp/rusty-jack",
            &version,
        ));
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
    fn test_picker_pause_reason_serializes() {
        let reason = DaemonPauseReason::picker_override(
            "builtin".into(),
            "Built-in Output".into(),
            Some("hdmi-1".into()),
        );
        let json = serde_json::to_string(&PauseResult::Paused {
            label: LAUNCH_AGENT_LABEL.into(),
            plist_path: "/tmp/test.plist".into(),
            was_loaded: true,
            pause_reason: Some(reason.clone()),
        })
        .unwrap();

        assert!(json.contains("\"status\":\"paused\""));
        assert!(json.contains("\"pause_reason\""));
        assert!(json.contains("\"kind\":\"picker_override\""));
        assert!(reason.message().contains("rusty-jack resume"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_gui_domain_looks_valid() {
        let domain = gui_domain().unwrap();
        assert!(domain.starts_with("gui/"));
    }
}
