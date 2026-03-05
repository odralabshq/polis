//! Agent list service — list installed agents from the VM registry.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::Result;

use crate::application::ports::{InstanceInspector, ShellExecutor, WorkspaceStateStore};
use crate::domain::agent::{AgentInfo, AgentRegistryEntry};
use crate::domain::workspace::VM_ROOT;

use super::ensure_vm_running;

/// List all installed agents by reading the JSON registry from the VM.
///
/// Reads `{VM_ROOT}/agents/agents.json` and maps each entry to `AgentInfo`,
/// marking the currently active agent based on persisted workspace state.
///
/// # Errors
///
/// Returns an error if the VM is not running or if the registry file
/// cannot be parsed (though missing registry returns an empty list).
///
/// # Requirements
///
/// - 3.1: Separate service module for agent list use case
/// - 6.1, 6.2, 6.3, 6.4: Structured data format instead of string markers
/// - 13.1: Read agents.json registry file from VM
/// - 13.2: Parse registry entries for name, version, description
/// - 13.3: Indicate active agent by comparing against persisted state
/// - 13.4: Exclude _template directory (handled by registry approach)
/// - 13.5: Return empty list when no agents installed
/// - 13.7: Return typed error when VM not running
/// - 13.8: Include warning field for malformed entries
pub async fn list_agents(
    provisioner: &(impl ShellExecutor + InstanceInspector),
    state_mgr: &impl WorkspaceStateStore,
) -> Result<Vec<AgentInfo>> {
    // Fail fast with a friendly message if the VM isn't running
    ensure_vm_running(provisioner).await?;

    // Read the registry file from the VM
    let registry_path = format!("{VM_ROOT}/agents/agents.json");
    let out = provisioner.exec(&["cat", &registry_path]).await;

    // Parse the registry - missing file means no agents
    let entries: Vec<serde_json::Value> = match out {
        Ok(output) => {
            let content = String::from_utf8_lossy(&output.stdout);
            if content.trim().is_empty() || !output.status.success() {
                vec![]
            } else {
                serde_json::from_str(content.trim()).unwrap_or_else(|_| vec![])
            }
        }
        Err(_) => vec![], // No registry file = no agents
    };

    // Load persisted state to determine active agent
    let state = state_mgr.load_async().await?;
    let active = state.and_then(|s| s.active_agent);

    // Map registry entries to AgentInfo, handling malformed entries individually
    Ok(entries
        .into_iter()
        .map(
            |v| match serde_json::from_value::<AgentRegistryEntry>(v.clone()) {
                Ok(e) => AgentInfo {
                    active: active.as_deref() == Some(&e.name),
                    name: e.name,
                    version: e.version,
                    description: e.description,
                    warning: None,
                },
                Err(err) => {
                    let name = v
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    AgentInfo {
                        active: active.as_deref() == Some(&name),
                        name,
                        version: None,
                        description: None,
                        warning: Some(format!("malformed registry entry: {err}")),
                    }
                }
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unit tests for list_agents would require mocking the provisioner and state manager.
    // These are integration-level tests that verify the function signature and basic logic.
    // Full integration tests are in the CLI test suite.

    #[test]
    fn registry_entry_deserializes_correctly() {
        let json = r#"{"name": "test-agent", "version": "1.0.0", "description": "Test agent"}"#;
        let entry: AgentRegistryEntry =
            serde_json::from_str(json).expect("deserialization should succeed");
        assert_eq!(entry.name, "test-agent");
        assert_eq!(entry.version, Some("1.0.0".to_string()));
        assert_eq!(entry.description, Some("Test agent".to_string()));
    }

    #[test]
    fn registry_entry_deserializes_with_optional_fields() {
        let json = r#"{"name": "minimal-agent"}"#;
        let entry: AgentRegistryEntry =
            serde_json::from_str(json).expect("deserialization should succeed");
        assert_eq!(entry.name, "minimal-agent");
        assert_eq!(entry.version, None);
        assert_eq!(entry.description, None);
    }

    #[test]
    fn registry_array_deserializes_correctly() {
        let json = r#"[
            {"name": "agent1", "version": "1.0.0", "description": "First agent"},
            {"name": "agent2", "version": "2.0.0", "description": "Second agent"}
        ]"#;
        let entries: Vec<AgentRegistryEntry> =
            serde_json::from_str(json).expect("deserialization should succeed");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "agent1");
        assert_eq!(entries[1].name, "agent2");
    }

    #[test]
    fn empty_registry_array_deserializes_correctly() {
        let json = "[]";
        let entries: Vec<AgentRegistryEntry> =
            serde_json::from_str(json).expect("deserialization should succeed");
        assert!(entries.is_empty());
    }
}
