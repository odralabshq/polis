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
    use crate::commands::config::{get_config_path, load_config};
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
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let agents_dir = home.join(".polis").join("agents");
    if !agents_dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&agents_dir)
        .with_context(|| format!("reading agents dir {}", agents_dir.display()))?
    {
        let entry = entry.context("reading dir entry")?;
        if entry.path().join("agent.yaml").exists()
            && let Some(name) = entry.file_name().to_str()
        {
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
    // Pre-flight: verify image exists and is intact before touching any state.
    let image_path = get_image_path()?;
    let image_sha = verify_image_at_launch(&image_path)?;

    let mut run_state = RunState {
        stage: RunStage::WorkspaceCreated,
        agent: agent.to_string(),
        workspace_id: generate_workspace_id(),
        started_at: Utc::now(),
        image_sha256: Some(image_sha),
    };

    for next_stage in [
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
    let output = match std::process::Command::new(exe)
        .args(["_extract-host-key"])
        .output()
    {
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
    let Ok(host_key) = String::from_utf8(output.stdout) else {
        return;
    };
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
fn execute_stage(_run_state: &mut RunState, stage: RunStage) -> Result<()> {
    println!("{}...", stage.description());

    match stage {
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

/// Resolve the VM image path.
///
/// Priority:
/// 1. `POLIS_IMAGE` env var (dev/CI override)
/// 2. `~/.polis/images/polis-workspace.qcow2` (standard cache from `polis init`)
///
/// # Errors
///
/// Returns an error if no image is found at either location.
fn get_image_path() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("POLIS_IMAGE") {
        let p = PathBuf::from(&override_path);
        anyhow::ensure!(
            p.exists(),
            "POLIS_IMAGE points to non-existent file: {override_path}"
        );
        return Ok(p);
    }

    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let path = home.join(".polis/images/polis-workspace.qcow2");

    if !path.exists() {
        anyhow::bail!(
            "No workspace image found.\n\n\
             Run 'polis init' to download the image (~3.2 GB)."
        );
    }
    Ok(path)
}

/// Verify image integrity before launching the VM (TOCTOU fix V-005).
///
/// For standard images: re-verify SHA-256 against the stored checksum.
/// For `POLIS_IMAGE` overrides without a sidecar: warn but allow.
///
/// Returns the hex-encoded SHA-256 hash.
///
/// # Errors
///
/// Returns an error if the checksum is missing (non-override), mismatched, or
/// the file cannot be read.
fn verify_image_at_launch(image_path: &std::path::Path) -> Result<String> {
    // The sidecar sits next to the image with an extra ".sha256" extension.
    // e.g. polis-workspace.qcow2 → polis-workspace.qcow2.sha256
    let mut sidecar = image_path.as_os_str().to_owned();
    sidecar.push(".sha256");
    let checksum_path = std::path::PathBuf::from(sidecar);

    if !checksum_path.exists() {
        if std::env::var("POLIS_IMAGE").is_ok() {
            eprintln!("Warning: using custom image from POLIS_IMAGE (no checksum verification)");
            return crate::commands::init::sha256_file(image_path);
        }
        anyhow::bail!("Image checksum missing. Re-run: polis init");
    }

    let expected = std::fs::read_to_string(&checksum_path)
        .with_context(|| format!("reading checksum {}", checksum_path.display()))?;
    let expected = expected.split_whitespace().next().unwrap_or_default().to_string();

    println!("  Verifying image integrity...");
    let actual = crate::commands::init::sha256_file(image_path)?;

    anyhow::ensure!(
        actual == expected,
        "Image integrity check failed (file may have been modified).\n\
         Expected: {expected}\n\
         Actual:   {actual}\n\n\
         Re-download with: polis init --force"
    );
    Ok(actual)
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
    let image_path = get_image_path()?;
    let image_url = format!("file://{}", image_path.canonicalize()?.display());

    println!("  Launching workspace from {}", image_path.display());

    let output = Command::new("multipass")
        .args([
            "launch", &image_url, "--name", VM_NAME, "--cpus", VM_CPUS, "--memory", VM_MEMORY,
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

/// Provision the workspace by ensuring services are running.
fn provision_workspace() -> Result<()> {
    println!("  Starting services...");

    // Services auto-start via systemd polis.service on boot
    // Just ensure the service is started (idempotent)
    let output = Command::new("multipass")
        .args(["exec", VM_NAME, "--", "sudo", "systemctl", "start", "polis"])
        .output()
        .context("failed to start polis service")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("  Warning: polis service start failed: {stderr}");
    }

    Ok(())
}

/// Wait for the workspace container to become healthy.
#[allow(clippy::cognitive_complexity)] // NOSONAR: Polling loop with nested checks is inherently complex
fn wait_for_workspace_healthy() {
    println!("  Waiting for workspace to be ready...");

    let max_attempts = 30;
    let delay = Duration::from_secs(2);

    for attempt in 1..=max_attempts {
        let output = Command::new("multipass")
            .args([
                "exec",
                VM_NAME,
                "--",
                "docker",
                "compose",
                "ps",
                "--format",
                "json",
                "workspace",
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

/// Generate a workspace ID with 64 bits of entropy (V-010).
///
/// Uses `RandomState` (`SipHash` with random keys) seeded with a nanosecond
/// timestamp for 64 bits of entropy, producing a 16-character hex suffix.
fn generate_workspace_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    format!("polis-{:016x}", hasher.finish())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, unsafe_code)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── get_image_path ───────────────────────────────────────────────────────

    #[test]
    fn test_get_image_path_polis_image_env_existing_file_returns_ok() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("custom.qcow2");
        std::fs::write(&img, b"fake").unwrap();
        // SAFETY: single-threaded test
        unsafe { std::env::set_var("POLIS_IMAGE", img.to_str().unwrap()) };
        let result = get_image_path();
        unsafe { std::env::remove_var("POLIS_IMAGE") };
        assert_eq!(result.unwrap(), img);
    }

    #[test]
    fn test_get_image_path_polis_image_env_missing_file_returns_error() {
        // SAFETY: single-threaded test
        unsafe { std::env::set_var("POLIS_IMAGE", "/nonexistent/custom.qcow2") };
        let result = get_image_path();
        unsafe { std::env::remove_var("POLIS_IMAGE") };
        let err = result.unwrap_err().to_string();
        assert!(err.contains("POLIS_IMAGE points to non-existent file"), "got: {err}");
    }

    #[test]
    fn test_get_image_path_no_image_returns_error_with_hint() {
        // SAFETY: single-threaded test
        unsafe { std::env::remove_var("POLIS_IMAGE") };
        // We can't guarantee ~/.polis/images/polis-workspace.qcow2 doesn't exist
        // on the test machine, so only assert the error message when it fails.
        if get_image_path().is_err() {
            let err = get_image_path().unwrap_err().to_string();
            assert!(err.contains("polis init"), "got: {err}");
        }
    }

    // ── verify_image_at_launch ───────────────────────────────────────────────

    #[test]
    fn test_verify_image_at_launch_matching_checksum_returns_hash() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("polis-workspace.qcow2");
        std::fs::write(&img, b"hello").unwrap();
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let sidecar = dir.path().join("polis-workspace.qcow2.sha256");
        std::fs::write(&sidecar, format!("{expected}  polis-workspace.qcow2\n")).unwrap();
        // SAFETY: single-threaded test
        unsafe { std::env::remove_var("POLIS_IMAGE") };
        let hash = verify_image_at_launch(&img).unwrap();
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_verify_image_at_launch_mismatched_checksum_returns_error() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("polis-workspace.qcow2");
        std::fs::write(&img, b"hello").unwrap();
        let sidecar = dir.path().join("polis-workspace.qcow2.sha256");
        std::fs::write(&sidecar, format!("{}  polis-workspace.qcow2\n", "a".repeat(64))).unwrap();
        // SAFETY: single-threaded test
        unsafe { std::env::remove_var("POLIS_IMAGE") };
        let err = verify_image_at_launch(&img).unwrap_err().to_string();
        assert!(err.contains("Image integrity check failed"), "got: {err}");
    }

    #[test]
    fn test_verify_image_at_launch_missing_sidecar_no_polis_image_returns_error() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("polis-workspace.qcow2");
        std::fs::write(&img, b"hello").unwrap();
        // No sidecar file, no POLIS_IMAGE override.
        // SAFETY: single-threaded test
        unsafe { std::env::remove_var("POLIS_IMAGE") };
        let err = verify_image_at_launch(&img).unwrap_err().to_string();
        assert!(err.contains("Image checksum missing"), "got: {err}");
    }

    #[test]
    fn test_verify_image_at_launch_missing_sidecar_with_polis_image_warns_and_returns_hash() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("custom.qcow2");
        std::fs::write(&img, b"hello").unwrap();
        // No sidecar, but POLIS_IMAGE is set → warn and return hash.
        // SAFETY: single-threaded test
        unsafe { std::env::set_var("POLIS_IMAGE", img.to_str().unwrap()) };
        let result = verify_image_at_launch(&img);
        unsafe { std::env::remove_var("POLIS_IMAGE") };
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // ── generate_workspace_id ────────────────────────────────────────────────

    #[test]
    fn test_generate_workspace_id_starts_with_polis() {
        let id = generate_workspace_id();
        assert!(id.starts_with("polis-"), "id should start with 'polis-'");
    }

    #[test]
    fn test_generate_workspace_id_has_16_char_hex_suffix() {
        let id = generate_workspace_id();
        let suffix = id.strip_prefix("polis-").unwrap();
        assert_eq!(suffix.len(), 16, "suffix should be 16 hex chars");
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

    proptest! {
        /// generate_workspace_id always matches expected format
        #[test]
        fn prop_generate_workspace_id_format(_seed in 0u32..1000) {
            let id = generate_workspace_id();
            prop_assert!(id.starts_with("polis-"));
            let suffix = id.strip_prefix("polis-").expect("prefix exists");
            prop_assert_eq!(suffix.len(), 16);
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
