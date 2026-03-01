//! Application service — agent install/remove/update use-cases.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

pub use crate::domain::agent::AgentInfo;
use anyhow::{Context, Result};

use crate::application::ports::{
    FileTransfer, InstanceInspector, ProgressReporter, ShellExecutor,
    WorkspaceStateStore,
};
use crate::application::services::vm::lifecycle::{self as vm, VmState};

/// Generate agent artifacts from `agent.yaml` and write them to
/// `<polis_dir>/agents/<name>/.generated/`.
///
/// Reads the manifest, calls pure domain generators, and writes the four
/// output files to disk via `spawn_blocking` to avoid blocking the async runtime.
///
/// # Errors
///
/// Returns an error if the manifest cannot be read/parsed, or if any file
/// write fails.
fn generate_and_write_artifacts(local_fs: &impl crate::application::ports::LocalFs, polis_dir: &std::path::Path, name: &str) -> Result<()> {
    use crate::domain::agent::artifacts;

    let manifest_path = polis_dir.join("agents").join(name).join("agent.yaml");
    let content = local_fs.read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: polis_common::agent::AgentManifest =
        serde_yaml::from_str(&content).context("failed to parse agent.yaml")?;

    let generated_dir = polis_dir.join("agents").join(name).join(".generated");
    local_fs.create_dir_all(&generated_dir)
        .with_context(|| format!("creating {}", generated_dir.display()))?;

    let compose = artifacts::compose_overlay(&manifest);
    local_fs.write(&generated_dir.join("compose.agent.yaml"), compose)
        .context("writing compose.agent.yaml")?;

    let unit = artifacts::systemd_unit(&manifest);
    let hash = artifacts::service_hash(&unit);
    local_fs.write(&generated_dir.join(format!("{name}.service")), unit)
        .context("writing .service file")?;
    local_fs.write(&generated_dir.join(format!("{name}.service.sha256")), hash)
        .context("writing .service.sha256 file")?;

    let env_content = local_fs.read_to_string(&polis_dir.join(".env")).unwrap_or_default();
    let filtered = artifacts::filtered_env(&env_content, &manifest);
    local_fs.write(&generated_dir.join(format!("{name}.env")), filtered)
        .context("writing .env file")?;

    Ok(())
}

/// Path to the polis project root inside the VM.
use crate::domain::workspace::VM_ROOT;

/// Install an agent from a local folder into the VM.
///
/// Steps:
/// 1. Validate the agent folder and manifest (domain validation)
/// 2. Generate artifacts using domain functions
/// 3. Transfer agent folder to VM via `FileTransfer`
///
/// # Errors
///
/// Returns an error if validation fails, artifact generation fails,
/// or any VM operation fails.
pub async fn install_agent(
    provisioner: &(impl ShellExecutor + FileTransfer + InstanceInspector),
    _state_mgr: &impl WorkspaceStateStore,
    local_fs: &impl crate::application::ports::LocalFs,
    reporter: &impl ProgressReporter,
    agent_path: &str,
) -> Result<String> {
    // Step 1: Validate agent folder and get name.
    let folder = std::path::Path::new(agent_path);
    anyhow::ensure!(local_fs.exists(folder), "Path not found: {agent_path}");
    let manifest_path = folder.join("agent.yaml");
    anyhow::ensure!(
        local_fs.exists(&manifest_path),
        "No agent.yaml found in: {agent_path}"
    );
    let content = local_fs.read_to_string(&manifest_path)?;

    let manifest: polis_common::agent::AgentManifest =
        serde_yaml::from_str(&content).context("failed to parse agent.yaml")?;
    crate::domain::agent::validate::validate_full_manifest(&manifest)?;
    let name = manifest.metadata.name.clone();

    // Step 2: Require VM running.
    anyhow::ensure!(
        vm::state(provisioner).await? == VmState::Running,
        "VM is not running. Start it first: polis start"
    );

    // Step 3: Ensure agent doesn't already exist.
    let target_dir = format!("{VM_ROOT}/agents/{name}");
    let exists = provisioner.exec(&["test", "-d", &target_dir]).await?;
    anyhow::ensure!(
        !exists.status.success(),
        "Agent '{name}' already installed. Remove it first: polis agent remove {name}"
    );

    // Step 4: Generate artifacts via domain functions.
    reporter.step(&format!("generating artifacts for '{name}'..."));
    let agent_folder = std::path::Path::new(agent_path);
    let parent_dir = agent_folder
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cannot determine parent directory of agent folder"))?;
    let polis_dir = parent_dir.parent().unwrap_or(parent_dir);
    generate_and_write_artifacts(local_fs, polis_dir, &name)?;

    // Step 5: Transfer agent folder to VM.
    reporter.step(&format!("copying '{name}' to VM..."));
    let dest = format!("{VM_ROOT}/agents/{name}");
    let out = provisioner
        .transfer_recursive(agent_path, &dest)
        .await
        .context("multipass transfer")?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to transfer agent folder: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    reporter.success(&format!("agent '{name}' installed"));
    Ok(name)
}

