//! Application service — workspace start use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use crate::domain::agent::artifacts;
use anyhow::{Context, Result};

pub struct StartOptions<'a, R: crate::application::ports::ProgressReporter> {
    pub reporter: &'a R,
    pub agent: Option<&'a str>,
    pub envs: Vec<String>,
    pub assets_dir: &'a std::path::Path,
    pub version: &'a str,
}


use chrono::Utc;

use crate::application::ports::{
    AssetExtractor, FileHasher, LocalFs, ProgressReporter, SshConfigurator, VmProvisioner,
    WorkspaceStateStore,
};
use crate::application::services::vm::{
    health::wait_ready,
    integrity::{verify_image_digests, write_config_hash},
    lifecycle::{self as vm, VmState},
    provision::{generate_certs_and_secrets, transfer_config},
    services::pull_images,
};
use crate::domain::workspace::{VM_ROOT, WorkspaceState};

/// Outcome of the `start_workspace` use-case.
#[derive(Debug)]
#[allow(dead_code)] // Public API — not yet called from commands/start.rs
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
    assets: &impl AssetExtractor,
    ssh: &impl SshConfigurator,
    hasher: &impl FileHasher,
    local_fs: &impl LocalFs,
    opts: StartOptions<'_, impl crate::application::ports::ProgressReporter>,
) -> Result<StartOutcome> {
    let reporter = opts.reporter;
    let StartOptions { agent, envs, assets_dir, version, .. } = opts;
    crate::domain::workspace::check_architecture()?;

    let vm_state = vm::state(provisioner).await?;

    match vm_state {
        VmState::Running => {
            handle_running_vm(provisioner, state_mgr, local_fs, reporter, agent, envs).await
        }
        VmState::NotFound => {
            create_and_start_vm(
                provisioner,
                state_mgr,
                assets,
                ssh,
                hasher,
                local_fs,
                StartOptions { reporter, agent, envs, assets_dir, version },
            )
            .await?;
            Ok(StartOutcome::Created {
                agent: agent.map(str::to_owned),
            })
        }
        _ => {
            restart_vm(provisioner, state_mgr, assets, ssh, local_fs, reporter, agent, envs).await?;
            wait_ready(provisioner, reporter, false).await?;
            Ok(StartOutcome::Restarted {
                agent: agent.map(str::to_owned),
            })
        }
    }
}

