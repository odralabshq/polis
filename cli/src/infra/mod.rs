//! Infrastructure layer — concrete implementations of application port traits.
//!
//! This module contains all I/O-performing code: process execution, filesystem
//! access, VM provisioning, SSH management, and asset extraction.
//!
//! Imports from `crate::domain` and `crate::application::ports` are allowed.
//! Imports from `crate::commands` or `crate::output` are forbidden.

#![allow(dead_code)] // Refactor in progress — infra defined ahead of callers

pub mod assets;
pub mod command_runner;
pub mod fs;
pub mod provisioner;
pub mod ssh;
pub mod state;