/// Remove an installed agent from the VM.
///
/// If the agent is currently active, stops the compose stack first and
/// restarts the base control plane after removal.
///
/// # Errors
///
/// Returns an error if the agent is not installed or any VM operation fails.
pub async fn remove_agent(
    provisioner: &(impl ShellExecutor + InstanceInspector),
    state_mgr: &impl WorkspaceStateStore,
    reporter: &impl ProgressReporter,
    agent_name: &str,
) -> Result<()> {
    anyhow::ensure!(
        crate::domain::agent::validate::is_valid_agent_name(agent_name),
        "invalid agent name: '{agent_name}'"
    );

    let agent_dir = format!("{VM_ROOT}/agents/{agent_name}");
    let exists = provisioner.exec(&["test", "-d", &agent_dir]).await?;
    anyhow::ensure!(
        exists.status.success(),
        "Agent '{agent_name}' is not installed."
    );

    let active = state_mgr.load_async().await?.and_then(|s| s.active_agent);
    let is_active = active.as_deref() == Some(agent_name);

    if is_active {
        reporter.step(&format!("stopping active agent '{agent_name}'..."));
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let overlay = format!("{VM_ROOT}/agents/{agent_name}/.generated/compose.agent.yaml");
        let down = provisioner
            .exec(&["docker", "compose", "-f", &base, "-f", &overlay, "down"])
            .await?;
        anyhow::ensure!(
            down.status.success(),
            "Failed to stop stack: {}",
            String::from_utf8_lossy(&down.stderr)
        );
    }

    reporter.step(&format!("removing '{agent_name}'..."));
    let rm = provisioner.exec(&["rm", "-rf", &agent_dir]).await?;
    anyhow::ensure!(
        rm.status.success(),
        "Failed to remove agent directory: {}",
        String::from_utf8_lossy(&rm.stderr)
    );

    if is_active {
        reporter.step("restarting control plane...");
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let up = provisioner
            .exec(&["docker", "compose", "-f", &base, "up", "-d"])
            .await?;
        anyhow::ensure!(
            up.status.success(),
            "Failed to restart control plane: {}",
            String::from_utf8_lossy(&up.stderr)
        );

        if let Ok(Some(mut state)) = state_mgr.load_async().await {
            state.active_agent = None;
            state_mgr.save_async(&state).await?;
        }
    }

    reporter.success(&format!("agent '{agent_name}' removed"));
    Ok(())
}

