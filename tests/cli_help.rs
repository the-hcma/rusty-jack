//! CLI smoke tests (help / parse); list requires macOS CoreAudio.

use assert_cmd::Command;
use predicates::prelude::*;

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
fn test_apply_subcommand_in_help() {
    Command::cargo_bin("rusty-jack")
        .unwrap()
        .arg("apply")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("policy"));
}
