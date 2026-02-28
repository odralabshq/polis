//! Application services — use-case orchestration.
//!
//! Each service module implements a single use-case by composing domain logic
//! with port trait calls. Services import only from `crate::domain` and
//! `crate::application::ports` — never from `crate::infra`, `crate::commands`,
//! or `crate::output`.

pub mod agent_crud;
pub mod update;
pub mod vm;
pub mod workspace_doctor;
pub mod workspace_start;
pub mod workspace_status;
