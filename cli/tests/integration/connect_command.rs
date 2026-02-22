//! Integration tests for `polis connect` (issue 13).

#![allow(clippy::expect_used)]

use assert_cmd::Command;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
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
        .failure();
}
