//! Infrastructure layer — concrete implementations of application port traits.
//!
//! This module contains all I/O-performing code: process execution, filesystem
//! access, VM provisioning, SSH management, and asset extraction.
//!
//! **Dependency rule:** Imports from `crate::domain` and `crate::application::ports`
//! are allowed. Imports from `crate::commands` or `crate::output` are forbidden.

pub mod assets;
pub mod blocking;
pub mod command_runner;
pub mod config;
pub mod fs;
pub mod network;
pub mod polis_dir;
pub mod process;
pub mod provisioner;
pub mod secure_fs;
pub mod security_gateway;
pub mod ssh;
pub mod state;
pub mod update;
