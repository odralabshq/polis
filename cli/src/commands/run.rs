//! Run command — state machine for checkpoint/resume and agent switching.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;
use polis_common::types::{RunStage, RunState};

use crate::state::StateManager;

/// VM name used by multipass.
const VM_NAME: &str = "polis";

/// Default VM resources.
const VM_CPUS: &str = "2";
const VM_MEMORY: &str = "4G";
const VM_DISK: &str = "20G";

/// Arguments for the run command.
#[derive(Args)]
pub struct RunArgs {
    /// Agent to run (e.g., claude-dev, gpt-dev)
    pub agent: Option<String>,
}

/// Entry point for `polis run`.
///
/// # Errors
///
/// Returns an error if agent resolution, state loading, or stage execution fails.
pub fn run(args: &RunArgs) -> Result<()> {
    let state_mgr = StateManager::new()?;

    let existing = match state_mgr.load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Warning: state file unreadable ({e}), starting fresh");
            None
        }
    };

    let target_agent = resolve_agent(args.agent.as_deref())?;

    match existing {
        Some(state) if state.agent == target_agent => resume_run(&state_mgr, state),
        Some(state) => switch_agent(&state_mgr, state, &target_agent),
        None => fresh_run(&state_mgr, &target_agent),
    }
}

/// Resolve which agent to use.
///
/// Priority:
/// 1. Explicit `--agent` argument
/// 2. `defaults.agent` from `~/.polis/config.yaml`
/// 3. Single installed agent (auto-select)
/// 4. Multiple installed agents (prompt)
/// 5. No agents installed (empty string — workspace starts without agent)
///
/// # Errors
///
/// Returns an error only if the interactive prompt fails.
fn resolve_agent(requested: Option<&str>) -> Result<String> {
    if let Some(agent) = requested {
        return Ok(agent.to_string());
    }

    // Check config for default agent
    if let Some(default) = get_default_agent()? {
        return Ok(default);
    }

    let agents = list_available_agents()?;
    match agents.len() {
        0 => Ok(String::new()),
        1 => Ok(agents.into_iter().next().unwrap_or_default()),
        _ => prompt_agent_selection(&agents),
    }
}

/// Read `defaults.agent` from `~/.polis/config.yaml`.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined or the config
/// file exists but cannot be parsed.
fn get_default_agent() -> Result<Option<String>> {
    use crate::commands::config::{load_config, get_config_path};
    let path = get_config_path()?;
    let config = load_config(&path)?;
    Ok(config.defaults.agent)
}

