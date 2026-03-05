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
    /// Warning message if the agent's manifest was malformed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Entry in the agents.json registry file on the VM.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentRegistryEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Domain decision for agent activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAction {
    /// Agent is not active — proceed with activation.
    Activate { agent: String },
    /// Agent is already active — no action needed.
    AlreadyActive { agent: String },
    /// A different agent is active — cannot activate without removing it first.
    Mismatch { active: String, requested: String },
}

/// Determine what action to take when a user requests agent activation.
///
/// Pure function: no I/O, no async, no side effects.
#[must_use]
pub fn resolve_agent_action(requested: &str, persisted: Option<&WorkspaceState>) -> AgentAction {
    match persisted.and_then(|s| s.active_agent.as_deref()) {
        None => AgentAction::Activate {
            agent: requested.to_string(),
        },
        Some(active) if active == requested => AgentAction::AlreadyActive {
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
    fn resolve_agent_action_no_state_returns_activate() {
        let action = resolve_agent_action("openclaw", None);
        assert_eq!(
            action,
            AgentAction::Activate {
                agent: "openclaw".to_string()
            }
        );
    }

    #[test]
    fn resolve_agent_action_no_active_agent_returns_activate() {
        let state = state_with_agent(None);
        let action = resolve_agent_action("openclaw", Some(&state));
        assert_eq!(
            action,
            AgentAction::Activate {
                agent: "openclaw".to_string()
            }
        );
    }

    #[test]
    fn resolve_agent_action_same_agent_returns_already_active() {
        let state = state_with_agent(Some("openclaw"));
        let action = resolve_agent_action("openclaw", Some(&state));
        assert_eq!(
            action,
            AgentAction::AlreadyActive {
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

    // Swap case tests (Req 14.1, 14.3, 14.4)

    #[test]
    fn resolve_agent_action_swap_from_openclaw_to_coder() {
        let state = state_with_agent(Some("openclaw"));
        let action = resolve_agent_action("coder", Some(&state));
        assert_eq!(
            action,
            AgentAction::Mismatch {
                active: "openclaw".to_string(),
                requested: "coder".to_string(),
            }
        );
    }

    #[test]
    fn resolve_agent_action_swap_from_coder_to_openclaw() {
        let state = state_with_agent(Some("coder"));
        let action = resolve_agent_action("openclaw", Some(&state));
        assert_eq!(
            action,
            AgentAction::Mismatch {
                active: "coder".to_string(),
                requested: "openclaw".to_string(),
            }
        );
    }

    #[test]
    fn resolve_agent_action_swap_preserves_agent_names_exactly() {
        // Ensure agent names are preserved exactly as provided
        let state = state_with_agent(Some("my-custom-agent"));
        let action = resolve_agent_action("another-agent", Some(&state));
        match action {
            AgentAction::Mismatch { active, requested } => {
                assert_eq!(active, "my-custom-agent");
                assert_eq!(requested, "another-agent");
            }
            _ => panic!("Expected Mismatch variant"),
        }
    }

    #[test]
    fn resolve_agent_action_mismatch_is_symmetric_in_detection() {
        // Swapping A→B and B→A both return Mismatch (though with different active/requested)
        let state_a = state_with_agent(Some("agent-a"));
        let state_b = state_with_agent(Some("agent-b"));

        let action_a_to_b = resolve_agent_action("agent-b", Some(&state_a));
        let action_b_to_a = resolve_agent_action("agent-a", Some(&state_b));

        assert!(matches!(action_a_to_b, AgentAction::Mismatch { .. }));
        assert!(matches!(action_b_to_a, AgentAction::Mismatch { .. }));
    }
}
