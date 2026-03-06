//! Polis CLI library — exposes modules for integration testing.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod app;
pub mod application;
pub mod cli;
pub mod commands;
pub mod domain;
pub mod infra;
pub mod output;

#[cfg(test)]
pub mod test_utils;
