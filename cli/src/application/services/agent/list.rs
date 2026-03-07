//! Agent list service — list installed agents from the VM registry.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::Result;

use crate::application::ports::{InstanceInspector, ShellExecutor, WorkspaceStateStore};
use crate::domain::agent::AgentInfo;

use super::ensure_vm_running;

/// List all installed agents by reading the JSON registry from the VM.
///
/// Delegates registry I/O to [`super::registry::read_registry`] and maps each
/// entry to `AgentInfo`, marking the currently active agent based on persisted
/// workspace state.
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
/// - 16.6: Use shared registry module for registry reads
pub async fn list_agents(
    provisioner: &(impl ShellExecutor + InstanceInspector),
    state_mgr: &impl WorkspaceStateStore,
) -> Result<Vec<AgentInfo>> {
    // Fail fast with a friendly message if the VM isn't running
    ensure_vm_running(provisioner).await?;

    // Read the registry via the shared registry module
    let result = super::registry::read_registry(provisioner).await?;

    // Load persisted state to determine active agent
    let state = state_mgr.load_async().await?;
    let active = state.and_then(|s| s.active_agent);

    // Map valid registry entries to AgentInfo; include a synthetic warning entry
    // for each malformed entry so callers can surface the issue to the user.
    let mut agents: Vec<AgentInfo> = result
        .entries
        .into_iter()
        .map(|e| AgentInfo {
            active: active.as_deref() == Some(&e.name),
            name: e.name,
            version: e.version,
            description: e.description,
            warning: None,
        })
        .collect();

    for warning in result.warnings {
        agents.push(AgentInfo {
            active: false,
            name: "<malformed>".to_string(),
            version: None,
            description: None,
            warning: Some(warning),
        });
    }

    Ok(agents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent::AgentRegistryEntry;

    // Unit tests for list_agents would require mocking the provisioner and state manager.
    // These are integration-level tests that verify the function signature and basic logic.
    // Full integration tests are in the CLI test suite.

    #[test]
    fn agent_info_maps_from_registry_entry() {
        let entry = AgentRegistryEntry {
            name: "test-agent".to_string(),
            version: Some("1.0.0".to_string()),
            description: Some("Test agent".to_string()),
        };
        let active: Option<String> = Some("test-agent".to_string());
        let info = AgentInfo {
            active: active.as_deref() == Some(&entry.name),
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            warning: None,
        };
        assert!(info.active);
        assert_eq!(info.name, "test-agent");
        assert_eq!(info.version, Some("1.0.0".to_string()));
        assert_eq!(info.description, Some("Test agent".to_string()));
        assert!(info.warning.is_none());
    }

    #[test]
    fn inactive_agent_marked_correctly() {
        let entry = AgentRegistryEntry {
            name: "other-agent".to_string(),
            version: None,
            description: None,
        };
        let active: Option<String> = Some("active-agent".to_string());
        let info = AgentInfo {
            active: active.as_deref() == Some(&entry.name),
            name: entry.name.clone(),
            version: entry.version,
            description: entry.description,
            warning: None,
        };
        assert!(!info.active);
    }

    // ── list_agents service tests ─────────────────────────────────────────

    use std::process::Output;
    use crate::application::ports::{InstanceInspector, ShellExecutor, WorkspaceStateStore};
    use crate::application::vm::test_support::{impl_shell_executor_stubs, ok_output, fail_output, StateStoreStub};
    use crate::domain::workspace::WorkspaceState;

    struct ListStub {
        info_running: bool,
        registry_json: &'static [u8],
    }

    impl InstanceInspector for ListStub {
        async fn info(&self) -> anyhow::Result<Output> {
            if self.info_running {
                Ok(ok_output(br#"{"info":{"polis":{"state":"Running","ipv4":[]}}}"#))
            } else {
                Ok(fail_output())
            }
        }
        async fn version(&self) -> anyhow::Result<Output> { anyhow::bail!("not expected") }
    }

    impl ShellExecutor for ListStub {
        async fn exec(&self, _: &[&str]) -> anyhow::Result<Output> {
            Ok(ok_output(self.registry_json))
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    #[tokio::test]
    async fn list_agents_vm_not_running_returns_error() {
        let stub = ListStub { info_running: false, registry_json: b"[]" };
        let store = StateStoreStub::empty();
        assert!(list_agents(&stub, &store).await.is_err());
    }

    #[tokio::test]
    async fn list_agents_empty_registry() {
        let stub = ListStub { info_running: true, registry_json: b"[]" };
        let store = StateStoreStub::empty();
        let agents = list_agents(&stub, &store).await.unwrap();
        assert!(agents.is_empty());
    }

    #[tokio::test]
    async fn list_agents_marks_active_agent() {
        let stub = ListStub {
            info_running: true,
            registry_json: br#"[{"name":"openclaw","version":"1.0"},{"name":"other"}]"#,
        };
        let mut state = WorkspaceState::default();
        state.active_agent = Some("openclaw".to_string());
        let store = StateStoreStub::with(state);
        let agents = list_agents(&stub, &store).await.unwrap();
        assert_eq!(agents.len(), 2);
        assert!(agents.iter().find(|a| a.name == "openclaw").unwrap().active);
        assert!(!agents.iter().find(|a| a.name == "other").unwrap().active);
    }

    #[tokio::test]
    async fn list_agents_malformed_entry_produces_warning() {
        let stub = ListStub {
            info_running: true,
            registry_json: br#"[{"name":"good"},{"version":"no-name"}]"#,
        };
        let store = StateStoreStub::empty();
        let agents = list_agents(&stub, &store).await.unwrap();
        assert_eq!(agents.len(), 2);
        assert!(agents.iter().any(|a| a.warning.is_some()));
    }
}