//! Application service — agent activation use-case.
//!
//! Activates an agent on a running workspace. Uses `ensure_vm_running` guard
//! to verify VM state before any operations.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! No workspace lifecycle imports (`create_and_start_vm`, `restart_vm`,
//! `resolve_action`, `StartAction`).
//!
//! # Requirements
//! 10.2, 14.1, 14.2, 14.8, 14.9, 14.10, 14.15, 14.16, 14.18

use anyhow::{Context, Result};
use chrono::Utc;

use crate::application::ports::{
    FileTransfer, InstanceInspector, LocalFs, ProgressReporter, ShellExecutor, WorkspaceStateStore,
};
use crate::application::vm::{compose::set_active_overlay, health::wait_ready};
use crate::domain::agent::{AgentAction, overlay_path, resolve_agent_action};
use crate::domain::error::SwapError;
use crate::domain::workspace::{VM_ROOT, WorkspaceState};

use super::{artifacts::write_artifacts_to_dir, ensure_vm_running};

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
    /// Agent was freshly activated.
    Activated {
        agent: String,
        onboarding: Vec<polis_common::agent::OnboardingStep>,
    },
    /// Agent was already active — no re-activation performed.
    AlreadyActive {
        agent: String,
        onboarding: Vec<polis_common::agent::OnboardingStep>,
    },
}

/// Result of attempting to activate an agent.
///
/// This enum allows the service to return a domain decision to the commands layer,
/// which can then handle user interaction (e.g., swap confirmation) appropriately.
#[derive(Debug)]
pub enum ActivateOutcome {
    /// Agent activated successfully.
    Activated(AgentOutcome),
    /// Agent activated but health check timed out — may not be healthy.
    ActivatedUnhealthy(AgentOutcome),
    /// Agent is already active — return existing onboarding steps.
    AlreadyActive(AgentOutcome),
    /// A different agent is active — confirmation required before swap.
    SwapRequired { active: String, requested: String },
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Options for the `swap_agent` use-case.
pub struct AgentSwapOptions<'a, R: ProgressReporter> {
    pub reporter: &'a R,
    pub active_name: &'a str,
    pub new_name: &'a str,
    pub envs: Vec<String>,
}

