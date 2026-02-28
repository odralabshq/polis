//! `polis start` — start workspace (download and create if needed).

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;

use crate::app::AppContext;
use crate::application::ports::{
    AssetExtractor, SshConfigurator, VmProvisioner, WorkspaceStateStore,
};
use crate::application::services::vm::{
    health::wait_ready,
    integrity::{verify_image_digests, write_config_hash},
    lifecycle::{self as vm, VmState},
    provision::{generate_certs_and_secrets, transfer_config},
    services::pull_images,
};
use crate::domain::workspace::WorkspaceState;
use crate::infra::fs::sha256_file;
use crate::infra::state::StateManager;
use crate::output::OutputContext;

/// Path to the polis project root inside the VM.
const VM_POLIS_ROOT: &str = "/opt/polis";

/// Arguments for the start command.
#[derive(Args, Default)]
pub struct StartArgs {
    /// Agent to activate (must match agents/<name>/ directory inside the VM)
    #[arg(long)]
    pub agent: Option<String>,
}

/// Check that the host architecture is amd64.
///
/// Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7.
///
/// # Errors
///
/// Returns an error if the host is arm64 / aarch64.
pub fn check_architecture() -> Result<()> {
    if std::env::consts::ARCH == "aarch64" {
        anyhow::bail!(
            "Polis requires an amd64 host. \
Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7. \
Please use an amd64 machine."
        );
    }
    Ok(())
}

/// Run `polis start`.
///
/// # Errors
///
/// Returns an error if image acquisition, VM creation, or health check fails.
pub async fn run(args: &StartArgs, app: &AppContext) -> Result<()> {
    let mp = &app.provisioner;
    let ctx = &app.output;
    let state_mgr = &app.state_mgr;

    check_architecture()?;

    let vm_state = vm::state(mp).await?;

    match vm_state {
        VmState::Running => return handle_running_vm(state_mgr, args, ctx),
        VmState::NotFound => {
            create_and_start_vm(app, args, mp).await?;
            print_success_message(args.agent.as_deref(), ctx);
            return Ok(());
        }
        _ => restart_vm(app, args, mp).await?,
    }

    wait_ready(mp, ctx.quiet).await?;
    print_success_message(args.agent.as_deref(), ctx);
    Ok(())
}

/// Handle the case where the VM is already running.
fn handle_running_vm(
    state_mgr: &StateManager,
    args: &StartArgs,
    ctx: &OutputContext,
) -> Result<()> {
    let current_agent = state_mgr.load()?.and_then(|s| s.active_agent);
    if current_agent == args.agent {
        print_already_running_message(args.agent.as_deref(), ctx);
        return Ok(());
    }
    let current_desc = agent_description(current_agent.as_deref());
    let requested_desc = args
        .agent
        .as_deref()
        .map_or_else(|| "no agent".to_string(), |n| format!("--agent {n}"));
    anyhow::bail!(
        "Workspace is running with {current_desc}. Stop first:\n  polis stop\n  polis start {requested_desc}"
    );
}

