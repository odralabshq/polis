//! `polis start` — start workspace (download and create if needed).

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;

use crate::multipass::Multipass;
use crate::state::{StateManager, WorkspaceState};
use crate::workspace::{health, image, vm};

/// Path to the polis project root inside the VM.
const VM_POLIS_ROOT: &str = "/opt/polis";

/// Arguments for the start command.
#[derive(Args)]
pub struct StartArgs {
    /// Use custom image instead of cached/downloaded
    #[arg(long)]
    pub image: Option<String>,

    /// Agent to activate (must match agents/<name>/ directory inside the VM)
    #[arg(long)]
    pub agent: Option<String>,
}

/// Run `polis start`.
///
/// # Errors
///
/// Returns an error if image acquisition, VM creation, or health check fails.
pub async fn run(args: &StartArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    let state_mgr = StateManager::new()?;
    let vm_state = vm::state(mp).await?;

    match vm_state {
        vm::VmState::Running => return handle_running_vm(&state_mgr, args, quiet),
        vm::VmState::NotFound => create_and_start_vm(&state_mgr, args, mp, quiet).await?,
        _ => restart_vm(&state_mgr, args, mp, quiet).await?,
    }

    health::wait_ready(mp, quiet).await?;
    print_success_message(args.agent.as_deref(), quiet);
    Ok(())
}

/// Handle the case where the VM is already running.
fn handle_running_vm(state_mgr: &StateManager, args: &StartArgs, quiet: bool) -> Result<()> {
    let current_agent = state_mgr.load()?.and_then(|s| s.active_agent);
    if current_agent == args.agent {
        print_already_running_message(args.agent.as_deref(), quiet);
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
async fn create_and_start_vm(
    state_mgr: &StateManager,
    args: &StartArgs,
    mp: &impl Multipass,
    quiet: bool,
) -> Result<()> {
    let source = resolve_image_source(args.image.as_deref(), state_mgr)?;
    let image_path = image::ensure_available(source, quiet)?;
    vm::create(mp, &image_path, quiet).await?;

    setup_agent_if_requested(mp, args.agent.as_deref()).await?;
    start_compose(mp, args.agent.as_deref()).await?;

    let sha256 = image::load_metadata(&image::images_dir()?)
        .ok()
        .flatten()
        .map(|m| m.sha256);
    let state = WorkspaceState {
        workspace_id: generate_workspace_id(),
        created_at: Utc::now(),
        image_sha256: sha256,
        image_source: args.image.clone(),
        active_agent: args.agent.clone(),
    };
    state_mgr.save(&state)
}

/// Restart a stopped VM.
async fn restart_vm(
    state_mgr: &StateManager,
    args: &StartArgs,
    mp: &impl Multipass,
    quiet: bool,
) -> Result<()> {
    vm::restart(mp, quiet).await?;

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
    state_mgr.save(&state)
}

/// Resolve image source from CLI arg or persisted state.
fn resolve_image_source(
    cli_image: Option<&str>,
    state_mgr: &StateManager,
) -> Result<image::ImageSource> {
    if let Some(s) = cli_image {
        return parse_image_source(s);
    }
    let persisted = state_mgr
        .load()?
        .and_then(|s| s.image_source)
        .map(|s| parse_image_source(&s))
        .transpose()?
        .unwrap_or(image::ImageSource::Default);
    Ok(persisted)
}

/// Parse a string into an `ImageSource`.
fn parse_image_source(s: &str) -> Result<image::ImageSource> {
    if s.starts_with("http://") || s.starts_with("https://") {
        return Ok(image::ImageSource::HttpUrl(s.to_string()));
    }
    let path = PathBuf::from(s);
    anyhow::ensure!(path.exists(), "Image file not found: {}", path.display());
    Ok(image::ImageSource::LocalFile(path))
}

/// Validate and generate artifacts for an agent if one is requested.
async fn setup_agent_if_requested(mp: &impl Multipass, agent: Option<&str>) -> Result<()> {
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
fn print_already_running_message(agent: Option<&str>, quiet: bool) {
    if quiet {
        return;
    }
    println!();
    println!("Workspace is running.");
    if let Some(name) = agent {
        println!("Agent: {name}");
    }
    println!();
    print_guarantees();
    println!();
    println!("Connect: polis connect");
    println!("Status:  polis status");
}

/// Print success message after workspace is ready.
fn print_success_message(agent: Option<&str>, quiet: bool) {
    if quiet {
        return;
    }
    println!();
    print_guarantees();
    println!();
    if let Some(name) = agent {
        println!("Workspace ready. Agent: {name}");
        println!();
        println!("Agent shell:    polis agent shell");
        println!("Agent commands: polis agent cmd help");
    } else {
        println!("Workspace ready.");
    }
    println!();
    println!("Connect: polis connect");
    println!("Status:  polis status");
}

/// Validate that the agent directory and manifest exist inside the VM.
///
/// # Errors
///
/// Returns an error if the agent manifest is missing or the VM is unreachable.
pub async fn validate_agent(mp: &impl Multipass, agent_name: &str) -> Result<()> {
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
        anyhow::bail!("Unknown agent '{agent_name}'. {hint}");
    }
    Ok(())
}

/// Call scripts/generate-agent.sh inside the VM.
///
/// # Errors
///
/// Returns an error if artifact generation fails or the VM is unreachable.
pub async fn generate_agent_artifacts(mp: &impl Multipass, agent_name: &str) -> Result<()> {
    let script = format!("{VM_POLIS_ROOT}/scripts/generate-agent.sh");
    let agents_dir = format!("{VM_POLIS_ROOT}/agents");
    let output = mp
        .exec(&["bash", &script, agent_name, &agents_dir])
        .await
        .context("running generate-agent.sh")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.is_empty() {
            stdout.to_string()
        } else {
            stderr.to_string()
        };
        // Exit code 2 = missing yq
        if output.status.code() == Some(2) {
            anyhow::bail!(
                "Error: yq v4+ is required inside the VM.\nInstall: sudo apt install yq\n\n{detail}"
            );
        }
        anyhow::bail!("Error: Agent artifact generation failed for '{agent_name}'.\n{detail}");
    }
    Ok(())
}

/// Start docker compose inside the VM, optionally with an agent overlay.
///
/// # Errors
///
/// Returns an error if docker compose fails or the VM is unreachable.
pub async fn start_compose(mp: &impl Multipass, agent_name: Option<&str>) -> Result<()> {
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
        anyhow::bail!("Error: Failed to start platform.\n{stderr}");
    }
    Ok(())
}

fn print_guarantees() {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
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
