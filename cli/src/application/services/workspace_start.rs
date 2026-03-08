//! Application service — workspace start use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

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
    AssetExtractor, FileHasher, HostKeyExtractor, LocalFs, ProgressReporter, ShellExecutor,
    SshConfigurator, VmProvisioner, WorkspaceStateStore,
};
use crate::application::services::vm::{
    health::wait_ready,
    integrity::{verify_image_digests, write_config_hash},
    lifecycle::{self as vm, VmState},
    provision::{generate_certs_and_secrets, transfer_config},
    services::pull_images,
};
use crate::domain::workspace::{ACTIVE_OVERLAY_PATH, READY_MARKER_PATH};
use crate::domain::workspace::{VM_ROOT, WorkspaceState};

/// Write the VM's external IP to `/opt/polis/.vm-ip` and append it to `.env`
/// so containers can reference it via `$POLIS_VM_IP`.
async fn persist_vm_ip(
    mp: &(impl crate::application::ports::InstanceInspector + ShellExecutor),
) -> Result<()> {
    let ip = vm::resolve_vm_ip(mp).await?;
    // Write standalone file for scripts
    mp.exec(&[
        "bash",
        "-c",
        &format!("printf '%s\\n' '{ip}' > /opt/polis/.vm-ip"),
    ])
    .await
    .context("writing .vm-ip")?;
    // Ensure POLIS_VM_IP is in .env (replace if exists, append if not)
    let script = format!(
        "sed -i '/^POLIS_VM_IP=/d' /opt/polis/.env 2>/dev/null; printf '%s\\n' 'POLIS_VM_IP={ip}' >> /opt/polis/.env"
    );
    mp.exec(&["bash", "-c", &script])
        .await
        .context("writing POLIS_VM_IP to .env")?;
    Ok(())
}

async fn persist_agent_metadata(
    mp: &impl ShellExecutor,
    metadata: &polis_common::agent::AgentMetadata,
) -> Result<()> {
    write_control_plane_env(
        mp,
        &[
            ("POLIS_AGENT_NAME", Some(metadata.name.as_str())),
            ("POLIS_AGENT_VERSION", Some(metadata.version.as_str())),
            (
                "POLIS_AGENT_DISPLAY_NAME",
                Some(metadata.display_name.as_str()),
            ),
        ],
    )
    .await
}

async fn clear_agent_metadata(mp: &impl ShellExecutor) -> Result<()> {
    write_control_plane_env(
        mp,
        &[
            ("POLIS_AGENT_NAME", None),
            ("POLIS_AGENT_VERSION", None),
            ("POLIS_AGENT_DISPLAY_NAME", None),
        ],
    )
    .await
}