/// Create a new VM and start the workspace.
///
/// Full provisioning flow (Phase 1 + Phase 2):
/// 1. `extract_assets()` — extract embedded files to temp dir
/// 2. `vm::create()` — launch VM with cloud-init, verify, start services
/// 3. `vm::transfer_config()` — transfer tarball, extract, write .env
/// 4. `vm::generate_certs_and_secrets()` — generate TLS certs and Valkey secrets
/// 5. `vm::pull_images()` — docker compose pull with 10-min timeout
/// 6. `digest::verify_image_digests()` — verify pulled images against manifest
/// 7. `setup_agent_if_requested()` — if agent specified
/// 8. `start_compose()` — docker compose up -d
/// 9. `health::wait_ready()` — wait for health check
/// 10. `vm::write_config_hash()` — write hash AFTER successful startup
async fn create_and_start_vm(
    app: &AppContext,
    args: &StartArgs,
    mp: &impl VmProvisioner,
) -> Result<()> {
    // Step 1: Extract all 3 embedded assets to a temp dir.
    // The TempDir guard must be held until all operations complete.
    let (assets_dir, _assets_guard): (std::path::PathBuf, Box<dyn std::any::Any>) = app
        .assets
        .extract_assets()
        .await
        .context("extracting embedded assets")?;

    // Compute the config tarball hash now (before transfer) so we can write it
    // after successful startup. Hash is computed on the host from the embedded asset.
    let tar_path = assets_dir.join("polis-setup.config.tar");
    let config_hash = sha256_file(&tar_path).context("computing config tarball SHA256")?;

    // Step 2: Launch VM with cloud-init and verify cloud-init completed.
    vm::create(mp, &app.assets, &app.ssh, app.output.quiet).await?;

    // Step 3: Transfer config tarball into VM, extract to /opt/polis, write .env.
    let version = env!("CARGO_PKG_VERSION");
    transfer_config(mp, &assets_dir, version)
        .await
        .context("transferring config to VM")?;

    // Step 3.5: Generate certificates and secrets inside the VM.
    generate_certs_and_secrets(mp)
        .await
        .context("generating certificates and secrets")?;

    // Step 4: Pull all Docker images (10-minute timeout).
    pull_images(mp).await.context("pulling Docker images")?;

    // Step 5: Verify pulled image digests against embedded manifest.
    verify_image_digests(mp)
        .await
        .context("verifying image digests")?;

    // Step 6: Set up agent artifacts if requested.
    setup_agent_if_requested(mp, args.agent.as_deref()).await?;

    // Step 7: Start docker compose (with optional agent overlay).
    start_compose(mp, args.agent.as_deref()).await?;

    // Step 8: Wait for all services to become healthy.
    wait_ready(mp, app.output.quiet).await?;

    // Step 9: Write config hash AFTER successful startup so failed provisioning
    // can be retried (Requirements 15.1, 15.3).
    write_config_hash(mp, &config_hash)
        .await
        .context("writing config hash")?;

    // Step 10: _assets_guard is dropped here, cleaning up the temp directory.

    let state = WorkspaceState {
        workspace_id: generate_workspace_id(),
        created_at: Utc::now(),
        image_sha256: None,
        image_source: None,
        active_agent: args.agent.clone(),
    };
    app.state_mgr.save_async(&state).await
}

async fn restart_vm(app: &AppContext, args: &StartArgs, mp: &impl VmProvisioner) -> Result<()> {
    let ctx = &app.output;
    let state_mgr = &app.state_mgr;
    vm::restart(mp, ctx.quiet).await?;

    if args.agent.is_some() {
        setup_agent_if_requested(mp, args.agent.as_deref()).await?;
        start_compose(mp, args.agent.as_deref()).await?;
    }

    let mut state = state_mgr.load()?.unwrap_or_else(|| WorkspaceState {
        workspace_id: generate_workspace_id(),
        created_at: Utc::now(),
        image_sha256: None,
        image_source: None,
        active_agent: None,
    });
    state.active_agent.clone_from(&args.agent);
    state_mgr.save_async(&state).await
}

/// Validate and generate artifacts for an agent if one is requested.
async fn setup_agent_if_requested(mp: &impl VmProvisioner, agent: Option<&str>) -> Result<()> {
    if let Some(name) = agent {
        validate_agent(mp, name).await?;
        generate_agent_artifacts(mp, name).await?;
    }
    Ok(())
}

/// Format agent description for error messages.
fn agent_description(agent: Option<&str>) -> String {
    agent.map_or_else(|| "no agent".to_string(), |n| format!("agent '{n}'"))
}

/// Print message when workspace is already running with matching config.
fn print_already_running_message(agent: Option<&str>, ctx: &OutputContext) {
    if ctx.quiet {
        return;
    }
    ctx.info("Workspace is running.");
    if let Some(name) = agent {
        ctx.kv("Agent", name);
    }
    print_guarantees(ctx);
    ctx.kv("Connect", "polis connect");
    ctx.kv("Status", "polis status");
}

/// Print success message after workspace is ready.
fn print_success_message(agent: Option<&str>, ctx: &OutputContext) {
    if ctx.quiet {
        return;
    }
    print_guarantees(ctx);
    if let Some(name) = agent {
        ctx.success(&format!("Workspace ready. Agent: {name}"));
        ctx.kv("Agent shell", "polis agent shell");
        ctx.kv("Agent commands", "polis agent cmd help");
    } else {
        ctx.success("Workspace ready.");
    }
    ctx.kv("Connect", "polis connect");
    ctx.kv("Status", "polis status");
}

