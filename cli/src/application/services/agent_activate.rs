//! Application service — agent activation use-case.
//!
//! Activates an agent on a running workspace. The caller must verify the VM
//! is running before calling `activate_agent`; a runtime check inside the
//! function provides defense-in-depth.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! No workspace lifecycle imports (`create_and_start_vm`, `restart_vm`,
//! `resolve_action`, `StartAction`).
//!
//! # Requirements
//! 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8, 11.4, 11.5, 13.1, 13.6

use anyhow::{Context, Result};
use chrono::Utc;

use crate::application::ports::{LocalFs, ProgressReporter, VmProvisioner, WorkspaceStateStore};
use crate::application::services::vm::{
    compose::set_active_overlay,
    health::wait_ready,
    lifecycle::{self as vm, VmState},
};
use crate::domain::agent::{overlay_path, resolve_agent_action, AgentAction};
use crate::domain::error::WorkspaceError;
use crate::domain::workspace::{WorkspaceState, VM_ROOT};

// ── Public types ──────────────────────────────────────────────────────────────

/// Options for the `activate_agent` use-case.
pub struct AgentActivateOptions<'a, R: ProgressReporter> {
    pub reporter: &'a R,
    pub agent_name: &'a str,
    pub envs: Vec<String>,
}

/// Outcome of the `activate_agent` use-case.
#[derive(Debug)]
pub enum AgentOutcome {
    /// Agent was freshly installed and activated.
    Installed {
        agent: String,
        onboarding: Vec<polis_common::agent::OnboardingStep>,
    },
    /// Agent was already active — no re-installation performed.
    AlreadyInstalled {
        agent: String,
        onboarding: Vec<polis_common::agent::OnboardingStep>,
    },
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Activate an agent on a running workspace.
///
/// The caller must verify the VM is running before calling this function.
/// A runtime check provides defense-in-depth (Req 6.2, 6.3).
///
/// # Algorithm
///
/// 1. Runtime check: VM must be Running (defense-in-depth)
/// 2. Load persisted state
/// 3. `resolve_agent_action` → Install / `AlreadyInstalled` / Mismatch
/// 4. Install path: `setup_agent` → `set_active_overlay` → `start_compose` →
///    persist state (before health) → `wait_ready`
/// 5. `AlreadyInstalled` path: return onboarding from persisted state
/// 6. Mismatch path: return `WorkspaceError::AgentMismatch` (typed error)
///
/// # Errors
///
/// Returns `WorkspaceError::NotRunning` if the VM is not running at runtime.
/// Returns `WorkspaceError::AgentMismatch` if a different agent is already active.
/// Returns other errors if any VM operation fails.
pub async fn activate_agent<P, S, L, R>(
    provisioner: &P,
    state_mgr: &S,
    local_fs: &L,
    opts: AgentActivateOptions<'_, R>,
) -> Result<AgentOutcome>
where
    P: VmProvisioner,
    S: WorkspaceStateStore,
    L: LocalFs,
    R: ProgressReporter,
{
    // Runtime check: VM must be running (Req 6.2, 6.3, 11.5)
    let vm_state = vm::state(provisioner).await?;
    if vm_state != VmState::Running {
        return Err(WorkspaceError::NotRunning.into());
    }

    let persisted = state_mgr.load_async().await?;

    // Pure domain decision (Req 6.4, 6.6, 6.7)
    let action = resolve_agent_action(opts.agent_name, persisted.as_ref());

    match action {
        AgentAction::Install { agent } => {
            // Install path (Req 6.4, 6.5)
            setup_agent(provisioner, local_fs, &agent, &opts.envs, opts.reporter).await?;
            set_active_overlay(provisioner, Some(&overlay_path(&agent))).await?;
            start_compose(provisioner, &agent).await?;

            // Persist active_agent BEFORE health wait — intentional for large
            // image pull resilience (Req 6.5). On retry, resolve_agent_action
            // returns AlreadyInstalled (idempotent).
            let mut state = persisted.unwrap_or_else(|| WorkspaceState {
                created_at: Utc::now(),
                image_sha256: None,
                image_source: None,
                active_agent: None,
                provisioning: None,
            });
            state.active_agent = Some(agent.clone());
            state_mgr.save_async(&state).await?;

            wait_ready(provisioner, opts.reporter, false, "agent ready").await?;

            let onboarding = load_onboarding(provisioner, &agent).await;
            Ok(AgentOutcome::Installed { agent, onboarding })
        }

        AgentAction::AlreadyInstalled { agent } => {
            // No re-install (Req 6.6)
            let onboarding = load_onboarding(provisioner, &agent).await;
            Ok(AgentOutcome::AlreadyInstalled { agent, onboarding })
        }

        AgentAction::Mismatch { active, requested } => {
            // Typed error, not bail! (Req 6.7, 6.8, 11.4)
            Err(WorkspaceError::AgentMismatch { active, requested }.into())
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Set up an agent inside the VM: transfer artifacts and pull the agent image.
///
/// Reads the agent manifest from the local filesystem, generates compose
/// artifacts, and transfers the `.generated/` folder into the VM.
///
/// Private to this module — single consumer (Req 13.1, 13.6).
async fn setup_agent(
    provisioner: &impl VmProvisioner,
    local_fs: &impl LocalFs,
    agent_name: &str,
    envs: &[String],
    reporter: &impl ProgressReporter,
) -> Result<()> {
    reporter.begin_stage(&format!("setting up agent '{agent_name}'..."));

    // Locate the agent directory relative to the polis project root.
    let agent_dir = std::path::PathBuf::from(VM_ROOT)
        .join("agents")
        .join(agent_name);
    let agent_dir_str = agent_dir.to_string_lossy().to_string();

    // Verify the agent directory exists in the VM.
    let exists = provisioner
        .exec(&["test", "-d", &agent_dir_str])
        .await
        .context("checking agent directory in VM")?;
    anyhow::ensure!(
        exists.status.success(),
        "Agent '{agent_name}' not found in VM at {agent_dir_str}. \
         Install it first: polis agent add --path <path>"
    );

    // Read the agent manifest from the VM.
    let manifest_path = format!("{agent_dir_str}/agent.yaml");
    let cat_out = provisioner
        .exec(&["cat", &manifest_path])
        .await
        .context("reading agent.yaml from VM")?;
    anyhow::ensure!(
        cat_out.status.success(),
        "Failed to read agent manifest from VM: {}",
        String::from_utf8_lossy(&cat_out.stderr)
    );

    let manifest_str =
        String::from_utf8(cat_out.stdout).context("parsing agent.yaml from VM as UTF-8")?;
    let manifest: polis_common::agent::AgentManifest =
        serde_yaml::from_str(&manifest_str).context("failed to parse agent.yaml")?;

    // Build the .env content from the provided envs.
    let env_content = build_env_content(envs);
    let filtered =
        crate::domain::agent::artifacts::filtered_env(&env_content, &manifest);

    // Write artifacts to a temp dir, then transfer to VM.
    let tmp = tempfile::tempdir().context("creating temp dir for artifact generation")?;
    let generated_dir = tmp.path().join("agents").join(agent_name).join(".generated");

    crate::application::services::agent_crud::write_artifacts_to_dir(
        local_fs,
        &generated_dir,
        agent_name,
        &manifest,
        filtered,
    )?;

    // Remove existing .generated to avoid nested directories from
    // `multipass transfer --recursive` (which nests src inside dest if dest exists).
    let generated_dest = format!("{agent_dir_str}/.generated");
    let _ = provisioner
        .exec(&["rm", "-rf", &generated_dest])
        .await;

    let generated_src_str = generated_dir.to_string_lossy().to_string();
    let transfer_out = provisioner
        .transfer_recursive(&generated_src_str, &generated_dest)
        .await
        .context("transferring agent artifacts to VM")?;
    anyhow::ensure!(
        transfer_out.status.success(),
        "Failed to transfer agent artifacts: {}",
        String::from_utf8_lossy(&transfer_out.stderr)
    );

    reporter.complete_stage();
    Ok(())
}

/// Start the agent's compose stack inside the VM.
///
/// Runs `docker compose -f <base> -f <overlay> up -d`.
///
/// Private to this module — single consumer (Req 13.6).
async fn start_compose(provisioner: &impl VmProvisioner, agent_name: &str) -> Result<()> {
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = overlay_path(agent_name);

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
        ])
        .await
        .context("starting agent compose stack")?;

    anyhow::ensure!(
        out.status.success(),
        "Failed to start agent compose stack: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

/// Load onboarding steps from the agent manifest inside the VM.
///
/// Returns an empty vec if the manifest cannot be read or parsed — this is
/// non-fatal; the agent is already running.
async fn load_onboarding(
    provisioner: &impl VmProvisioner,
    agent_name: &str,
) -> Vec<polis_common::agent::OnboardingStep> {
    let manifest_path = format!("{VM_ROOT}/agents/{agent_name}/agent.yaml");
    let Ok(out) = provisioner.exec(&["cat", &manifest_path]).await else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let Ok(content) = String::from_utf8(out.stdout) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_yaml::from_str::<polis_common::agent::AgentManifest>(&content) else {
        return Vec::new();
    };
    manifest.spec.onboarding
}

/// Build a `.env`-style string from a list of `KEY=VALUE` strings.
fn build_env_content(envs: &[String]) -> String {
    envs.join("\n")
}
