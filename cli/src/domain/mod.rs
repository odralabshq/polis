//! Domain layer — pure business logic, types, and validation.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, `std::process`, or `std::net`.
//! All functions are synchronous and take data in, returning data out.

pub mod agent;
pub mod config;
pub mod error;
pub mod health;
pub mod workspace;

#[allow(unused_imports)]
pub use config::{PolisConfig, SecurityConfig, validate_config_key, validate_config_value};
#[allow(unused_imports)]
pub use error::{AgentError, ConfigError, WorkspaceError};
#[allow(unused_imports)]
pub use health::{
    DoctorChecks, ImageCheckResult, NetworkChecks, PrerequisiteChecks, SecurityChecks,
    VersionDrift, WorkspaceChecks, collect_issues,
};
#[allow(unused_imports)]
pub use workspace::{WorkspaceState, check_architecture, validate_workspace_id};
pub mod ssh;