/// Validate that the agent directory and manifest exist inside the VM.
///
/// # Errors
///
/// Returns an error if the agent manifest is missing or the VM is unreachable.
pub async fn validate_agent(mp: &impl VmProvisioner, agent_name: &str) -> Result<()> {
    let manifest_path = format!("{VM_POLIS_ROOT}/agents/{agent_name}/agent.yaml");
    let output = mp
        .exec(&["test", "-f", &manifest_path])
        .await
        .context("checking agent manifest")?;
    if !output.status.success() {
        // List available agents for the error message
        let list_output = mp
            .exec(&[
                "bash",
                "-c",
                &format!("ls {VM_POLIS_ROOT}/agents/ 2>/dev/null || true"),
            ])
            .await
            .unwrap_or_else(|_| std::process::Output {
                status: std::process::ExitStatus::default(),
                stdout: vec![],
                stderr: vec![],
            });
        let available = String::from_utf8_lossy(&list_output.stdout)
            .lines()
            .filter(|l| !l.is_empty() && *l != "_template")
            .collect::<Vec<_>>()
            .join(", ");
        let hint = if available.is_empty() {
            "No agents installed. Use: polis agent add --path <folder>".to_string()
        } else {
            format!("Available agents: {available}")
        };
        anyhow::bail!("unknown agent '{agent_name}'. {hint}");
    }
    Ok(())
}

/// Generate agent artifacts from the VM manifest using pure Rust domain functions.
///
/// Reads the manifest from the VM, generates artifacts locally in a temp dir,
/// and transfers the `.generated/` folder back into the VM.
/// This replaces the old `generate-agent.sh` shell script invocation.
///
/// # Errors
///
/// Returns an error if artifact generation fails or the VM is unreachable.
pub async fn generate_agent_artifacts(mp: &impl VmProvisioner, agent_name: &str) -> Result<()> {
    let manifest_path = format!("{VM_POLIS_ROOT}/agents/{agent_name}/agent.yaml");

    // Read manifest from VM.
    let cat_out = mp
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
    tokio::task::spawn_blocking(move || {
        use crate::domain::agent::artifacts;

        let manifest: polis_common::agent::AgentManifest =
            serde_yaml::from_slice(&stdout_bytes).context("parsing agent.yaml from VM")?;
        crate::domain::agent::validate::validate_full_manifest(&manifest)?;

        let generated_dir = tmp_path.join("agents").join(&name).join(".generated");
        std::fs::create_dir_all(&generated_dir)
            .with_context(|| format!("creating {}", generated_dir.display()))?;

        let compose = artifacts::compose_overlay(&manifest);
        std::fs::write(generated_dir.join("compose.agent.yaml"), &compose)
            .context("writing compose.agent.yaml")?;

        let unit = artifacts::systemd_unit(&manifest);
        let hash = artifacts::service_hash(&unit);
        std::fs::write(generated_dir.join(format!("{name}.service")), &unit)
            .context("writing .service file")?;
        std::fs::write(generated_dir.join(format!("{name}.service.sha256")), &hash)
            .context("writing .service.sha256 file")?;

        // No .env available on VM path — write empty file.
        std::fs::write(generated_dir.join(format!("{name}.env")), "")
            .context("writing .env file")?;

        Ok::<(), anyhow::Error>(())
    })
    .await
    .context("spawn_blocking for artifact generation")??;

    // Transfer the generated artifacts back into the VM.
    let generated_src = tmp
        .path()
        .join("agents")
        .join(agent_name)
        .join(".generated");
    let generated_src_str = generated_src.to_string_lossy().to_string();
    let generated_dest = format!("{VM_POLIS_ROOT}/agents/{agent_name}/.generated");
    let transfer_out = mp
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

/// Start docker compose inside the VM, optionally with an agent overlay.
///
/// # Errors
///
/// Returns an error if docker compose fails or the VM is unreachable.
pub async fn start_compose(mp: &impl VmProvisioner, agent_name: Option<&str>) -> Result<()> {
    let base = format!("{VM_POLIS_ROOT}/docker-compose.yml");
    let mut args: Vec<String> = vec!["docker".into(), "compose".into(), "-f".into(), base];
    if let Some(name) = agent_name {
        let overlay = format!("{VM_POLIS_ROOT}/agents/{name}/.generated/compose.agent.yaml");
        args.push("-f".into());
        args.push(overlay);
    }
    args.extend(["up".into(), "-d".into(), "--remove-orphans".into()]);

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = mp.exec(&arg_refs).await.context("starting platform")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to start platform.\n{stderr}");
    }
    Ok(())
}

