//! Integration tests for `polis connect`.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("polis"));
    cmd.env("NO_COLOR", "1");
    cmd
}

#[test]
fn test_connect_help_is_accessible() {
    polis().args(["connect", "--help"]).assert().success();
}

#[test]
fn test_connect_help_does_not_mention_ide() {
    polis()
        .args(["connect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--ide").not());
}

#[test]
fn test_connect_ide_flag_is_rejected() {
    // --ide was removed; clap must reject it as an unknown argument.
    polis()
        .args(["connect", "--ide", "vscode"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--ide").or(predicate::str::contains("unexpected")));
}
