//! Application services — use-case orchestration.
//!
//! Each service module implements a single use-case by composing domain logic
//! with port trait calls. Services import only from `crate::domain` and
//! `crate::application::ports` — never from `crate::infra`, `crate::commands`,
//! or `crate::output`.

pub mod agent;
pub mod security;
pub mod workspace;
pub mod ssh;
pub mod update;
