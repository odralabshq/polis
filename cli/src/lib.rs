//! Polis CLI library — exposes modules for integration testing.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod commands;
pub mod output;
pub mod ssh;
pub mod state;
pub mod workspace;
