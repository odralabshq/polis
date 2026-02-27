//! Polis CLI library — exposes modules for integration testing.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod assets;
pub mod command_runner;
pub mod commands;
pub mod multipass;
pub mod output;
pub mod provisioner;
pub mod ssh;
pub mod state;
pub mod workspace;
