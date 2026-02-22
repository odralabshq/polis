//! Integration tests for `polis connect` (issue 13).

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
fn test_connect_unknown_ide_exits_with_error() {
    polis()
        .args(["connect", "--ide", "emacs"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("emacs").or(predicate::str::contains("invalid")));
}
