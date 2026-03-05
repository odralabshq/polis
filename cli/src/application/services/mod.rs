//! Application services — use-case orchestration.
//!
//! Each service module implements a single use-case by composing domain logic
//! with port trait calls. Services import only from `crate::domain` and
//! `crate::application::ports` — never from `crate::infra`, `crate::commands`,
//! or `crate::output`.

pub mod agent;
pub mod cleanup_service;
pub mod config_service;
pub mod provisioning;
pub mod security_service;
pub mod ssh_provision;
pub mod update;
pub mod vm;
pub mod workspace_doctor;
pub mod workspace_repair;
pub mod workspace_start;
pub mod workspace_status;
pub mod workspace_stop;
