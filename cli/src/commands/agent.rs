//! `polis agent` — agent management subcommands.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::multipass::Multipass;
use crate::state::StateManager;
use crate::workspace::vm;

const VM_ROOT: &str = "/opt/polis";

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Install a new agent from a local folder
    Add(AddArgs),
    /// Remove an installed agent
    Remove(RemoveArgs),
    /// List available agents
    List,
    /// Restart the active agent's workspace
    Restart,
    /// Re-generate artifacts and recreate workspace
    Update,
}

#[derive(Args)]
pub struct AddArgs {
    /// Path to agent folder on host (must contain agent.yaml)
    #[arg(long)]
    pub path: String,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Agent name to remove
    pub name: String,
}

pub fn run(cmd: AgentCommand, mp: &impl Multipass, quiet: bool, json: bool) -> Result<()> {
    match cmd {
        AgentCommand::Add(args) => add(args, mp, quiet),
        AgentCommand::Remove(args) => remove(args, mp, quiet),
        AgentCommand::List => list(mp, quiet, json),
        AgentCommand::Restart => restart(mp, quiet),
        AgentCommand::Update => update(mp, quiet),
    }
}

fn add(args: AddArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    // Validate local path
    let folder = std::path::Path::new(&args.path);
    anyhow::ensure!(folder.exists(), "Path not found: {}", args.path);
    let manifest = folder.join("agent.yaml");
    anyhow::ensure!(
        manifest.exists(),
        "No agent.yaml found in: {}",
        args.path
    );

    // Read agent name from manifest via local yq
    let name_out = std::process::Command::new("yq")
        .args([".metadata.name", manifest.to_str().unwrap()])
        .output()
        .context("running yq to read agent name (yq must be installed locally)")?;
    anyhow::ensure!(
        name_out.status.success(),
        "Failed to read metadata.name from agent.yaml"
    );
    let name = String::from_utf8_lossy(&name_out.stdout).trim().to_string();
    anyhow::ensure!(!name.is_empty() && name != "null", "metadata.name is missing in agent.yaml");

    // VM must be running
    anyhow::ensure!(
        vm::state(mp)? == vm::VmState::Running,
        "VM is not running. Start it first: polis start"
    );

    // Check agent doesn't already exist
    let agent_dir = format!("{VM_ROOT}/agents/{name}");
    let exists = mp.exec(&["test", "-d", &agent_dir])?;
    anyhow::ensure!(
        !exists.status.success(),
        "Agent '{name}' already installed. Remove it first: polis agent remove {name}"
    );

    // Transfer folder to VM using multipass transfer --recursive
    if !quiet {
        println!("Copying agent '{name}' to VM...");
    }
    let dest = format!("polis:{VM_ROOT}/agents/{name}");
    let out = std::process::Command::new("multipass")
        .args(["transfer", "--recursive", &args.path, &dest])
        .output()
        .context("multipass transfer")?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to transfer agent folder: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Generate artifacts
    if !quiet {
        println!("Generating artifacts...");
    }
    let script = format!("{VM_ROOT}/scripts/generate-agent.sh");
    let agents_dir = format!("{VM_ROOT}/agents");
    let gen_out = mp
        .exec(&["bash", &script, &name, &agents_dir])
        .context("generate-agent.sh")?;
    if !gen_out.status.success() {
        // Cleanup on failure
        let _ = mp.exec(&["rm", "-rf", &agent_dir]);
        let stderr = String::from_utf8_lossy(&gen_out.stderr);
        let stdout = String::from_utf8_lossy(&gen_out.stdout);
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        anyhow::bail!("Artifact generation failed:\n{detail}");
    }

    if !quiet {
        println!("Agent '{name}' installed. Start with: polis start --agent {name}");
    }
    Ok(())
}

fn remove(args: RemoveArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    let name = &args.name;
    let agent_dir = format!("{VM_ROOT}/agents/{name}");

    // Must exist
    let exists = mp.exec(&["test", "-d", &agent_dir])?;
    anyhow::ensure!(
        exists.status.success(),
        "Agent '{name}' is not installed."
    );

    let state_mgr = StateManager::new()?;
    let active = state_mgr.load()?.and_then(|s| s.active_agent);
    let is_active = active.as_deref() == Some(name.as_str());

    if is_active {
        if !quiet {
            println!("Stopping active agent '{name}'...");
        }
        // Compose down full stack
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
        let down = mp.exec(&["docker", "compose", "-f", &base, "-f", &overlay, "down"])?;
        anyhow::ensure!(
            down.status.success(),
            "Failed to stop stack: {}",
            String::from_utf8_lossy(&down.stderr)
        );
    }

    // Delete agent directory
    let rm = mp.exec(&["rm", "-rf", &agent_dir])?;
    anyhow::ensure!(
        rm.status.success(),
        "Failed to remove agent directory: {}",
        String::from_utf8_lossy(&rm.stderr)
    );

    if is_active {
        // Restart control plane only
        if !quiet {
            println!("Restarting control plane...");
        }
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let up = mp.exec(&["docker", "compose", "-f", &base, "up", "-d"])?;
        anyhow::ensure!(
            up.status.success(),
            "Failed to restart control plane: {}",
            String::from_utf8_lossy(&up.stderr)
        );

        // Clear active_agent in state
        if let Ok(Some(mut state)) = state_mgr.load() {
            state.active_agent = None;
            let _ = state_mgr.save(&state);
        }
    }

    if !quiet {
        println!("Agent '{name}' removed.");
    }
    Ok(())
}