/// Update the active agent's artifacts and recreate its workspace container.
///
/// Reads the agent manifest from the VM, regenerates artifacts locally,
/// transfers them back, and force-recreates the workspace container.
///
/// # Errors
///
/// Returns an error if no agent is active, the VM is not running, or any
/// VM operation fails.
pub async fn update_agent(
    provisioner: &(impl ShellExecutor + FileTransfer + InstanceInspector),
    state_mgr: &impl WorkspaceStateStore,
    local_fs: &impl crate::application::ports::LocalFs,
    reporter: &impl ProgressReporter,
) -> Result<String> {
    let name = state_mgr
        .load_async()
        .await?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("no active agent. Start one: polis start --agent <name>"))?;

    anyhow::ensure!(
        vm::state(provisioner).await? == VmState::Running,
        "Workspace is not running. Start it first: polis start --agent <name>"
    );

    reporter.step(&format!("regenerating artifacts for '{name}'..."));

    // Read agent.yaml from the VM.
    let cat_out = provisioner
        .exec(&["cat", &format!("{VM_ROOT}/agents/{name}/agent.yaml")])
        .await
        .context("reading agent.yaml from VM")?;
    anyhow::ensure!(
        cat_out.status.success(),
        "Failed to read agent manifest from VM: {}",
        String::from_utf8_lossy(&cat_out.stderr)
    );

    // Write manifest to a temp dir and run the Rust generator.
    let tmp = tempfile::tempdir().context("creating temp dir for artifact generation")?;
    let agent_dir = tmp.path().join("agents").join(&name);
    let stdout_str = String::from_utf8(cat_out.stdout).context("parsing agent.yaml from VM as UTF-8")?;
    local_fs.create_dir_all(&agent_dir)?;
    local_fs.write(&agent_dir.join("agent.yaml"), stdout_str)?;

    generate_and_write_artifacts(local_fs, tmp.path(), &name)?;

    // Transfer the regenerated .generated/ folder back into the VM.
    // Remove existing .generated to avoid nested directories from
    // `multipass transfer --recursive` (which nests src inside dest if dest exists).
    reporter.step("transferring updated artifacts...");
    let generated_src = agent_dir.join(".generated");
    let generated_src_str = generated_src.to_string_lossy().to_string();
    let generated_dest = format!("{VM_ROOT}/agents/{name}/.generated");
    provisioner
        .exec(&["rm", "-rf", &generated_dest])
        .await
        .context("removing old generated artifacts")?;
    let transfer_out = provisioner
        .transfer_recursive(&generated_src_str, &generated_dest)
        .await
        .context("transferring regenerated artifacts to VM")?;
    anyhow::ensure!(
        transfer_out.status.success(),
        "Failed to transfer regenerated artifacts: {}",
        String::from_utf8_lossy(&transfer_out.stderr)
    );

    reporter.step("recreating workspace container...");
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
    let out = provisioner
        .exec(&[
            "docker",
            "compose",
            "-f",
            &base,
            "-f",
            &overlay,
            "up",
            "-d",
            "--force-recreate",
            "workspace",
        ])
        .await?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to recreate workspace: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    reporter.success(&format!("agent '{name}' updated"));
    Ok(name)
}

/// List all installed agents.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn list_agents(
    provisioner: &impl ShellExecutor,
    state_mgr: &impl WorkspaceStateStore,
) -> Result<Vec<AgentInfo>> {
    // Scan agents/*/agent.yaml inside VM (exclude _template).
    let scan = provisioner
        .exec(&[
            "bash",
            "-c",
            &format!(
                "for f in {VM_ROOT}/agents/*/agent.yaml; do \
                   dir=$(dirname \"$f\"); \
                   name=$(basename \"$dir\"); \
                   [ \"$name\" = \"_template\" ] && continue; \
                   [ -f \"$f\" ] || continue; \
                   printf '===AGENT:%s===\\n' \"$name\"; \
                   cat \"$f\"; \
                   printf '\\n===END===\\n'; \
                 done"
            ),
        ])
        .await?;

    let output = String::from_utf8_lossy(&scan.stdout);
    let active = state_mgr.load_async().await?.and_then(|s| s.active_agent);

    let mut agents = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_yaml = String::new();

    for line in output.lines() {
        if let Some(name) = line
            .strip_prefix("===AGENT:")
            .and_then(|s| s.strip_suffix("==="))
        {
            current_name = Some(name.to_string());
            current_yaml.clear();
        } else if line == "===END===" {
            if let Some(dir_name) = current_name.take() {
                let is_active = active.as_deref() == Some(&dir_name);
                if let Ok(m) = serde_yaml::from_str::<serde_yaml::Value>(&current_yaml) {
                    let metadata = m.get("metadata");
                    agents.push(AgentInfo {
                        name: metadata
                            .and_then(|m| m.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(&dir_name)
                            .to_string(),
                        version: metadata
                            .and_then(|m| m.get("version"))
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        description: metadata
                            .and_then(|m| m.get("description"))
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        active: is_active,
                    });
                }
            }
        } else if current_name.is_some() {
            current_yaml.push_str(line);
            current_yaml.push('\n');
        }
    }

    Ok(agents)
}
