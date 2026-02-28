//! Unit tests for polis CLI
//!
//! These tests use mocked dependencies and run fast without external I/O.

pub mod mocks;

mod agent_command;
mod agent_crud_service;
mod architecture;
mod config_command;
mod doctor_command;
mod doctor_service;
mod helpers;
mod output;
mod property_tests;
mod provisioner_tests;
mod start_command;
mod start_stop_delete;
mod status_command;
mod workspace_start_service;