async fn write_control_plane_env(
    mp: &impl ShellExecutor,
    entries: &[(&str, Option<&str>)],
) -> Result<()> {
    let cleanup = entries
        .iter()
        .map(|(key, _)| format!("/^{key}=/d"))
        .collect::<Vec<_>>()
        .join(";");
    let appends = entries
        .iter()
        .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
        .map(|line| shell_single_quote(&line))
        .collect::<Vec<_>>();
    let append_cmd = if appends.is_empty() {
        String::new()
    } else {
        format!("; printf '%s\\n' {} >> /opt/polis/.env", appends.join(" "))
    };
    let script = format!(
        "touch /opt/polis/.env; sed -i '{cleanup}' /opt/polis/.env 2>/dev/null || true{append_cmd}"
    );
    mp.exec(&["bash", "-c", &script])
        .await
        .context("updating agent metadata in .env")?;
    Ok(())
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Outcome of the `start_workspace` use-case.
#[derive(Debug)]
pub enum StartOutcome {
    /// Workspace was already running with the same agent config.
    AlreadyRunning {
        agent: Option<String>,
        onboarding: Vec<polis_common::agent::OnboardingStep>,
    },
    /// Workspace was freshly created and started.
    Created {
        agent: Option<String>,
        onboarding: Vec<polis_common::agent::OnboardingStep>,
    },
    /// A stopped workspace was restarted.
    Restarted {
        agent: Option<String>,
        onboarding: Vec<polis_common::agent::OnboardingStep>,
    },
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
    ssh: &(impl SshConfigurator + HostKeyExtractor),
    hasher: &impl FileHasher,
    local_fs: &impl LocalFs,
    opts: StartOptions<'_, impl crate::application::ports::ProgressReporter>,
) -> Result<StartOutcome> {
    let reporter = opts.reporter;
    let StartOptions {
        agent,
        envs,
        assets_dir,
        version,
        ..
    } = opts;
    crate::domain::workspace::check_architecture()?;

    let vm_state = vm::state(provisioner).await?;

    match vm_state {
        VmState::Running => {
            handle_running_vm(
                provisioner,
                state_mgr,
                local_fs,
                reporter,
                agent,
                envs,
                assets_dir,
                version,
            )
            .await
        }
        VmState::NotFound => {
            let onboarding = create_and_start_vm(
                provisioner,
                state_mgr,
                assets,
                ssh,
                hasher,
                local_fs,
                StartOptions {
                    reporter,
                    agent,
                    envs,
                    assets_dir,
                    version,
                },
            )
            .await?;
            Ok(StartOutcome::Created {
                agent: agent.map(str::to_owned),
                onboarding,
            })
        }
        _ => {
            let onboarding = restart_vm(
                provisioner,
                state_mgr,
                local_fs,
                reporter,
                agent,
                envs,
                assets_dir,
                version,
            )
            .await?;
            let msg = agent.map_or_else(
                || "workspace ready".to_string(),
                |n| format!("workspace ready with agent: {n}"),
            );
            wait_ready(provisioner, reporter, false, &msg).await?;
            Ok(StartOutcome::Restarted {
                agent: agent.map(str::to_owned),
                onboarding,
            })
        }
    }
}

/// Handle the case where the VM is already running.
///
/// When no agent is currently active and one is requested, set it up
/// in-place without stopping the VM. This avoids a stop/start cycle
/// which triggers the Hyper-V Default Switch DHCP bug on Windows.
#[allow(clippy::too_many_arguments)]
async fn handle_running_vm(
    provisioner: &impl VmProvisioner,
    state_mgr: &impl WorkspaceStateStore,
    local_fs: &impl LocalFs,
    reporter: &impl ProgressReporter,
    agent: Option<&str>,
    envs: Vec<String>,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<StartOutcome> {
    let current_agent = state_mgr.load_async().await?.and_then(|s| s.active_agent);
    if current_agent.as_deref() == agent {
        return Ok(StartOutcome::AlreadyRunning {
            agent: agent.map(str::to_owned),
            onboarding: vec![],
        });
    }

    // Allow adding an agent to a running workspace that has no agent.
    if current_agent.is_none()
        && let Some(name) = agent
    {
        // Re-transfer config so the VM has the latest agents directory.
        // This handles the case where the VM was created by an older binary
        // that embedded a different config tarball.
        reporter.begin_stage("updating workspace config...");
        transfer_config(provisioner, assets_dir, version)
            .await
            .context("transferring config to VM")?;
        reporter.complete_stage();

        reporter.begin_stage(&format!("installing agent '{name}'..."));
        let onboarding = setup_agent(provisioner, local_fs, name, &envs).await?;

        // Persist VM IP for container access.
        persist_vm_ip(provisioner).await.ok(); // best-effort

        // Update symlink for future reboots, then start via compose directly.
        let overlay = crate::domain::agent::overlay_path(name);
        set_active_overlay(provisioner, Some(&overlay)).await?;
        start_compose(provisioner, Some(name)).await?;

        // Persist state before health wait so the CLI tracks the agent
        // even if health polling times out (e.g. first-time install).
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

        let msg = format!("workspace ready with agent: {name}");
        wait_ready(provisioner, reporter, false, &msg).await?;

        return Ok(StartOutcome::Restarted {
            agent: Some(name.to_owned()),
            onboarding,
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
    assets: &impl AssetExtractor,
    ssh: &(impl SshConfigurator + HostKeyExtractor),
    hasher: &impl FileHasher,
    local_fs: &impl LocalFs,
    opts: StartOptions<'_, impl crate::application::ports::ProgressReporter>,
) -> Result<Vec<polis_common::agent::OnboardingStep>> {
    let reporter = opts.reporter;
    let StartOptions {
        agent,
        envs,
        assets_dir,
        version,
        ..
    } = opts;
    // Step 1: Compute config hash before transfer.
    let tar_path = assets_dir.join("polis-setup.config.tar");
    let config_hash = hasher
        .sha256_file(&tar_path)
        .context("computing config tarball SHA256")?;

    reporter.begin_stage("preparing workspace...");

    // Step 2: Launch VM with cloud-init.
    vm::create(provisioner, assets, ssh, local_fs, ssh, reporter, true).await?;

    // Step 3: Transfer config tarball.
    reporter.begin_stage("securing workspace...");
    transfer_config(provisioner, assets_dir, version)
        .await
        .context("transferring config to VM")?;

    // Step 3b: Persist VM IP for container access.
    persist_vm_ip(provisioner).await.ok(); // best-effort

    // Step 4: Generate certificates and secrets.
    generate_certs_and_secrets(provisioner)
        .await
        .context("generating certificates and secrets")?;

    // Step 5: Pull Docker images.
    reporter.begin_stage("verifying components...");
    pull_images(provisioner, reporter)
        .await
        .context("pulling Docker images")?;

    // Step 6: Verify image digests.
    verify_image_digests(provisioner, assets, reporter)
        .await
        .context("verifying image digests")?;

    // Step 7: Set up agent if requested.
    let (overlay, onboarding) = if let Some(name) = agent {
        reporter.begin_stage(&format!("installing agent '{name}'..."));
        let steps = setup_agent(provisioner, local_fs, name, &envs).await?;
        (Some(crate::domain::agent::overlay_path(name)), steps)
    } else {
        clear_agent_metadata(provisioner)
            .await
            .context("clearing active agent metadata")?;
        (None, vec![])
    };

    // Step 8: Set active overlay symlink and start via systemd.
    set_active_overlay(provisioner, overlay.as_deref()).await?;
    set_ready_marker(provisioner, true).await?;
    provisioner
        .exec(&["sudo", "systemctl", "start", "polis"])
        .await
        .context("starting polis service")?;

    // Step 9: Wait for health.
    let msg = agent.map_or_else(
        || "workspace ready".to_string(),
        |n| format!("workspace ready with agent: {n}"),
    );
    wait_ready(provisioner, reporter, false, &msg).await?;

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
    state_mgr.save_async(&state).await?;

    Ok(onboarding)
}

/// Restart a stopped VM.
#[allow(clippy::too_many_arguments)]
async fn restart_vm(
    provisioner: &impl VmProvisioner,
    state_mgr: &impl WorkspaceStateStore,
    local_fs: &impl LocalFs,
    reporter: &impl ProgressReporter,
    agent: Option<&str>,
    envs: Vec<String>,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<Vec<polis_common::agent::OnboardingStep>> {
    // Start the VM (systemd polis.service is gated by .ready which was cleared).
    reporter.begin_stage("starting workspace...");
    vm::start(provisioner).await?;
    reporter.complete_stage();

    // Re-transfer config so the VM has the latest agents, scripts, and
    // compose files from the current binary. This handles version upgrades
    // and cases where the VM was created by an older binary.
    reporter.begin_stage("updating workspace config...");
    transfer_config(provisioner, assets_dir, version)
        .await
        .context("transferring config to VM")?;
    reporter.complete_stage();

    // Persist VM IP for container access.
    persist_vm_ip(provisioner).await.ok(); // best-effort

    // Pull images BEFORE starting services.
    reporter.begin_stage("verifying components...");
    pull_images(provisioner, reporter)
        .await
        .context("pulling Docker images")?;

    let (overlay, onboarding) = if let Some(name) = agent {
        reporter.begin_stage(&format!("installing agent '{name}'..."));
        let steps = setup_agent(provisioner, local_fs, name, &envs).await?;
        (Some(crate::domain::agent::overlay_path(name)), steps)
    } else {
        clear_agent_metadata(provisioner)
            .await
            .context("clearing active agent metadata")?;
        (None, vec![])
    };

    // Set overlay symlink, then gate-open and start services.
    set_active_overlay(provisioner, overlay.as_deref()).await?;
    set_ready_marker(provisioner, true).await?;
    reporter.begin_stage("starting services...");
    provisioner
        .exec(&["sudo", "systemctl", "start", "polis"])
        .await
        .context("starting polis service")?;
    reporter.complete_stage();

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
    state_mgr.save_async(&state).await?;

    Ok(onboarding)
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
) -> Result<Vec<polis_common::agent::OnboardingStep>> {
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
    // Generate artifacts in a temp dir under ~/polis/tmp so the Multipass
    // snap daemon (AppArmor-confined) can read it for transfer.
    let base = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join("polis")
        .join("tmp");
    local_fs
        .create_dir_all(&base)
        .context("creating ~/polis/tmp")?;
    let tmp = tempfile::Builder::new()
        .prefix("polis-agent-")
        .tempdir_in(&base)
        .context("creating temp dir for agent artifacts")?;
    let tmp_path = tmp.path().to_path_buf();

    let manifest: polis_common::agent::AgentManifest =
        serde_yaml::from_slice(&stdout_bytes).context("parsing agent.yaml from VM")?;
    crate::domain::agent::validate::validate_full_manifest(&manifest)?;

    let onboarding = manifest.spec.onboarding.clone();

    persist_agent_metadata(provisioner, &manifest.metadata)
        .await
        .context("persisting active agent metadata")?;

    let generated_dir = tmp_path.join("agents").join(&name).join(".generated");

    // Write environment variables to the agent's .env file, forcing LF line endings.
    let env_content = if envs.is_empty() {
        String::new()
    } else {
        format!("{}\n", envs.join("\n")).replace("\r\n", "\n")
    };

    crate::application::services::agent_crud::write_artifacts_to_dir(
        local_fs,
        &generated_dir,
        &name,
        &manifest,
        env_content,
    )?;

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

    Ok(onboarding)
}

/// Set or remove the active compose overlay symlink.
async fn set_active_overlay(
    provisioner: &impl ShellExecutor,
    overlay_path: Option<&str>,
) -> Result<()> {
    match overlay_path {
        Some(path) => {
            provisioner
                .exec(&["ln", "-sf", path, ACTIVE_OVERLAY_PATH])
                .await
                .context("creating overlay symlink")?;
        }
        None => {
            provisioner
                .exec(&["rm", "-f", ACTIVE_OVERLAY_PATH])
                .await
                .context("removing overlay symlink")?;
        }
    }
    Ok(())
}

/// Set or clear the ready marker that gates `polis.service` auto-start.
async fn set_ready_marker(provisioner: &impl ShellExecutor, enabled: bool) -> Result<()> {
    if enabled {
        provisioner
            .exec(&["touch", READY_MARKER_PATH])
            .await
            .context("creating ready marker")?;
    } else {
        provisioner
            .exec(&["rm", "-f", READY_MARKER_PATH])
            .await
            .context("removing ready marker")?;
    }
    Ok(())
}

/// Start docker compose with optional agent overlay.
async fn start_compose<P: VmProvisioner>(provisioner: &P, agent_name: Option<&str>) -> Result<()> {
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let mut args: Vec<String> = vec![
        "timeout".into(),
        "120".into(),
        "docker".into(),
        "compose".into(),
        "-f".into(),
        base,
    ];
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
        if output.status.code() == Some(124) {
            anyhow::bail!(
                "docker compose up timed out after 2 minutes.\n\
                 Diagnose: polis doctor"
            );
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to start platform.\n{stderr}");
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    use std::{
        process::{Output, Stdio},
        sync::Mutex,
    };

    use polis_common::agent::AgentMetadata;

    use crate::application::ports::ShellExecutor;
    use crate::application::services::vm::test_support::ok_output;

    #[derive(Default)]
    struct RecordingShell {
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl RecordingShell {
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().expect("calls lock").clone()
        }
    }

    impl ShellExecutor for RecordingShell {
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(args.iter().map(ToString::to_string).collect());
            Ok(ok_output(b""))
        }

        async fn exec_with_stdin(&self, _args: &[&str], _input: &[u8]) -> Result<Output> {
            anyhow::bail!("exec_with_stdin not expected in test")
        }

        fn exec_spawn(&self, _args: &[&str]) -> Result<tokio::process::Child> {
            let mut command = tokio::process::Command::new("cmd");
            command
                .arg("/C")
                .arg("exit 0")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            command.spawn().context("failed to spawn placeholder child")
        }

        async fn exec_status(&self, _args: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("exec_status not expected in test")
        }
    }

    #[test]
    fn shell_single_quote_escapes_single_quotes() {
        assert_eq!(shell_single_quote("o'clock"), "'o'\"'\"'clock'");
    }

    #[tokio::test]
    async fn persist_agent_metadata_writes_control_plane_env_vars() {
        let shell = RecordingShell::default();
        let metadata = AgentMetadata {
            name: "openclaw".to_string(),
            display_name: "OpenClaw Agent".to_string(),
            version: "1.2.3".to_string(),
            description: "test agent".to_string(),
            author: None,
            license: None,
            provider: None,
            capabilities: Vec::new(),
        };

        persist_agent_metadata(&shell, &metadata)
            .await
            .expect("persist metadata");

        let calls = shell.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "bash");
        assert_eq!(calls[0][1], "-c");

        let script = &calls[0][2];
        assert!(script.contains("POLIS_AGENT_NAME=openclaw"), "{script}");
        assert!(script.contains("POLIS_AGENT_VERSION=1.2.3"), "{script}");
        assert!(
            script.contains("POLIS_AGENT_DISPLAY_NAME=OpenClaw Agent"),
            "{script}"
        );
    }

    #[tokio::test]
    async fn clear_agent_metadata_only_removes_existing_vars() {
        let shell = RecordingShell::default();

        clear_agent_metadata(&shell)
            .await
            .expect("clear agent metadata");

        let calls = shell.calls();
        assert_eq!(calls.len(), 1);
        let script = &calls[0][2];
        assert!(script.contains("/^POLIS_AGENT_NAME=/d"), "{script}");
        assert!(script.contains("/^POLIS_AGENT_VERSION=/d"), "{script}");
        assert!(script.contains("/^POLIS_AGENT_DISPLAY_NAME=/d"), "{script}");
        assert!(!script.contains("printf '%s\\n'"), "{script}");
    }
}
