//! Domain logic for agent management — pure functions, no I/O, no async.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, `std::process`, or `std::net`.

pub mod artifacts;
pub mod validate;

#[allow(unused_imports)]
pub use artifacts::{compose_overlay, filtered_env, service_hash, systemd_unit};
#[allow(unused_imports)]
pub use validate::{
    AGENT_NAME_RE, ALLOWED_RW_PREFIXES, PLATFORM_PORTS, SHELL_METACHAR_RE, is_valid_agent_name,
    validate_full_manifest,
};
/// Information about an installed agent.
#[derive(Debug, serde::Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub active: bool,
}