/// Swap the active agent using the Saga pattern.
///
/// If the new agent fails to start, attempts to restart the old agent as a
/// rollback. If rollback also fails, returns a typed error with manual recovery
/// commands for both agents.
///
/// # Algorithm
///
/// 1. Stop old agent compose stack
///    - Fails: Abort, return error, old agent stays active
/// 2. Set overlay to new agent artifacts
/// 3. Start new agent compose stack
///    - Fails: Rollback (restart old agent compose stack)
///      - Rollback succeeds: Return error (new agent failed, old agent restored)
///      - Rollback fails: Return error (both failed, manual recovery commands)
/// 4. Persist `active_agent` = `new_name`
/// 5. Health check wait
///    - Timeout: Return warning (agent may not be healthy)
///    - Success: Return success with onboarding steps
///
/// # Errors
///
/// Returns typed `SwapError` variants with recovery commands:
/// - `StopFailed`: Failed to stop the old agent
/// - `StartFailedRolledBack`: New agent failed to activate, old agent restored
/// - `StartFailedRollbackFailed`: Both new agent activation and rollback failed
///
/// # Requirements
///
/// - 14.5: Swap agents when user confirms
/// - 14.13: Abort swap if stopping old agent fails
/// - 14.14: Rollback on new agent activation failure, typed error if rollback fails
pub async fn swap_agent<P, S, L, R>(
    provisioner: &P,
    state_mgr: &S,
    local_fs: &L,
    opts: AgentSwapOptions<'_, R>,
) -> Result<ActivateOutcome>
where
    P: ShellExecutor + FileTransfer + InstanceInspector,
    S: WorkspaceStateStore,
    L: LocalFs,
    R: ProgressReporter,
{
    // Validate env vars before any VM operations (Req 14.17)
    validate_env_vars(&opts.envs)?;

    let active_name = opts.active_name;
    let new_name = opts.new_name;
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let old_overlay = overlay_path(active_name);
    let new_overlay = overlay_path(new_name);

    // Step 1: Stop old agent compose stack (Req 14.13)
    opts.reporter
        .begin_stage(&format!("stopping agent '{active_name}'..."));
    let down = provisioner
        .exec(&["docker", "compose", "-f", &base, "-f", &old_overlay, "down"])
        .await
        .context("stopping old agent compose stack")?;

    if !down.status.success() {
        // Abort: return error, old agent stays active
        return Err(SwapError::StopFailed {
            agent: active_name.to_string(),
            recovery: format!(
                "docker compose -f {base} -f {old_overlay} down && polis agent activate {new_name}"
            ),
        }
        .into());
    }
    opts.reporter.complete_stage();

    // Step 2: Set up new agent (generate artifacts, transfer to VM)
    setup_agent(provisioner, local_fs, new_name, &opts.envs, opts.reporter).await?;

    // Step 3: Set overlay to new agent artifacts
    set_active_overlay(provisioner, Some(&new_overlay)).await?;

    // Step 4: Start new agent compose stack (Req 14.14)
    opts.reporter
        .begin_stage(&format!("starting agent '{new_name}'..."));
    let up = provisioner
        .exec(&[
            "docker",
            "compose",
            "-f",
            &base,
            "-f",
            &new_overlay,
            "up",
            "-d",
        ])
        .await
        .context("starting new agent compose stack")?;

    if !up.status.success() {
        let start_error = String::from_utf8_lossy(&up.stderr).to_string();

        // Rollback: restart old agent compose stack
        opts.reporter.begin_stage(&format!(
            "new agent failed, rolling back to '{active_name}'..."
        ));

        // Reset overlay to old agent
        let _ = set_active_overlay(provisioner, Some(&old_overlay)).await;

        let rollback = provisioner
            .exec(&[
                "docker",
                "compose",
                "-f",
                &base,
                "-f",
                &old_overlay,
                "up",
                "-d",
            ])
            .await;

        let rollback_succeeded = rollback
            .as_ref()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if rollback_succeeded {
            opts.reporter.complete_stage();
            // Rollback succeeded: return error (new agent failed, old agent restored)
            return Err(SwapError::StartFailedRolledBack {
                agent: new_name.to_string(),
                old_agent: active_name.to_string(),
                recovery: format!("polis agent activate {new_name}"),
            }
            .into());
        }
        // Rollback failed: return error with manual recovery commands for both
        let rollback_error = rollback.map_or_else(
            |e| e.to_string(),
            |o| String::from_utf8_lossy(&o.stderr).to_string(),
        );

        return Err(SwapError::StartFailedRollbackFailed {
            agent: new_name.to_string(),
            old_agent: active_name.to_string(),
            original: start_error,
            rollback: rollback_error,
            old_recovery: format!("docker compose -f {base} -f {old_overlay} up -d"),
            new_recovery: format!("docker compose -f {base} -f {new_overlay} up -d"),
        }
        .into());
    }
    opts.reporter.complete_stage();

    // Step 5-6: Persist state and wait for health
    finalize_activation(state_mgr, provisioner, opts.reporter, new_name).await
}

/// Persist the active agent state and wait for health check.
async fn finalize_activation<P, S, R>(
    state_mgr: &S,
    provisioner: &P,
    reporter: &R,
    agent_name: &str,
) -> Result<ActivateOutcome>
where
    P: ShellExecutor,
    S: WorkspaceStateStore,
    R: ProgressReporter,
{
    // Persist active_agent (Req 14.9)
    let persisted = state_mgr.load_async().await?;
    let mut state = persisted.unwrap_or_else(|| WorkspaceState {
        created_at: Utc::now(),
        image_sha256: None,
        image_source: None,
        active_agent: None,
        provisioning: None,
    });
    state.active_agent = Some(agent_name.to_string());
    state_mgr.save_async(&state).await?;

    // Health check wait (Req 14.10, 14.16, 14.18)
    let health_result = wait_ready(provisioner, reporter, false, "agent ready").await;

    let onboarding = load_onboarding(provisioner, agent_name).await;
    let outcome = AgentOutcome::Activated {
        agent: agent_name.to_string(),
        onboarding,
    };

    match health_result {
        Ok(()) => Ok(ActivateOutcome::Activated(outcome)),
        Err(_) => Ok(ActivateOutcome::ActivatedUnhealthy(outcome)),
    }
}