/// Handle the case where the VM is already running.
///
/// When no agent is currently active and one is requested, set it up
/// in-place without stopping the VM. This avoids a stop/start cycle
/// which triggers the Hyper-V Default Switch DHCP bug on Windows.
async fn handle_running_vm(
    provisioner: &impl VmProvisioner,
    state_mgr: &impl WorkspaceStateStore,
    local_fs: &impl LocalFs,
    reporter: &impl ProgressReporter,
    agent: Option<&str>,
    envs: Vec<String>,
) -> Result<StartOutcome> {
    let current_agent = state_mgr.load_async().await?.and_then(|s| s.active_agent);
    if current_agent.as_deref() == agent {
        return Ok(StartOutcome::AlreadyRunning {
            agent: agent.map(str::to_owned),
        });
    }

    // Allow adding an agent to a running workspace that has no agent.
    if current_agent.is_none() {
        if let Some(name) = agent {
            reporter.step(&format!("setting up agent '{name}'..."));
            setup_agent(provisioner, local_fs, name, &envs).await?;
            reporter.step("restarting platform services with agent...");
            start_compose(provisioner, Some(name)).await?;
            reporter.step("waiting for workspace to become healthy...");
            wait_ready(provisioner, reporter, false).await?;
            reporter.success("workspace ready");

            let mut state = state_mgr
                .load_async()
                .await?
                .unwrap_or_else(|| WorkspaceState {
                    created_at: Utc::now(),
                    image_sha256: None,
                    image_source: None,
                    active_agent: None,
                });
            state.active_agent = Some(name.to_owned());
            state_mgr.save_async(&state).await?;

            return Ok(StartOutcome::Restarted {
                agent: Some(name.to_owned()),
            });
        }
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
    assets: &impl AssetExtractor,
    ssh: &impl SshConfigurator,
    hasher: &impl FileHasher,
    local_fs: &impl LocalFs,
    opts: StartOptions<'_, impl crate::application::ports::ProgressReporter>,
) -> Result<()> {
    let reporter = opts.reporter;
    let StartOptions { agent, envs, assets_dir, version, .. } = opts;
    // Step 1: Compute config hash before transfer.
    let tar_path = assets_dir.join("polis-setup.config.tar");
    let config_hash = hasher
        .sha256_file(&tar_path)
        .context("computing config tarball SHA256")?;

    reporter.step("workspace isolation starting...");

    // Step 2: Launch VM with cloud-init.
    vm::create(provisioner, assets, ssh, reporter, true).await?;
    reporter.success("workspace isolation started");

    // Step 3: Transfer config tarball.
    reporter.step("transferring configuration...");
    transfer_config(provisioner, assets_dir, version)
        .await
        .context("transferring config to VM")?;

    // Step 4: Generate certificates and secrets.
    reporter.step("generating certificates and secrets...");
    generate_certs_and_secrets(provisioner)
        .await
        .context("generating certificates and secrets")?;

    // Step 5: Pull Docker images.
    reporter.step("pulling Docker images...");
    pull_images(provisioner, reporter)
        .await
        .context("pulling Docker images")?;

    // Step 6: Verify image digests.
    reporter.step("verifying image digests...");
    verify_image_digests(provisioner, assets, reporter)
        .await
        .context("verifying image digests")?;

    // Step 7: Set up agent if requested.
    if let Some(name) = agent {
        reporter.step(&format!("setting up agent '{name}'..."));
        setup_agent(provisioner, local_fs, name, &envs).await?;
    }

    // Step 8: Start docker compose.
    reporter.step("starting platform services...");
    start_compose(provisioner, agent).await?;

    // Step 9: Wait for health.
    reporter.step("waiting for workspace to become healthy...");
    wait_ready(provisioner, reporter, true).await?;
    reporter.success("workspace ready");

    // Step 10: Write config hash after successful startup.
    write_config_hash(provisioner, &config_hash)
        .await
        .context("writing config hash")?;

    // Step 11: Persist state.
    let state = WorkspaceState {
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
    _assets: &impl AssetExtractor,
    _ssh: &impl SshConfigurator,
    local_fs: &impl LocalFs,
    reporter: &impl ProgressReporter,
    agent: Option<&str>,
    envs: Vec<String>,
) -> Result<()> {
    reporter.step("restarting workspace...");
    vm::restart(provisioner, reporter, true).await?;
    reporter.success("workspace restarted");

    if let Some(name) = agent {
        setup_agent(provisioner, local_fs, name, &envs).await?;
        start_compose(provisioner, agent).await?;
    }

    let mut state = state_mgr
        .load_async()
        .await?
        .unwrap_or_else(|| WorkspaceState {
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: None,
        });
    state.active_agent = agent.map(str::to_owned);
    state_mgr.save_async(&state).await
}

/// Validate and generate artifacts for an agent.
///
/// Reads the manifest from the VM, generates artifacts using the Rust domain
/// functions, and transfers the `.generated/` folder back into the VM.
/// This replaces the old `generate-agent.sh` shell script invocation.
async fn setup_agent<P: VmProvisioner>(
    provisioner: &P,
    local_fs: &impl LocalFs,
    agent_name: &str,
    envs: &[String],
) -> Result<()> {
    // Verify agent manifest exists in the VM.
    let manifest_path = format!("{VM_ROOT}/agents/{agent_name}/agent.yaml");
    let check = provisioner
        .exec(&["test", "-f", &manifest_path])
        .await
        .context("checking agent manifest")?;
    if !check.status.success() {
        anyhow::bail!("unknown agent '{agent_name}'");
    }

    // Read manifest from VM.
    let cat_out = provisioner
        .exec(&["cat", &manifest_path])
        .await
        .context("reading agent manifest from VM")?;
    anyhow::ensure!(
        cat_out.status.success(),
        "failed to read agent manifest from VM: {}",
        String::from_utf8_lossy(&cat_out.stderr)
    );

    // Generate artifacts in a temp dir using pure Rust domain functions.
    let name = agent_name.to_owned();
    let stdout_bytes = cat_out.stdout.clone();
    let tmp = tempfile::tempdir().context("creating temp dir for artifact generation")?;
    let tmp_path = tmp.path().to_path_buf();
    

    let manifest: polis_common::agent::AgentManifest =
        serde_yaml::from_slice(&stdout_bytes).context("parsing agent.yaml from VM")?;
    crate::domain::agent::validate::validate_full_manifest(&manifest)?;

    let generated_dir = tmp_path.join("agents").join(&name).join(".generated");
    local_fs.create_dir_all(&generated_dir)?;

    let compose = artifacts::compose_overlay(&manifest).replace("\r\n", "\n");
    local_fs.write(&generated_dir.join("compose.agent.yaml"), compose)?;

    let unit = artifacts::systemd_unit(&manifest).replace("\r\n", "\n");
    let hash = artifacts::service_hash(&unit);
    local_fs.write(&generated_dir.join(format!("{name}.service")), unit)?;
    local_fs.write(&generated_dir.join(format!("{name}.service.sha256")), hash)?;
    
    // Write environment variables to the agent's .env file, forcing LF line endings.
    let env_content = if envs.is_empty() {
        String::new()
    } else {
        format!("{}\n", envs.join("\n")).replace("\r\n", "\n")
    };
    local_fs.write(&generated_dir.join(format!("{name}.env")), env_content)?;

    // Transfer the generated artifacts back into the VM.
    // Remove existing .generated to avoid nested directories from
    // `multipass transfer --recursive` (which nests src inside dest if dest exists).
    let generated_src = tmp
        .path()
        .join("agents")
        .join(agent_name)
        .join(".generated");
    let generated_src_str = generated_src.to_string_lossy().to_string();
    let generated_dest = format!("{VM_ROOT}/agents/{agent_name}/.generated");
    provisioner
        .exec(&["rm", "-rf", &generated_dest])
        .await
        .context("removing old generated artifacts")?;
    let transfer_out = provisioner
        .transfer_recursive(&generated_src_str, &generated_dest)
        .await
        .context("transferring generated artifacts to VM")?;
    anyhow::ensure!(
        transfer_out.status.success(),
        "failed to transfer generated artifacts: {}",
        String::from_utf8_lossy(&transfer_out.stderr)
    );

    Ok(())
}

/// Start docker compose with optional agent overlay.
async fn start_compose<P: VmProvisioner>(provisioner: &P, agent_name: Option<&str>) -> Result<()> {
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let mut args: Vec<String> = vec!["docker".into(), "compose".into(), "-f".into(), base];
    if let Some(name) = agent_name {
        let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
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
