//! Domain layer — pure business logic, types, and validation.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, `std::process`, or `std::net`.
//! All functions are synchronous and take data in, returning data out.

pub mod agent;
pub mod config;
pub mod error;
pub mod health;
pub mod process;
pub mod security;
pub mod ssh;
pub mod util;
pub mod workspace;

#[allow(unused_imports)]
pub use config::{PolisConfig, SecurityConfig, validate_config_key};
#[allow(unused_imports)]
pub use error::{AgentError, ConfigError, WorkspaceError};
#[allow(unused_imports)]
pub use health::{
    CertificateStatus, DiagnosticReport, ImageCheckResult, MalwareDbStatus, NetworkChecks,
    PrerequisiteChecks, SecurityChecks, WorkspaceChecks, collect_issues,
};
#[allow(unused_imports)]
pub use workspace::{WorkspaceState, check_architecture};
