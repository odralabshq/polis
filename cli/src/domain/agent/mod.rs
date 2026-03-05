//! Domain logic for agent management — pure functions, no I/O, no async.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, `std::process`, or `std::net`.

pub mod artifacts;
pub mod validate;

use crate::domain::workspace::WorkspaceState;

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

/// Domain decision for agent activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAction {
    /// Agent is not installed — proceed with installation.
    Install { agent: String },
    /// Agent is already active — no action needed.
    AlreadyInstalled { agent: String },
    /// A different agent is active — cannot install without removing it first.
    Mismatch { active: String, requested: String },
}

/// Determine what action to take when a user requests agent activation.
///
/// Pure function: no I/O, no async, no side effects.
#[must_use]
pub fn resolve_agent_action(requested: &str, persisted: Option<&WorkspaceState>) -> AgentAction {
    match persisted.and_then(|s| s.active_agent.as_deref()) {
        None => AgentAction::Install {
            agent: requested.to_string(),
        },
        Some(active) if active == requested => AgentAction::AlreadyInstalled {
            agent: requested.to_string(),
        },
        Some(active) => AgentAction::Mismatch {
            active: active.to_string(),
            requested: requested.to_string(),
        },
    }
}

/// Returns the path to an agent's compose overlay file inside the VM.
#[must_use]
pub fn overlay_path(agent_name: &str) -> String {
    format!(
        "{}/agents/{agent_name}/.generated/compose.agent.yaml",
        super::workspace::VM_ROOT
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::workspace::WorkspaceState;
    use chrono::Utc;

    fn state_with_agent(agent: Option<&str>) -> WorkspaceState {
        WorkspaceState {
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: agent.map(str::to_string),
            provisioning: None,
        }
    }

    #[test]
    fn resolve_agent_action_no_state_returns_install() {
        let action = resolve_agent_action("openclaw", None);
        assert_eq!(
            action,
            AgentAction::Install {
                agent: "openclaw".to_string()
            }
        );
    }

    #[test]
    fn resolve_agent_action_no_active_agent_returns_install() {
        let state = state_with_agent(None);
        let action = resolve_agent_action("openclaw", Some(&state));
        assert_eq!(
            action,
            AgentAction::Install {
                agent: "openclaw".to_string()
            }
        );
    }

    #[test]
    fn resolve_agent_action_same_agent_returns_already_installed() {
        let state = state_with_agent(Some("openclaw"));
        let action = resolve_agent_action("openclaw", Some(&state));
        assert_eq!(
            action,
            AgentAction::AlreadyInstalled {
                agent: "openclaw".to_string()
            }
        );
    }

    #[test]
    fn resolve_agent_action_different_agent_returns_mismatch() {
        let state = state_with_agent(Some("openclaw"));
        let action = resolve_agent_action("other", Some(&state));
        assert_eq!(
            action,
            AgentAction::Mismatch {
                active: "openclaw".to_string(),
                requested: "other".to_string(),
            }
        );
    }
}
