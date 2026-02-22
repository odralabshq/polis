//! Unit tests for polis CLI
//!
//! These tests use mocked dependencies and run fast without external I/O.

pub mod mocks;

mod config_command;
mod container_update;
mod doctor_command;
mod output;
mod property_tests;
mod start_stop_delete;
mod status_command;