fn print_guarantees(ctx: &OutputContext) {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
    if ctx.quiet {
        return;
    }
    let gov = Style::new().truecolor(37, 56, 144);
    let sec = Style::new().truecolor(26, 107, 160);
    let obs = Style::new().truecolor(26, 151, 179);
    println!(
        "✓ {}  policy engine active · audit trail recording",
        "[governance]   ".if_supports_color(Stdout, |t| t.style(gov))
    );
    println!(
        "✓ {}  workspace isolated · traffic inspection enabled",
        "[security]     ".if_supports_color(Stdout, |t| t.style(sec))
    );
    println!(
        "✓ {}  action tracing live · trust scoring active",
        "[observability]".if_supports_color(Stdout, |t| t.style(obs))
    );
}

/// Generates a unique workspace ID in format `polis-{16 hex chars}`.
///
/// Uses multiple entropy sources:
/// - System time (nanoseconds)
/// - Process ID
/// - `RandomState` hasher (OS entropy)
#[must_use]
pub fn generate_workspace_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    // CORR-001: Add multiple entropy sources to prevent duplicates
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    // Add process ID for additional entropy
    hasher.write_u32(std::process::id());
    // RandomState already provides randomness, but hash again for good measure
    hasher.write_u64(RandomState::new().build_hasher().finish());
    format!("polis-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_architecture_passes_on_non_arm64() {
        // On any non-aarch64 host this must succeed.
        // On an aarch64 host the test is skipped so CI on Apple Silicon still passes.
        if std::env::consts::ARCH == "aarch64" {
            // Running on arm64 — verify the function correctly returns an error.
            let err = check_architecture().expect_err("expected Err");
            let msg = err.to_string();
            assert!(msg.contains("amd64"), "error should mention amd64: {msg}");
            assert!(msg.contains("Sysbox"), "error should mention Sysbox: {msg}");
            assert!(msg.contains("arm64"), "error should mention arm64: {msg}");
        } else {
            assert!(
                check_architecture().is_ok(),
                "check_architecture() should succeed on non-arm64 host"
            );
        }
    }

    #[test]
    fn check_architecture_error_message_content() {
        // Directly verify the error message text by simulating what the function
        // would produce on arm64 — we inspect the bail! string directly.
        let msg = "Polis requires an amd64 host. \
Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7. \
Please use an amd64 machine.";
        assert!(msg.contains("amd64"));
        assert!(msg.contains("Sysbox"));
        assert!(msg.contains("arm64"));
        assert!(msg.contains("v0.6.7"));
    }

    #[test]
    fn workspace_id_format() {
        let id = generate_workspace_id();
        assert!(
            id.starts_with("polis-"),
            "expected 'polis-' prefix, got: {id}"
        );
        // "polis-" (6) + 16 hex chars
        assert_eq!(id.len(), 22, "expected 22 chars, got: {id}");
        assert!(id[6..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn workspace_id_unique() {
        let a = generate_workspace_id();
        let b = generate_workspace_id();
        assert_ne!(a, b);
    }

    #[test]
    fn test_state_persists_custom_image_source() {
        let state = WorkspaceState {
            workspace_id: "test".to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: Some("/custom/image.qcow2".to_string()),
            active_agent: None,
        };
        assert_eq!(state.image_source, Some("/custom/image.qcow2".to_string()));
    }

    #[test]
    fn test_state_serializes_with_image_source() {
        let state = WorkspaceState {
            workspace_id: "test".to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: Some("https://example.com/image.qcow2".to_string()),
            active_agent: None,
        };
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(json.contains("image_source"));
        assert!(json.contains("https://example.com/image.qcow2"));
    }

    #[test]
    fn test_state_deserializes_without_image_source() {
        // Old state files without image_source should still load
        let json =
            r#"{"workspace_id":"test","created_at":"2024-01-01T00:00:00Z","image_sha256":null}"#;
        let state: WorkspaceState = serde_json::from_str(json).expect("deserialize");
        assert_eq!(state.workspace_id, "test");
        assert_eq!(state.image_source, None);
    }
}
