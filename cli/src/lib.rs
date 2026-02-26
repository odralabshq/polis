//! Polis CLI library — exposes modules for integration testing.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod assets;
pub mod commands;
pub mod multipass;
pub mod output;
pub mod ssh;
pub mod state;
pub mod workspace;
