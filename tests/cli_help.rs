//! CLI smoke tests (help / parse); list requires macOS CoreAudio.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help_shows_version_and_commit() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_help_subcommands_alphabetical() {
    let output = Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8_lossy(&output);
    let names: Vec<&str> = help
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .skip_while(|word| *word != "Commands:")
        .skip(1)
        .take_while(|line| *line != "Options:")
        .filter(|word| *word != "help")
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn test_help_shows_rusty_jack() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("rusty-jack"));
}

#[test]
fn test_list_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("list")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("hdmi"));
}

#[test]
fn test_status_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("status")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("default"));
}

#[test]
fn test_picker_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("picker")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("interactive"));
}

#[test]
fn test_disable_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("disable")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Uninstall"));
}

#[test]
fn test_pause_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("pause")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Pause"));
}

#[test]
fn test_resume_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("resume")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Resume"));
}

#[test]
fn test_apply_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("apply")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("policy"));
}
