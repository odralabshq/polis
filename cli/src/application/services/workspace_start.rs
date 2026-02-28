//! Application service — workspace start use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

#![allow(dead_code)] // Refactor in progress — service defined ahead of callers

use anyhow::{Context, Result};
use chrono::Utc;

use crate::application::ports::{ProgressReporter, VmProvisioner, WorkspaceStateStore};
use crate::domain::workspace::WorkspaceState;
use crate::workspace::{digest, health, vm};

/// Path to the polis project root inside the VM.
const VM_POLIS_ROOT: &str = "/opt/polis";

/// Outcome of the `start_workspace` use-case.
#[derive(Debug)]
pub enum StartOutcome {
    /// Workspace was already running with the same agent config.
    AlreadyRunning { agent: Option<String> },
    /// Workspace was freshly created and started.
    Created { agent: Option<String> },
    /// A stopped workspace was restarted.
    Restarted { agent: Option<String> },
}

/// Start the workspace, creating it if needed.
///
/// Accepts port trait bounds so the caller can inject real or mock
/// implementations. The service never touches `OutputContext` or any
/// presentation type.
///
/// # Errors
///
/// Returns an error if any step of the provisioning workflow fails.
pub async fn start_workspace(
    provisioner: &impl VmProvisioner,
    state_mgr: &impl WorkspaceStateStore,
    reporter: &impl ProgressReporter,
    agent: Option<&str>,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<StartOutcome> {
    crate::domain::workspace::check_architecture()?;

    let vm_state = vm::state(provisioner).await?;

    match vm_state {
        vm::VmState::Running => handle_running_vm(state_mgr, agent).await,
        vm::VmState::NotFound => {
            create_and_start_vm(provisioner, state_mgr, reporter, agent, assets_dir, version)
                .await?;
            Ok(StartOutcome::Created {
                agent: agent.map(str::to_owned),
            })
        }
        _ => {
            restart_vm(provisioner, state_mgr, reporter, agent).await?;
            health::wait_ready(provisioner, false).await?;
            Ok(StartOutcome::Restarted {
                agent: agent.map(str::to_owned),
            })
        }
    }
}

/// Handle the case where the VM is already running.
async fn handle_running_vm(
    state_mgr: &impl WorkspaceStateStore,
    agent: Option<&str>,
) -> Result<StartOutcome> {
    let current_agent = state_mgr.load_async().await?.and_then(|s| s.active_agent);
    if current_agent.as_deref() == agent {
        return Ok(StartOutcome::AlreadyRunning {
            agent: agent.map(str::to_owned),
        });
    }
    let current_desc = current_agent
        .as_deref()
        .map_or_else(|| "no agent".to_string(), |n| format!("agent '{n}'"));
    let requested_desc = agent.map_or_else(|| "no agent".to_string(), |n| format!("--agent {n}"));
    anyhow::bail!(
        "Workspace is running with {current_desc}. Stop first:\n  polis stop\n  polis start {requested_desc}"
    );
}

/// Full provisioning flow for a new VM.
async fn create_and_start_vm(
    provisioner: &impl VmProvisioner,
    state_mgr: &impl WorkspaceStateStore,
    reporter: &impl ProgressReporter,
    agent: Option<&str>,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<()> {
    // Step 1: Compute config hash before transfer.
    let tar_path = assets_dir.join("polis-setup.config.tar");
    let config_hash = vm::sha256_file(&tar_path).context("computing config tarball SHA256")?;

    reporter.step("workspace isolation starting...");

    // Step 2: Launch VM with cloud-init.
    vm::create(provisioner, true).await?;
    reporter.success("workspace isolation started");

    // Step 3: Transfer config tarball.
    reporter.step("transferring configuration...");
    vm::transfer_config(provisioner, assets_dir, version)
        .await
        .context("transferring config to VM")?;

    // Step 4: Generate certificates and secrets.
    reporter.step("generating certificates and secrets...");
    vm::generate_certs_and_secrets(provisioner)
        .await
        .context("generating certificates and secrets")?;

    // Step 5: Pull Docker images.
    reporter.step("pulling Docker images...");
    vm::pull_images(provisioner)
        .await
        .context("pulling Docker images")?;

    // Step 6: Verify image digests.
    reporter.step("verifying image digests...");
    digest::verify_image_digests(provisioner)
        .await
        .context("verifying image digests")?;

    // Step 7: Set up agent if requested.
    if let Some(name) = agent {
        reporter.step(&format!("setting up agent '{name}'..."));
        setup_agent(provisioner, name).await?;
    }

    // Step 8: Start docker compose.
    reporter.step("starting platform services...");
    start_compose(provisioner, agent).await?;

    // Step 9: Wait for health.
    reporter.step("waiting for workspace to become healthy...");
    health::wait_ready(provisioner, true).await?;
    reporter.success("workspace ready");

    // Step 10: Write config hash after successful startup.
    vm::write_config_hash(provisioner, &config_hash)
        .await
        .context("writing config hash")?;

    // Step 11: Persist state.
    let state = WorkspaceState {
        workspace_id: crate::domain::workspace::generate_workspace_id(),
        created_at: Utc::now(),
        image_sha256: None,
        image_source: None,
        active_agent: agent.map(str::to_owned),
    };
    state_mgr.save_async(&state).await
}

/// Restart a stopped VM.
async fn restart_vm(
    provisioner: &impl VmProvisioner,
    state_mgr: &impl WorkspaceStateStore,
    reporter: &impl ProgressReporter,
    agent: Option<&str>,
) -> Result<()> {
    reporter.step("restarting workspace...");
    vm::restart(provisioner, true).await?;
    reporter.success("workspace restarted");

    if let Some(name) = agent {
        setup_agent(provisioner, name).await?;
        start_compose(provisioner, agent).await?;
    }

    let mut state = state_mgr
        .load_async()
        .await?
        .unwrap_or_else(|| WorkspaceState {
            workspace_id: crate::domain::workspace::generate_workspace_id(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: None,
        });
    state.active_agent = agent.map(str::to_owned);
    state_mgr.save_async(&state).await
}

/// Validate and generate artifacts for an agent.
async fn setup_agent<P: VmProvisioner>(provisioner: &P, agent_name: &str) -> Result<()> {
    const VM_ROOT: &str = "/opt/polis";

    // Verify agent manifest exists in the VM.
    let manifest_path = format!("{VM_ROOT}/agents/{agent_name}/agent.yaml");
    let output = provisioner
        .exec(&["test", "-f", &manifest_path])
        .await
        .context("checking agent manifest")?;
    if !output.status.success() {
        anyhow::bail!("unknown agent '{agent_name}'");
    }

    // Run generate-agent.sh inside the VM.
    let script = format!("{VM_ROOT}/scripts/generate-agent.sh");
    let agents_dir = format!("{VM_ROOT}/agents");
    let output = provisioner
        .exec(&["bash", &script, agent_name, &agents_dir])
        .await
        .context("running generate-agent.sh")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("agent artifact generation failed for '{agent_name}'.\n{stderr}");
    }
    Ok(())
}

/// Start docker compose with optional agent overlay.
async fn start_compose<P: VmProvisioner>(provisioner: &P, agent_name: Option<&str>) -> Result<()> {
    let base = format!("{VM_POLIS_ROOT}/docker-compose.yml");
    let mut args: Vec<String> = vec!["docker".into(), "compose".into(), "-f".into(), base];
    if let Some(name) = agent_name {
        let overlay = format!("{VM_POLIS_ROOT}/agents/{name}/.generated/compose.agent.yaml");
        args.push("-f".into());
        args.push(overlay);
    }
    args.extend(["up".into(), "-d".into(), "--remove-orphans".into()]);

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = provisioner
        .exec(&arg_refs)
        .await
        .context("starting platform")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to start platform.\n{stderr}");
    }
    Ok(())
}