fn list(mp: &impl Multipass, quiet: bool, json: bool) -> Result<()> {
    // Scan agents/*/agent.yaml inside VM (exclude _template)
    let scan = mp.exec(&[
        "bash",
        "-c",
        &format!(
            "for f in {VM_ROOT}/agents/*/agent.yaml; do \
               dir=$(dirname \"$f\"); \
               name=$(basename \"$dir\"); \
               [ \"$name\" = \"_template\" ] && continue; \
               [ -f \"$f\" ] || continue; \
               n=$(yq '.metadata.name' \"$f\" 2>/dev/null); \
               v=$(yq '.metadata.version' \"$f\" 2>/dev/null); \
               d=$(yq '.metadata.description' \"$f\" 2>/dev/null); \
               echo \"$name|${{n:-null}}|${{v:-null}}|${{d:-null}}\"; \
             done"
        ),
    ])?;

    let output = String::from_utf8_lossy(&scan.stdout);
    let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();

    let state_mgr = StateManager::new()?;
    let active = state_mgr.load()?.and_then(|s| s.active_agent);

    if json {
        let agents: Vec<serde_json::Value> = lines
            .iter()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(4, '|').collect();
                if parts.len() < 4 {
                    return None;
                }
                let name = parts[0];
                Some(serde_json::json!({
                    "name": name,
                    "version": null_or_str(parts[1]),
                    "description": null_or_str(parts[2]),
                    "active": active.as_deref() == Some(name),
                }))
            })
            .collect();
        println!("{}", serde_json::json!({ "agents": agents }));
        return Ok(());
    }

    if lines.is_empty() {
        if !quiet {
            println!("No agents installed. Install one: polis agent add --path <folder>");
        }
        return Ok(());
    }

    println!("Available agents:\n");
    for line in &lines {
        let parts: Vec<&str> = line.splitn(4, '|').collect();
        if parts.len() < 4 {
            eprintln!("warning: skipping malformed entry: {line}");
            continue;
        }
        let name = parts[0];
        let version = null_or_str(parts[1]).unwrap_or_default();
        let desc = null_or_str(parts[2]).unwrap_or_default();
        let marker = if active.as_deref() == Some(name) {
            "  [active]"
        } else {
            ""
        };
        println!("  {name:<16} {version:<10} {desc}{marker}");
    }
    println!("\nStart an agent: polis start --agent <name>");
    Ok(())
}

fn restart(mp: &impl Multipass, quiet: bool) -> Result<()> {
    let state_mgr = StateManager::new()?;
    let name = state_mgr
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("No active agent. Start one: polis start --agent <name>"))?;

    anyhow::ensure!(
        vm::state(mp)? == vm::VmState::Running,
        "Workspace is not running."
    );

    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
    let out = mp.exec(&[
        "docker", "compose", "-f", &base, "-f", &overlay, "restart", "workspace",
    ])?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to restart workspace: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    if !quiet {
        println!("Agent '{name}' workspace restarted.");
    }
    Ok(())
}

fn update(mp: &impl Multipass, quiet: bool) -> Result<()> {
    let state_mgr = StateManager::new()?;
    let name = state_mgr
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("No active agent. Start one: polis start --agent <name>"))?;

    // Re-generate artifacts first
    if !quiet {
        println!("Regenerating artifacts for '{name}'...");
    }
    let script = format!("{VM_ROOT}/scripts/generate-agent.sh");
    let agents_dir = format!("{VM_ROOT}/agents");
    let gen_out = mp
        .exec(&["bash", &script, &name, &agents_dir])
        .context("generate-agent.sh")?;
    if !gen_out.status.success() {
        let stderr = String::from_utf8_lossy(&gen_out.stderr);
        let stdout = String::from_utf8_lossy(&gen_out.stdout);
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        anyhow::bail!("Artifact generation failed (workspace NOT recreated):\n{detail}");
    }

    // Recreate workspace
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
    let out = mp.exec(&[
        "docker", "compose", "-f", &base, "-f", &overlay, "up", "-d", "--force-recreate",
        "workspace",
    ])?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to recreate workspace: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    if !quiet {
        println!("Agent '{name}' updated and workspace recreated.");
    }
    Ok(())
}

fn null_or_str(s: &str) -> Option<String> {
    if s == "null" || s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