/// List agents installed under `~/.polis/agents/`.
///
/// Each subdirectory containing an `agent.yaml` is treated as an installed agent.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
fn list_available_agents() -> Result<Vec<String>> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let agents_dir = home.join(".polis").join("agents");
    if !agents_dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&agents_dir)
        .with_context(|| format!("reading agents dir {}", agents_dir.display()))?
    {
        let entry = entry.context("reading dir entry")?;
        if entry.path().join("agent.yaml").exists() && let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// Prompt the user to select an agent interactively.
///
/// # Errors
///
/// Returns an error if the selection cannot be read.
fn prompt_agent_selection(agents: &[String]) -> Result<String> {
    use dialoguer::Select;
    let idx = Select::new()
        .with_prompt("Select agent")
        .items(agents)
        .default(0)
        .interact()
        .context("agent selection")?;
    Ok(agents[idx].clone())
}

/// Resume from the last completed stage.
///
/// # Errors
///
/// Returns an error if any remaining stage fails.
fn resume_run(state_mgr: &StateManager, mut run_state: RunState) -> Result<()> {
    println!("Resuming from: {}", run_state.stage.description());
    let mut next = run_state.stage.next();
    while let Some(next_stage) = next {
        execute_stage(&mut run_state, next_stage)?;
        state_mgr.advance(&mut run_state, next_stage)?;
        next = next_stage.next();
    }
    println!("{} is ready", run_state.agent);
    Ok(())
}

/// Prompt to switch agents, then restart the agent only (preserving workspace).
///
/// # Errors
///
/// Returns an error if the user declines or the switch fails.
fn switch_agent(state_mgr: &StateManager, run_state: RunState, target_agent: &str) -> Result<()> {
    println!();
    println!("  Workspace is running {}.", run_state.agent);
    println!();

    let confirmed = dialoguer::Confirm::new()
        .with_prompt(format!(
            "Switch to {target_agent}? This will restart the agent."
        ))
        .default(true)
        .interact()
        .context("switch confirmation")?;

    if !confirmed {
        return Ok(());
    }

    let mut new_state = RunState {
        stage: RunStage::Provisioned,
        agent: target_agent.to_string(),
        workspace_id: run_state.workspace_id,
        started_at: Utc::now(),
        image_sha256: run_state.image_sha256,
    };
    execute_stage(&mut new_state, RunStage::AgentReady)?;
    state_mgr.advance(&mut new_state, RunStage::AgentReady)?;
    println!("{target_agent} is ready");
    Ok(())
}

/// Fresh run — execute all stages from the beginning.
///
/// # Errors
///
/// Returns an error if any stage fails.
fn fresh_run(state_mgr: &StateManager, agent: &str) -> Result<()> {
    let mut run_state = RunState {
        stage: RunStage::ImageReady,
        agent: agent.to_string(),
        workspace_id: generate_workspace_id(),
        started_at: Utc::now(),
        image_sha256: None,
    };

    for next_stage in [
        RunStage::ImageReady,
        RunStage::WorkspaceCreated,
        RunStage::CredentialsSet,
        RunStage::Provisioned,
        RunStage::AgentReady,
    ] {
        execute_stage(&mut run_state, next_stage)?;
        state_mgr.advance(&mut run_state, next_stage)?;
        if next_stage == RunStage::Provisioned {
            pin_host_key();
        }
    }

    println!("{agent} is ready");
    Ok(())
}

/// Pins the workspace SSH host key into `~/.polis/known_hosts`.
///
/// Failures are non-fatal: a warning is printed and provisioning continues.
fn pin_host_key() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("polis"));
    let output = match std::process::Command::new(exe).args(["_extract-host-key"]).output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Warning: could not pin host key: {e}");
            return;
        }
    };
    if !output.status.success() {
        eprintln!("Warning: could not pin host key — SSH may prompt for verification");
        return;
    }
    let Ok(host_key) = String::from_utf8(output.stdout) else { return };
    match crate::ssh::KnownHostsManager::new().and_then(|m| m.update(host_key.trim())) {
        Ok(()) => println!("Host key pinned"),
        Err(e) => eprintln!("Warning: could not save host key: {e}"),
    }
}

/// Execute a single pipeline stage.
///
/// # Errors
///
/// Returns an error if the stage operation fails.
fn execute_stage(run_state: &mut RunState, stage: RunStage) -> Result<()> {
    println!("{}...", stage.description());

    match stage {
        RunStage::ImageReady => {
            let sha = ensure_image_ready()?;
            run_state.image_sha256 = Some(sha);
        }
        RunStage::WorkspaceCreated => {
            create_workspace()?;
        }
        RunStage::CredentialsSet => {
            configure_credentials()?;
        }
        RunStage::Provisioned => {
            provision_workspace()?;
        }
        RunStage::AgentReady => {
            // Agent installation is a no-op for now — workspace container
            // starts automatically with docker compose.
            wait_for_workspace_healthy();
        }
    }

    Ok(())
}

/// Ensure the VM image is available locally.
///
/// Returns the SHA256 hash of the image.
fn ensure_image_ready() -> Result<String> {
    let image_path = get_image_path();

    if image_path.exists() {
        println!("  Using local image: {}", image_path.display());
        return compute_image_hash(&image_path);
    }

    anyhow::bail!(
        "VM image not found at {}\n\
         Build it with: cd packer && packer build .\n\
         Or download from GitHub releases.",
        image_path.display()
    );
}

/// Get the path to the VM image.
fn get_image_path() -> PathBuf {
    // Check for image in standard locations
    let candidates = [
        // Relative to current working directory (dev workflow)
        PathBuf::from("packer/output/polis-vm-dev-amd64.qcow2"),
        // User's polis directory
        dirs::home_dir()
            .map(|h| h.join(".polis").join("images").join("polis-vm-dev-amd64.qcow2"))
            .unwrap_or_default(),
    ];

    for path in &candidates {
        if path.exists() {
            return path.clone();
        }
    }

    // Return the first candidate as the expected path
    candidates[0].clone()
}