/// Activate an agent on a running workspace.
///
/// Uses `ensure_vm_running` guard to verify VM state before any operations.
///
/// # Algorithm
///
/// 1. `ensure_vm_running` guard (Req 14.8)
/// 2. Load persisted state
/// 3. `resolve_agent_action` → Activate / `AlreadyActive` / Mismatch
/// 4. Activate path: `setup_agent` → `set_active_overlay` → `start_compose` →
///    persist state (before health) → health check wait
/// 5. `AlreadyActive` path: return `AlreadyActive` with onboarding
/// 6. Mismatch path: return `SwapRequired` (Req 14.4)
///
/// # Compose Start Failure Rollback (Req 14.15)
///
/// If `docker compose up -d` fails, the overlay symlink is removed and an error
/// is returned. State is NOT persisted — the system remains in its previous state.
///
/// # Health Check (Req 14.10, 14.16, 14.18)
///
/// Uses the existing workspace health check infrastructure (900s timeout, 2s interval).
/// If the health check times out, returns `ActivatedUnhealthy` with a warning.
///
/// # Errors
///
/// Returns `WorkspaceError::NotRunning` if the VM is not running.
/// Returns other errors if any VM operation fails.
pub async fn activate_agent<P, S, L, R>(
    provisioner: &P,
    state_mgr: &S,
    local_fs: &L,
    opts: AgentActivateOptions<'_, R>,
) -> Result<ActivateOutcome>
where
    P: ShellExecutor + FileTransfer + InstanceInspector,
    S: WorkspaceStateStore,
    L: LocalFs,
    R: ProgressReporter,
{
    // Validate env vars before any VM operations (Req 14.17)
    validate_env_vars(&opts.envs)?;

    // VM running guard (Req 14.8)
    ensure_vm_running(provisioner).await?;

    let persisted = state_mgr.load_async().await?;

    // Pure domain decision (Req 14.1, 14.3)
    let action = resolve_agent_action(opts.agent_name, persisted.as_ref());

    match action {
        AgentAction::Activate { agent } => {
            // Activate path (Req 14.1, 14.2)
            setup_agent(provisioner, local_fs, &agent, &opts.envs, opts.reporter).await?;

            let overlay = overlay_path(&agent);
            set_active_overlay(provisioner, Some(&overlay)).await?;

            // Start compose with rollback on failure (Req 14.15)
            if let Err(e) = start_compose(provisioner, &agent).await {
                // Rollback: remove overlay symlink
                let _ = set_active_overlay(provisioner, None).await;
                return Err(e);
            }

            // Persist active_agent BEFORE health wait — intentional for large
            // image pull resilience (Req 14.9). On retry, resolve_agent_action
            // returns AlreadyActive (idempotent).
            let mut state = persisted.unwrap_or_else(|| WorkspaceState {
                created_at: Utc::now(),
                image_sha256: None,
                image_source: None,
                active_agent: None,
                provisioning: None,
            });
            state.active_agent = Some(agent.clone());
            state_mgr.save_async(&state).await?;

            // Health check wait (Req 14.10, 14.16, 14.18)
            let health_result = wait_ready(provisioner, opts.reporter, false, "agent ready").await;

            let onboarding = load_onboarding(provisioner, &agent).await;
            let outcome = AgentOutcome::Activated { agent, onboarding };

            match health_result {
                Ok(()) => Ok(ActivateOutcome::Activated(outcome)),
                Err(_) => {
                    // Health check timed out — return ActivatedUnhealthy (Req 14.16)
                    Ok(ActivateOutcome::ActivatedUnhealthy(outcome))
                }
            }
        }

        AgentAction::AlreadyActive { agent } => {
            // No re-activation (Req 14.3)
            let onboarding = load_onboarding(provisioner, &agent).await;
            Ok(ActivateOutcome::AlreadyActive(
                AgentOutcome::AlreadyActive { agent, onboarding },
            ))
        }

        AgentAction::Mismatch { active, requested } => {
            // Return SwapRequired instead of error (Req 14.4)
            Ok(ActivateOutcome::SwapRequired { active, requested })
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Set up an agent inside the VM: transfer artifacts and pull the agent image.
///
/// Reads the agent manifest from the local filesystem, generates compose
/// artifacts, and transfers the `.generated/` folder into the VM.
///
/// Private to this module — single consumer.
async fn setup_agent<P>(
    provisioner: &P,
    local_fs: &impl LocalFs,
    agent_name: &str,
    envs: &[String],
    reporter: &impl ProgressReporter,
) -> Result<()>
where
    P: ShellExecutor + FileTransfer,
{
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
         Install it first: polis agent install --path <path>"
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
        serde_yaml_ng::from_str(&manifest_str).context("failed to parse agent.yaml")?;

    // Build the .env content from the provided envs.
    let env_content = build_env_content(envs);
    let filtered = crate::domain::agent::artifacts::filtered_env(&env_content, &manifest);

    // Write artifacts to a temp dir, then transfer to VM.
    let tmp = tempfile::tempdir().context("creating temp dir for artifact generation")?;
    let generated_dir = tmp
        .path()
        .join("agents")
        .join(agent_name)
        .join(".generated");

    write_artifacts_to_dir(local_fs, &generated_dir, agent_name, &manifest, filtered)?;

    // Remove existing .generated to avoid nested directories from
    // `multipass transfer --recursive` (which nests src inside dest if dest exists).
    let generated_dest = format!("{agent_dir_str}/.generated");
    let _ = provisioner.exec(&["rm", "-rf", &generated_dest]).await;

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
/// Private to this module — single consumer.
async fn start_compose(provisioner: &impl ShellExecutor, agent_name: &str) -> Result<()> {
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = overlay_path(agent_name);

    let out = provisioner
        .exec(&["docker", "compose", "-f", &base, "-f", &overlay, "up", "-d"])
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
    provisioner: &impl ShellExecutor,
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
    let Ok(manifest) = serde_yaml_ng::from_str::<polis_common::agent::AgentManifest>(&content)
    else {
        return Vec::new();
    };
    manifest.spec.onboarding
}

/// Validate environment variable format.
///
/// Returns an error if any env var doesn't match `KEY=VAL` format with non-empty KEY.
/// This validation runs before any VM operations (Req 14.17).
///
/// # Errors
///
/// - Returns error if any env var is missing `=`
/// - Returns error if any env var has an empty key (empty string before first `=`)
pub(crate) fn validate_env_vars(envs: &[String]) -> Result<()> {
    for env in envs {
        let eq_pos = env.find('=');
        match eq_pos {
            None => anyhow::bail!("Invalid environment variable '{env}': missing '='"),
            Some(0) => anyhow::bail!("Invalid environment variable '{env}': empty key"),
            Some(_) => {} // Valid: non-empty key followed by '='
        }
    }
    Ok(())
}

/// Build a `.env`-style string from a list of `KEY=VALUE` strings.
pub(crate) fn build_env_content(envs: &[String]) -> String {
    envs.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_env_vars_accepts_valid_key_val() {
        // Valid KEY=VAL format
        assert!(validate_env_vars(&["FOO=bar".to_string()]).is_ok());
        assert!(validate_env_vars(&["KEY=value".to_string()]).is_ok());
        assert!(validate_env_vars(&["A=B".to_string()]).is_ok());
    }

    #[test]
    fn validate_env_vars_accepts_empty_value() {
        // Empty value is valid (KEY= is allowed)
        assert!(validate_env_vars(&["FOO=".to_string()]).is_ok());
        assert!(validate_env_vars(&["KEY=".to_string()]).is_ok());
    }

    #[test]
    fn validate_env_vars_accepts_value_with_equals() {
        // Value containing '=' is valid (KEY=val=ue)
        assert!(validate_env_vars(&["FOO=bar=baz".to_string()]).is_ok());
        assert!(validate_env_vars(&["KEY=a=b=c".to_string()]).is_ok());
    }

    #[test]
    fn validate_env_vars_accepts_multiple_valid() {
        // Multiple valid env vars
        assert!(
            validate_env_vars(&[
                "FOO=bar".to_string(),
                "BAZ=qux".to_string(),
                "EMPTY=".to_string(),
            ])
            .is_ok()
        );
    }

    #[test]
    fn validate_env_vars_accepts_empty_list() {
        // Empty list is valid
        assert!(validate_env_vars(&[]).is_ok());
    }

    #[test]
    fn validate_env_vars_rejects_missing_equals() {
        // Missing '=' is invalid
        let result = validate_env_vars(&["FOOBAR".to_string()]);
        assert!(result.is_err());
        let err = result.expect_err("should fail for missing '='").to_string();
        assert!(
            err.contains("missing '='"),
            "Error should mention missing '=': {err}"
        );
        assert!(
            err.contains("FOOBAR"),
            "Error should include the invalid value: {err}"
        );
    }

    #[test]
    fn validate_env_vars_rejects_empty_key() {
        // Empty key (starts with '=') is invalid
        let result = validate_env_vars(&["=value".to_string()]);
        assert!(result.is_err());
        let err = result.expect_err("should fail for empty key").to_string();
        assert!(
            err.contains("empty key"),
            "Error should mention empty key: {err}"
        );
        assert!(
            err.contains("=value"),
            "Error should include the invalid value: {err}"
        );
    }

    #[test]
    fn validate_env_vars_rejects_first_invalid_in_list() {
        // First invalid env var in a list should cause failure
        let result = validate_env_vars(&[
            "VALID=ok".to_string(),
            "INVALID".to_string(),
            "ALSO_VALID=yes".to_string(),
        ]);
        assert!(result.is_err());
        let err = result
            .expect_err("should fail for invalid entry")
            .to_string();
        assert!(
            err.contains("INVALID"),
            "Error should include the invalid value: {err}"
        );
    }
}
