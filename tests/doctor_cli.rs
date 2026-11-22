//! End-to-end `rusty-jack doctor` exit-status tests.
//!
//! Hermetic: `HOME` points at a temp dir holding a fake LaunchAgent plist (so the daemon's
//! `RUST_LOG=info` is known), a fake daemon log, and a config, so no real daemon, log, or
//! macOS permission state is consulted for the verdict under test.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ProgramArguments</key><array><string>/opt/test/rusty-jack</string><string>daemon</string></array>
  <key>EnvironmentVariables</key><dict><key>RUST_LOG</key><string>info</string></dict>
</dict></plist>
"#;

fn write_config(dir: &Path, activity_monitor: &str) -> std::path::PathBuf {
    let path = dir.join("config.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"version":1,"auto_switch":true,"preferred_device":{{"uid":"hdmi"}},"fallback_uids":[],"activity_monitor":"{activity_monitor}"}}"#
        ),
    )
    .unwrap();
    path
}

fn doctor(home: &Path, config: &Path, log: &Path) -> Command {
    let agents = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("com.example.rusty-jack.plist"), PLIST).unwrap();
    let mut cmd = Command::cargo_bin("rusty-jack").unwrap();
    cmd.env("HOME", home)
        .env("RUSTY_JACK_LOG_FILE", log)
        .env_remove("RUSTY_JACK_CONFIG")
        .env_remove("HDMI_SOUND_CONTROLLER_CONFIG")
        .args(["--config", config.to_str().unwrap(), "doctor", "--json"]);
    cmd
}

#[test]
fn test_doctor_fails_when_daemon_accessibility_missing() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(dir.path(), "event_tap");
    let log = dir.path().join("daemon.log");
    std::fs::write(
        &log,
        "20261122-10:45:16.336 | WARN     | daemon       | [privacy] startup: Accessibility permission missing (required for activity_monitor=event_tap)\n",
    )
    .unwrap();
    doctor(dir.path(), &config, &log)
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"state\": \"fail\""));
}

#[test]
fn test_doctor_succeeds_when_daemon_accessibility_granted() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(dir.path(), "event_tap");
    let log = dir.path().join("daemon.log");
    std::fs::write(
        &log,
        "20261122-10:54:17.521 | INFO     | daemon       | [activity] event tap active (listen-only)\n\
         20261122-10:54:19.051 | INFO     | daemon       | [privacy] startup: Accessibility permission looks granted\n",
    )
    .unwrap();
    doctor(dir.path(), &config, &log)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"accessibility_daemon\""))
        .stdout(predicate::str::contains("\"state\": \"fail\"").not());
}

#[test]
fn test_doctor_succeeds_with_warnings_only_when_no_verdict_logged() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(dir.path(), "event_tap");
    let log = dir.path().join("empty.log");
    std::fs::write(&log, "").unwrap();
    doctor(dir.path(), &config, &log)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"state\": \"warn\""));
}

#[test]
fn test_doctor_succeeds_and_skips_everything_for_idle_monitor() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(dir.path(), "idle");
    let log = dir.path().join("none.log");
    doctor(dir.path(), &config, &log)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"state\": \"skipped\""))
        .stdout(predicate::str::contains("\"state\": \"fail\"").not());
}