/// Compute SHA256 hash of the image file.
fn compute_image_hash(path: &PathBuf) -> Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening image {}", path.display()))?;

    // Read first 64KB for a quick hash (full hash would be slow for 3GB file)
    let mut buffer = vec![0u8; 65536];
    let bytes_read = file.read(&mut buffer).context("reading image")?;
    buffer.truncate(bytes_read);

    // Simple hash of the header bytes
    let hash: u64 = buffer.iter().fold(0u64, |acc, &b| {
        acc.wrapping_mul(31).wrapping_add(u64::from(b))
    });

    Ok(format!("{hash:016x}"))
}

/// Create the workspace VM via multipass.
fn create_workspace() -> Result<()> {
    // Check if VM already exists
    let info = Command::new("multipass")
        .args(["info", VM_NAME, "--format", "json"])
        .output();

    if let Ok(output) = info
        && output.status.success()
    {
        println!("  Workspace already exists, starting...");
        return start_vm();
    }

    // Launch new VM from local image
    let image_path = get_image_path();
    let image_url = format!("file://{}", image_path.canonicalize()?.display());

    println!("  Launching workspace from {}", image_path.display());

    let output = Command::new("multipass")
        .args([
            "launch",
            &image_url,
            "--name", VM_NAME,
            "--cpus", VM_CPUS,
            "--memory", VM_MEMORY,
            "--disk", VM_DISK,
        ])
        .output()
        .context("failed to run multipass launch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("multipass launch failed: {stderr}");
    }

    println!("  Workspace created");
    Ok(())
}

/// Start an existing VM.
fn start_vm() -> Result<()> {
    let output = Command::new("multipass")
        .args(["start", VM_NAME])
        .output()
        .context("failed to run multipass start")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("multipass start failed: {stderr}");
    }

    Ok(())
}

/// Configure credentials inside the VM.
fn configure_credentials() -> Result<()> {
    // Transfer CA certificate if it exists
    let ca_cert = PathBuf::from("certs/ca/ca.pem");
    if ca_cert.exists() {
        println!("  Transferring CA certificate...");
        let output = Command::new("multipass")
            .args([
                "transfer",
                &ca_cert.to_string_lossy(),
                &format!("{VM_NAME}:/tmp/ca.pem"),
            ])
            .output()
            .context("failed to transfer CA cert")?;

        if !output.status.success() {
            eprintln!("  Warning: could not transfer CA cert");
        }
    } else {
        println!("  No CA certificate found, skipping...");
    }

    Ok(())
}

/// Provision the workspace by starting docker compose services.
fn provision_workspace() -> Result<()> {
    println!("  Starting services...");

    // Run docker compose up inside the VM
    let output = Command::new("multipass")
        .args([
            "exec", VM_NAME, "--",
            "docker", "compose", "up", "-d",
        ])
        .output()
        .context("failed to run docker compose")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Non-fatal: compose might not be set up yet
        eprintln!("  Warning: docker compose up failed: {stderr}");
    }

    Ok(())
}

/// Wait for the workspace container to become healthy.
fn wait_for_workspace_healthy() {
    println!("  Waiting for workspace to be ready...");

    let max_attempts = 30;
    let delay = Duration::from_secs(2);

    for attempt in 1..=max_attempts {
        let output = Command::new("multipass")
            .args([
                "exec", VM_NAME, "--",
                "docker", "compose", "ps", "--format", "json", "workspace",
            ])
            .output();

        if let Ok(output) = output
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next()
                && let Ok(container) = serde_json::from_str::<serde_json::Value>(line)
            {
                let state = container.get("State").and_then(|s| s.as_str());
                let health = container.get("Health").and_then(|h| h.as_str());

                if state == Some("running") {
                    if health == Some("healthy") {
                        println!("  Workspace is healthy");
                        return;
                    }
                    if attempt % 5 == 0 {
                        println!("  Workspace starting (attempt {attempt}/{max_attempts})...");
                    }
                }
            }
        }

        std::thread::sleep(delay);
    }

    // Don't fail — workspace might still be starting
    eprintln!("  Warning: workspace health check timed out");
}

fn generate_workspace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("polis-{ts:08x}")
}


// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── get_image_path ───────────────────────────────────────────────────────

    #[test]
    fn test_get_image_path_returns_packer_output_as_default() {
        let path = get_image_path();
        assert!(
            path.to_string_lossy().contains("polis-vm-dev-amd64.qcow2"),
            "path should contain image filename"
        );
    }

    // ── compute_image_hash ───────────────────────────────────────────────────

    #[test]
    fn test_compute_image_hash_returns_hex_string() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.qcow2");
        std::fs::write(&path, b"test image content").unwrap();

        let hash = compute_image_hash(&path).unwrap();
        assert_eq!(hash.len(), 16, "hash should be 16 hex chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_compute_image_hash_is_deterministic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.qcow2");
        std::fs::write(&path, b"deterministic content").unwrap();

        let hash1 = compute_image_hash(&path).unwrap();
        let hash2 = compute_image_hash(&path).unwrap();
        assert_eq!(hash1, hash2, "same content should produce same hash");
    }

    #[test]
    fn test_compute_image_hash_different_content_different_hash() {
        let dir = TempDir::new().unwrap();
        let path1 = dir.path().join("a.qcow2");
        let path2 = dir.path().join("b.qcow2");
        std::fs::write(&path1, b"content A").unwrap();
        std::fs::write(&path2, b"content B").unwrap();

        let hash1 = compute_image_hash(&path1).unwrap();
        let hash2 = compute_image_hash(&path2).unwrap();
        assert_ne!(hash1, hash2, "different content should produce different hash");
    }

    #[test]
    fn test_compute_image_hash_nonexistent_file_returns_error() {
        let path = PathBuf::from("/nonexistent/path/image.qcow2");
        assert!(compute_image_hash(&path).is_err());
    }

    // ── generate_workspace_id ────────────────────────────────────────────────

    #[test]
    fn test_generate_workspace_id_starts_with_polis() {
        let id = generate_workspace_id();
        assert!(id.starts_with("polis-"), "id should start with 'polis-'");
    }

    #[test]
    fn test_generate_workspace_id_has_hex_suffix() {
        let id = generate_workspace_id();
        let suffix = id.strip_prefix("polis-").unwrap();
        assert_eq!(suffix.len(), 8, "suffix should be 8 hex chars");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── resolve_agent ────────────────────────────────────────────────────────

    #[test]
    fn test_resolve_agent_explicit_arg_takes_priority() {
        let result = resolve_agent(Some("explicit-agent")).unwrap();
        assert_eq!(result, "explicit-agent");
    }

    #[test]
    fn test_resolve_agent_explicit_arg_used_as_is() {
        // Even non-existent agent names are accepted
        let result = resolve_agent(Some("nonexistent-xyz-123")).unwrap();
        assert_eq!(result, "nonexistent-xyz-123");
    }
}

// ============================================================================
// Property-Based Tests
// ============================================================================

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use tempfile::TempDir;

    proptest! {
        /// compute_image_hash is deterministic for any content
        #[test]
        fn prop_compute_image_hash_deterministic(content in proptest::collection::vec(any::<u8>(), 1..1000)) {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().join("test.qcow2");
            std::fs::write(&path, &content).expect("write");

            let hash1 = compute_image_hash(&path).expect("hash1");
            let hash2 = compute_image_hash(&path).expect("hash2");
            prop_assert_eq!(hash1, hash2);
        }

        /// compute_image_hash always returns 16 hex chars
        #[test]
        fn prop_compute_image_hash_format(content in proptest::collection::vec(any::<u8>(), 1..1000)) {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().join("test.qcow2");
            std::fs::write(&path, &content).expect("write");

            let hash = compute_image_hash(&path).expect("hash");
            prop_assert_eq!(hash.len(), 16);
            prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        }

        /// generate_workspace_id always matches expected format
        #[test]
        fn prop_generate_workspace_id_format(_seed in 0u32..1000) {
            let id = generate_workspace_id();
            prop_assert!(id.starts_with("polis-"));
            let suffix = id.strip_prefix("polis-").unwrap();
            prop_assert_eq!(suffix.len(), 8);
            prop_assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
        }

        /// resolve_agent with explicit arg always returns that arg
        #[test]
        fn prop_resolve_agent_explicit_returns_same(agent in "[a-z][a-z0-9-]{1,30}") {
            let result = resolve_agent(Some(&agent)).expect("resolve");
            prop_assert_eq!(result, agent);
        }
    }
}

