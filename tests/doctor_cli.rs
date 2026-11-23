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
    write_config_with(dir, activity_monitor, false)
}

/// `auto_switch: true` means the daemon is expected to keep audio routed, so a missing or
/// paused daemon is a failure; with it off, daemon problems are only warnings.
fn write_config_with(dir: &Path, activity_monitor: &str, auto_switch: bool) -> std::path::PathBuf {
    let path = dir.join("config.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"version":1,"auto_switch":{auto_switch},"preferred_device":{{"uid":"hdmi"}},"fallback_uids":[],"activity_monitor":"{activity_monitor}"}}"#
        ),
    )
    .unwrap();
    path
}

fn doctor(home: &Path, config: &Path, log: &Path) -> Command {
    doctor_with(home, config, log, true)
}

fn doctor_with(home: &Path, config: &Path, log: &Path, json: bool) -> Command {
    let agents = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("com.example.rusty-jack.plist"), PLIST).unwrap();
    build_doctor(home, config, log, json)
}

/// Like [`doctor`], but with no LaunchAgent plist: the daemon is not installed.
fn doctor_without_daemon(home: &Path, config: &Path, log: &Path) -> Command {
    build_doctor(home, config, log, true)
}

fn build_doctor(home: &Path, config: &Path, log: &Path, json: bool) -> Command {
    let mut cmd = Command::cargo_bin("rusty-jack").unwrap();
    // `false` stands in for launchctl: a plist exists but the job is never loaded, so the
    // daemon reads as paused and the real launchd (and the machine's real daemon) is never used.
    cmd.env("HOME", home)
        .env("RUSTY_JACK_LOG_FILE", log)
        .env("RUSTY_JACK_LAUNCHCTL", "/usr/bin/false")
        .env_remove("RUSTY_JACK_CONFIG")
        .env_remove("HDMI_SOUND_CONTROLLER_CONFIG")
        .args(["--config", config.to_str().unwrap(), "doctor"]);
    if json {
        cmd.arg("--json");
    }
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

#[test]
fn test_doctor_never_rewrites_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    // Valid but non-canonical (unsorted keys, compact): `load_config` would rewrite it.
    let config = write_config(dir.path(), "idle");
    let original = std::fs::read(&config).unwrap();

    // Precondition: the rewriting loader really does change this file.
    let probe = dir.path().join("probe.json");
    std::fs::write(&probe, &original).unwrap();
    rusty_jack::config::load_config(&probe).unwrap();
    assert_ne!(
        std::fs::read(&probe).unwrap(),
        original,
        "sample must be non-canonical"
    );

    let log = dir.path().join("none.log");
    doctor(dir.path(), &config, &log).assert().success();
    assert_eq!(
        std::fs::read(&config).unwrap(),
        original,
        "doctor rewrote the config"
    );
}

#[test]
fn test_doctor_fails_when_auto_routing_is_configured_but_the_daemon_is_paused() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config_with(dir.path(), "idle", true);
    let log = dir.path().join("none.log");
    doctor(dir.path(), &config, &log)
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"id\": \"daemon_state\""))
        .stdout(predicate::str::contains("\"state\": \"fail\""))
        .stdout(predicate::str::contains("rusty-jack resume"));
}

#[test]
fn test_doctor_fails_and_suggests_install_when_no_daemon_is_installed() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config_with(dir.path(), "idle", true);
    let log = dir.path().join("none.log");
    doctor_without_daemon(dir.path(), &config, &log)
        .assert()
        .failure()
        .stdout(predicate::str::contains("LaunchAgent is not installed"))
        .stdout(predicate::str::contains("rusty-jack install"));
}

#[test]
fn test_doctor_only_warns_about_a_paused_daemon_without_auto_switch() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config_with(dir.path(), "idle", false);
    let log = dir.path().join("none.log");
    doctor(dir.path(), &config, &log)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"daemon_state\""))
        .stdout(predicate::str::contains("\"state\": \"fail\"").not());
}

#[test]
fn test_doctor_json_includes_the_daemon_routing_and_volume_checks() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(dir.path(), "idle");
    let log = dir.path().join("none.log");
    let assert = doctor(dir.path(), &config, &log).assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    for id in [
        "daemon_state",
        "daemon_binary",
        "eqmac_readiness",
        "routing",
        "daemon_log",
    ] {
        assert!(
            out.contains(&format!("\"id\": \"{id}\"")),
            "missing {id} in {out}"
        );
    }
}

#[test]
fn test_doctor_text_ends_with_an_overall_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("none.log");

    let failing = write_config_with(dir.path(), "idle", true);
    doctor_with(dir.path(), &failing, &log, false)
        .assert()
        .failure()
        .stdout(predicate::str::contains("Overall: problems found"));

    let quiet = write_config(dir.path(), "idle");
    doctor_with(dir.path(), &quiet, &log, false)
        .assert()
        .success()
        .stdout(predicate::str::contains("Overall:"));
}
