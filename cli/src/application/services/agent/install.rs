//! Agent install service — install an agent from a local path onto the VM.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::{Context, Result};

use crate::application::ports::{
    FileTransfer, InstanceInspector, LocalFs, ProgressReporter, ShellExecutor,
};
use crate::domain::agent::AgentRegistryEntry;
use crate::domain::workspace::VM_ROOT;

use super::artifacts::write_artifacts_to_dir;
use super::ensure_vm_running;
use super::registry::{read_registry, write_registry};

/// Install an agent from a local folder into the VM.
///
/// Steps:
/// 1. Validate the agent folder and manifest (domain validation)
/// 2. Ensure VM is running
/// 3. Check agent doesn't already exist on VM
/// 4. Generate artifacts using domain functions
/// 5. Transfer agent folder to VM via `FileTransfer`
/// 6. Update the agents.json registry on the VM
///
/// If the transfer fails, attempts cleanup by removing the partially-created
/// agent directory from the VM.
///
/// # Errors
///
/// Returns an error if:
/// - The specified path does not exist (Req 11.5)
/// - No agent.yaml manifest found at path (Req 11.6)
/// - Manifest validation fails (Req 11.7)
/// - VM is not running (Req 11.9)
/// - Agent already installed (Req 11.8)
/// - Transfer or artifact generation fails
///
/// # Requirements
///
/// - 3.2: Separate service module for agent install use case
/// - 11.1: Add subcommand wired to this service
/// - 11.2: Read and parse agent.yaml manifest
/// - 11.3: Generate artifacts using domain functions
/// - 11.4: Transfer agent directory to VM
/// - 11.5: Error when path not found
/// - 11.6: Error when no manifest found
/// - 11.7: Error listing validation violations
/// - 11.8: Error when agent already installed
/// - 11.9: Error when VM not running
/// - 11.10: Validate agent name against `AGENT_NAME_RE`
/// - 11.11: No VM mutations until local validation complete
/// - 11.12: Cleanup on partial transfer failure
pub async fn install_agent(
    provisioner: &(impl ShellExecutor + FileTransfer + InstanceInspector),
    local_fs: &impl LocalFs,
    reporter: &impl ProgressReporter,
    agent_path: &str,
) -> Result<String> {
    // Step 1: Validate agent folder and get name (Req 11.5, 11.6, 11.7, 11.10, 11.11)
    let folder = std::path::Path::new(agent_path);
    anyhow::ensure!(local_fs.exists(folder), "Path not found: {agent_path}");

    let manifest_path = folder.join("agent.yaml");
    anyhow::ensure!(
        local_fs.exists(&manifest_path),
        "No agent.yaml found in: {agent_path}"
    );

    let content = local_fs.read_to_string(&manifest_path)?;
    let manifest: polis_common::agent::AgentManifest =
        serde_yaml_ng::from_str(&content).context("failed to parse agent.yaml")?;

    // Validate manifest before any VM operations (Req 11.7, 11.10, 11.11)
    crate::domain::agent::validate::validate_full_manifest(&manifest)?;
    let name = manifest.metadata.name.clone();

    // Step 2: Ensure VM is running (Req 11.9)
    ensure_vm_running(provisioner).await?;

    // Step 3: Ensure agent doesn't already exist (Req 11.8)
    let target_dir = format!("{VM_ROOT}/agents/{name}");
    let exists = provisioner.exec(&["test", "-d", &target_dir]).await?;
    anyhow::ensure!(
        !exists.status.success(),
        "Agent '{name}' already installed. Remove it first: polis agent remove {name}"
    );

    // Step 4: Generate artifacts via domain functions (Req 11.3)
    reporter.step(&format!("generating artifacts for '{name}'..."));
    let agent_folder = std::path::Path::new(agent_path);
    let parent_dir = agent_folder
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cannot determine parent directory of agent folder"))?;
    let polis_dir = parent_dir.parent().unwrap_or(parent_dir);
    generate_and_write_artifacts(local_fs, polis_dir, &name)?;

    // Step 5: Transfer agent folder to VM (Req 11.4, 11.12)
    reporter.step(&format!("copying '{name}' to VM..."));
    let dest = format!("{VM_ROOT}/agents/{name}");
    let transfer_result = provisioner.transfer_recursive(agent_path, &dest).await;

    // Handle transfer failure with cleanup (Req 11.12)
    match transfer_result {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            // Transfer command ran but failed - attempt cleanup
            let _ = provisioner.exec(&["rm", "-rf", &dest]).await;
            anyhow::bail!(
                "Failed to transfer agent folder: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(e) => {
            // Transfer command itself failed - attempt cleanup
            let _ = provisioner.exec(&["rm", "-rf", &dest]).await;
            return Err(e).context("multipass transfer");
        }
    }

    // Step 6: Update the agents.json registry (Req 11.4 - registry update after successful transfer)
    reporter.step("updating agent registry...");
    let update_result = async {
        let result = read_registry(provisioner).await?;
        let new_entry = AgentRegistryEntry {
            name: manifest.metadata.name.clone(),
            version: Some(manifest.metadata.version.clone()),
            description: Some(manifest.metadata.description.clone()),
        };
        let mut entries = result.entries;
        entries.retain(|e| e.name != new_entry.name);
        entries.push(new_entry);
        write_registry(provisioner, &entries).await
    }
    .await;
    if let Err(e) = update_result {
        // Registry update failed - cleanup the transferred directory
        let _ = provisioner.exec(&["rm", "-rf", &dest]).await;
        return Err(e).context("failed to update agent registry");
    }

    reporter.success(&format!("agent '{name}' installed"));
    Ok(name)
}

/// Generate agent artifacts from `agent.yaml` and write them to
/// `<polis_dir>/agents/<name>/.generated/`.
///
/// Reads the manifest, calls pure domain generators, and writes the four
/// output files to disk.
fn generate_and_write_artifacts(
    local_fs: &impl LocalFs,
    polis_dir: &std::path::Path,
    name: &str,
) -> Result<()> {
    let manifest_path = polis_dir.join("agents").join(name).join("agent.yaml");
    let content = local_fs
        .read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: polis_common::agent::AgentManifest =
        serde_yaml_ng::from_str(&content).context("failed to parse agent.yaml")?;

    let generated_dir = polis_dir.join("agents").join(name).join(".generated");

    let env_content = local_fs
        .read_to_string(&polis_dir.join(".env"))
        .unwrap_or_default();
    let filtered = crate::domain::agent::artifacts::filtered_env(&env_content, &manifest);

    write_artifacts_to_dir(local_fs, &generated_dir, name, &manifest, filtered)
}


