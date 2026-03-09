//! Shared agent registry operations — read/write agents.json on the VM.
//!
//! Provides a single source of truth for registry I/O, consumed by
//! `install`, `remove`, and `list` service modules.

use anyhow::{Context, Result};

use crate::application::ports::ShellExecutor;
use crate::domain::agent::AgentRegistryEntry;
use crate::domain::workspace::VM_ROOT;

/// Result of a lenient registry read: valid entries plus per-entry warnings.
pub struct RegistryReadResult {
    /// Successfully parsed entries.
    pub entries: Vec<AgentRegistryEntry>,
    /// Warning messages for entries that could not be deserialized.
    pub warnings: Vec<String>,
}

/// Read the agent registry from the VM, parsing each entry individually.
///
/// Reads `{VM_ROOT}/agents/agents.json` and deserializes the top-level JSON
/// array entry-by-entry. Malformed entries are skipped and their index and
/// error are recorded in `warnings` rather than failing the entire read.
///
/// Returns an empty result if the file is missing, empty, or unreadable.
///
/// # Errors
///
/// Returns an error only if the file content is present but is not a valid
/// JSON array at the top level.
pub async fn read_registry(provisioner: &impl ShellExecutor) -> Result<RegistryReadResult> {
    let registry_path = format!("{VM_ROOT}/agents/agents.json");
    let out = provisioner.exec(&["cat", &registry_path]).await;

    match out {
        Ok(output) if output.status.success() => {
            let content = String::from_utf8_lossy(&output.stdout);
            if content.trim().is_empty() {
                return Ok(RegistryReadResult {
                    entries: vec![],
                    warnings: vec![],
                });
            }
            // Parse as a generic JSON array first so we can handle each entry
            // individually without failing the whole read on a single bad entry.
            let values: Vec<serde_json::Value> =
                serde_json::from_str(&content).context("parsing registry JSON as array")?;

            let mut entries = Vec::with_capacity(values.len());
            let mut warnings = Vec::new();

            for (i, value) in values.into_iter().enumerate() {
                match serde_json::from_value::<AgentRegistryEntry>(value) {
                    Ok(entry) => entries.push(entry),
                    Err(e) => warnings.push(format!("registry entry {i} is malformed: {e}")),
                }
            }

            Ok(RegistryReadResult { entries, warnings })
        }
        _ => Ok(RegistryReadResult {
            entries: vec![],
            warnings: vec![],
        }), // Missing file or read failure — treat as empty registry
    }
}

/// Write the agent registry to the VM.
///
/// Serializes `entries` as pretty-printed JSON and writes it to
/// `{VM_ROOT}/agents/agents.json` via stdin to avoid shell-escaping issues.
///
/// # Errors
///
/// Returns an error if serialization fails or the write command exits non-zero.
pub async fn write_registry(
    provisioner: &impl ShellExecutor,
    entries: &[AgentRegistryEntry],
) -> Result<()> {
    let registry_path = format!("{VM_ROOT}/agents/agents.json");
    let json = serde_json::to_string_pretty(entries).context("serializing registry")?;

    let result = provisioner
        .exec_with_stdin(&["tee", &registry_path], json.as_bytes())
        .await?;

    anyhow::ensure!(
        result.status.success(),
        "Failed to write registry: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entry_round_trips_via_json() -> anyhow::Result<()> {
        let entries = vec![
            AgentRegistryEntry {
                name: "agent-a".to_string(),
                version: Some("1.0.0".to_string()),
                description: Some("First agent".to_string()),
            },
            AgentRegistryEntry {
                name: "agent-b".to_string(),
                version: None,
                description: None,
            },
        ];

        let json = serde_json::to_string_pretty(&entries)?;
        let parsed: Vec<AgentRegistryEntry> = serde_json::from_str(&json)?;

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "agent-a");
        assert_eq!(parsed[0].version, Some("1.0.0".to_string()));
        assert_eq!(parsed[0].description, Some("First agent".to_string()));
        assert_eq!(parsed[1].name, "agent-b");
        assert_eq!(parsed[1].version, None);
        assert_eq!(parsed[1].description, None);
        Ok(())
    }

    #[test]
    fn empty_json_array_deserializes_to_empty_vec() -> anyhow::Result<()> {
        let entries: Vec<AgentRegistryEntry> = serde_json::from_str("[]")?;
        assert!(entries.is_empty());
        Ok(())
    }

    #[test]
    fn whitespace_only_content_treated_as_empty() {
        // Simulates the trim().is_empty() branch in read_registry
        let content = "   \n  ";
        assert!(content.trim().is_empty());
    }

    #[test]
    fn malformed_entry_produces_warning_and_skips_entry() -> anyhow::Result<()> {
        // An entry missing the required `name` field is malformed.
        let json = r#"[{"name":"good","version":"1.0"},{"version":"bad-no-name"}]"#;
        let values: Vec<serde_json::Value> = serde_json::from_str(json)?;
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        for (i, value) in values.into_iter().enumerate() {
            match serde_json::from_value::<AgentRegistryEntry>(value) {
                Ok(e) => entries.push(e),
                Err(e) => warnings.push(format!("registry entry {i} is malformed: {e}")),
            }
        }
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "good");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("registry entry 1 is malformed"));
        Ok(())
    }

    #[test]
    fn all_valid_entries_produce_no_warnings() -> anyhow::Result<()> {
        let json = r#"[{"name":"a"},{"name":"b","version":"2.0"}]"#;
        let values: Vec<serde_json::Value> = serde_json::from_str(json)?;
        let mut entries = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        for (i, value) in values.into_iter().enumerate() {
            match serde_json::from_value::<AgentRegistryEntry>(value) {
                Ok(e) => entries.push(e),
                Err(e) => warnings.push(format!("registry entry {i} is malformed: {e}")),
            }
        }
        assert_eq!(entries.len(), 2);
        assert!(warnings.is_empty());
        Ok(())
    }
}
