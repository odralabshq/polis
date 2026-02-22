//! `polis agent` — agent management subcommands.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde::Deserialize;

use crate::multipass::{Multipass, VM_NAME};
use crate::output::OutputContext;
use crate::state::StateManager;
use crate::workspace::{CONTAINER_NAME, vm};

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
    /// Open an interactive shell in the workspace container
    Shell,
    /// Run a command in the workspace container
    Exec(ExecArgs),
    /// Run an agent-specific command (defined in agents/<name>/commands.sh)
    Cmd(CmdArgs),
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

#[derive(Args)]
pub struct ExecArgs {
    /// Command and arguments to run in the workspace container
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

#[derive(Args)]
pub struct CmdArgs {
    /// Agent-specific subcommand (e.g. token, devices, onboard)
    #[arg(trailing_var_arg = true, required = true)]
    pub args: Vec<String>,
}

/// Minimal agent manifest shape — only the fields we need.
#[derive(Deserialize)]
struct AgentManifest {
    metadata: AgentMetadata,
}

#[derive(Deserialize)]
struct AgentMetadata {
    name: String,
}

/// Run the given agent subcommand.
///
/// # Errors
///
/// Returns an error if the subcommand fails (e.g. VM not running, agent not
/// found, artifact generation failure).
pub async fn run(
    cmd: AgentCommand,
    mp: &impl Multipass,
    ctx: &OutputContext,
    json: bool,
) -> Result<()> {
    match cmd {
        AgentCommand::Add(args) => add(&args, mp, ctx).await,
        AgentCommand::Remove(args) => remove(&args, mp, ctx).await,
        AgentCommand::List => list(mp, ctx, json).await,
        AgentCommand::Restart => restart(mp, ctx).await,
        AgentCommand::Update => update(mp, ctx).await,
        AgentCommand::Shell => shell(mp).await,
        AgentCommand::Exec(args) => exec_cmd(mp, &args).await,
        AgentCommand::Cmd(args) => agent_cmd(mp, &args).await,
    }
}

async fn add(args: &AddArgs, mp: &impl Multipass, ctx: &OutputContext) -> Result<()> {
    // Validate local path
    let folder = std::path::Path::new(&args.path);
    anyhow::ensure!(folder.exists(), "Path not found: {}", args.path);
    let manifest_path = folder.join("agent.yaml");
    anyhow::ensure!(
        manifest_path.exists(),
        "No agent.yaml found in: {}",
        args.path
    );

    // Parse agent name from manifest using serde_yaml (no yq dependency)
    let manifest_content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: AgentManifest = serde_yaml::from_str(&manifest_content)
        .context("failed to parse agent.yaml: missing or invalid metadata.name")?;
    let name = manifest.metadata.name;
    anyhow::ensure!(!name.is_empty(), "metadata.name is empty in agent.yaml");

    // VM must be running
    anyhow::ensure!(
        vm::state(mp).await? == vm::VmState::Running,
        "VM is not running. Start it first: polis start"
    );

    // Check agent doesn't already exist
    let target_dir = format!("{VM_ROOT}/agents/{name}");
    let exists = mp.exec(&["test", "-d", &target_dir]).await?;
    anyhow::ensure!(
        !exists.status.success(),
        "Agent '{name}' already installed. Remove it first: polis agent remove {name}"
    );

    // Transfer folder to VM using the Multipass trait
    if !ctx.quiet {
        println!("Copying agent '{name}' to VM...");
    }
    let dest = format!("{VM_ROOT}/agents/{name}");
    let out = mp
        .transfer_recursive(&args.path, &dest)
        .await
        .context("multipass transfer")?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to transfer agent folder: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Generate artifacts
    if !ctx.quiet {
        println!("Generating artifacts...");
    }
    let script = format!("{VM_ROOT}/scripts/generate-agent.sh");
    let all_agents = format!("{VM_ROOT}/agents");
    let gen_out = mp
        .exec(&["bash", &script, &name, &all_agents])
        .await
        .context("generate-agent.sh")?;
    if !gen_out.status.success() {
        // Cleanup on failure
        let _ = mp.exec(&["rm", "-rf", &target_dir]).await;
        let stderr = String::from_utf8_lossy(&gen_out.stderr);
        let stdout = String::from_utf8_lossy(&gen_out.stdout);
        let detail = if stderr.is_empty() { stdout } else { stderr };
        anyhow::bail!("Artifact generation failed:\n{detail}");
    }

    if !ctx.quiet {
        println!("Agent '{name}' installed. Start with: polis start --agent {name}");
    }
    Ok(())
}

async fn remove(args: &RemoveArgs, mp: &impl Multipass, ctx: &OutputContext) -> Result<()> {
    let name = &args.name;
    let agent_dir = format!("{VM_ROOT}/agents/{name}");

    // Must exist
    let exists = mp.exec(&["test", "-d", &agent_dir]).await?;
    anyhow::ensure!(exists.status.success(), "Agent '{name}' is not installed.");

    let state_mgr = StateManager::new()?;
    let active = state_mgr.load()?.and_then(|s| s.active_agent);
    let is_active = active.as_deref() == Some(name.as_str());

    if is_active {
        if !ctx.quiet {
            println!("Stopping active agent '{name}'...");
        }
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
        let down = mp
            .exec(&["docker", "compose", "-f", &base, "-f", &overlay, "down"])
            .await?;
        anyhow::ensure!(
            down.status.success(),
            "Failed to stop stack: {}",
            String::from_utf8_lossy(&down.stderr)
        );
    }

    let rm = mp.exec(&["rm", "-rf", &agent_dir]).await?;
    anyhow::ensure!(
        rm.status.success(),
        "Failed to remove agent directory: {}",
        String::from_utf8_lossy(&rm.stderr)
    );

    if is_active {
        if !ctx.quiet {
            println!("Restarting control plane...");
        }
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let up = mp
            .exec(&["docker", "compose", "-f", &base, "up", "-d"])
            .await?;
        anyhow::ensure!(
            up.status.success(),
            "Failed to restart control plane: {}",
            String::from_utf8_lossy(&up.stderr)
        );

        if let Ok(Some(mut state)) = state_mgr.load() {
            state.active_agent = None;
            let _ = state_mgr.save(&state);
        }
    }

    if !ctx.quiet {
        println!("Agent '{name}' removed.");
    }
    Ok(())
}

async fn list(mp: &impl Multipass, ctx: &OutputContext, json: bool) -> Result<()> {
    // Scan agents/*/agent.yaml inside VM (exclude _template), emit JSON per line
    let scan = mp
        .exec(&[
            "bash",
            "-c",
            &format!(
                "for f in {VM_ROOT}/agents/*/agent.yaml; do \
                   dir=$(dirname \"$f\"); \
                   name=$(basename \"$dir\"); \
                   [ \"$name\" = \"_template\" ] && continue; \
                   [ -f \"$f\" ] || continue; \
                   n=$(yq -o=json '.metadata.name' \"$f\" 2>/dev/null); \
                   v=$(yq -o=json '.metadata.version' \"$f\" 2>/dev/null); \
                   d=$(yq -o=json '.metadata.description' \"$f\" 2>/dev/null); \
                   printf '{{\"dir\":\"%s\",\"name\":%s,\"version\":%s,\"description\":%s}}\\n' \
                     \"$name\" \"${{n:-null}}\" \"${{v:-null}}\" \"${{d:-null}}\"; \
                 done"
            ),
        ])
        .await?;

    let output = String::from_utf8_lossy(&scan.stdout);
    let state_mgr = StateManager::new()?;
    let active = state_mgr.load()?.and_then(|s| s.active_agent);

    // Parse each JSON line; skip malformed entries
    let agents: Vec<serde_json::Value> = output
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| eprintln!("warning: skipping malformed agent entry: {e}"))
                .ok()?;
            let dir_name = v.get("dir")?.as_str()?.to_string();
            let is_active = active.as_deref() == Some(&dir_name);
            Some(serde_json::json!({
                "name": v.get("name").cloned().unwrap_or(serde_json::Value::Null),
                "version": v.get("version").cloned().unwrap_or(serde_json::Value::Null),
                "description": v.get("description").cloned().unwrap_or(serde_json::Value::Null),
                "active": is_active,
            }))
        })
        .collect();

    if json {
        println!("{}", serde_json::json!({ "agents": agents }));
        return Ok(());
    }

    if agents.is_empty() {
        if !ctx.quiet {
            println!("No agents installed. Install one: polis agent add --path <folder>");
        }
        return Ok(());
    }

    println!("Available agents:\n");
    for agent in &agents {
        let name = agent["name"].as_str().unwrap_or("(unknown)");
        let version = agent["version"].as_str().unwrap_or("");
        let desc = agent["description"].as_str().unwrap_or("");
        let marker = if agent["active"].as_bool().unwrap_or(false) {
            "  [active]"
        } else {
            ""
        };
        println!("  {name:<16} {version:<10} {desc}{marker}");
    }
    println!("\nStart an agent: polis start --agent <name>");
    Ok(())
}

async fn restart(mp: &impl Multipass, ctx: &OutputContext) -> Result<()> {
    let state_mgr = StateManager::new()?;
    let name = state_mgr
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("No active agent. Start one: polis start --agent <name>"))?;

    anyhow::ensure!(
        vm::state(mp).await? == vm::VmState::Running,
        "Workspace is not running."
    );

    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
    let out = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            &base,
            "-f",
            &overlay,
            "restart",
            "workspace",
        ])
        .await?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to restart workspace: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    if !ctx.quiet {
        println!("Agent '{name}' workspace restarted.");
    }
    Ok(())
}

async fn update(mp: &impl Multipass, ctx: &OutputContext) -> Result<()> {
    let state_mgr = StateManager::new()?;
    let name = state_mgr
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("No active agent. Start one: polis start --agent <name>"))?;

    if !ctx.quiet {
        println!("Regenerating artifacts for '{name}'...");
    }
    let script = format!("{VM_ROOT}/scripts/generate-agent.sh");
    let all_agents = format!("{VM_ROOT}/agents");
    let gen_out = mp
        .exec(&["bash", &script, &name, &all_agents])
        .await
        .context("generate-agent.sh")?;
    if !gen_out.status.success() {
        let stderr = String::from_utf8_lossy(&gen_out.stderr);
        let stdout = String::from_utf8_lossy(&gen_out.stdout);
        let detail = if stderr.is_empty() { stdout } else { stderr };
        anyhow::bail!("Artifact generation failed (workspace NOT recreated):\n{detail}");
    }

    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
    let out = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            &base,
            "-f",
            &overlay,
            "up",
            "-d",
            "--force-recreate",
            "workspace",
        ])
        .await?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to recreate workspace: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    if !ctx.quiet {
        println!("Agent '{name}' updated and workspace recreated.");
    }
    Ok(())
}

/// Require the VM to be running; return an error otherwise.
async fn require_running(mp: &impl Multipass) -> Result<()> {
    anyhow::ensure!(
        vm::state(mp).await? == vm::VmState::Running,
        "Workspace is not running. Start it first: polis start --agent <name>"
    );
    Ok(())
}

/// Return the active agent name from state, or error.
fn require_active_agent() -> Result<String> {
    StateManager::new()?
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("No active agent. Start one: polis start --agent <name>"))
}

async fn shell(mp: &impl Multipass) -> Result<()> {
    require_running(mp).await?;
    let name = require_active_agent()?;

    // Read runtime.user from agent manifest inside the VM
    let user_out = mp
        .exec(&[
            "bash",
            "-c",
            &format!("yq '.spec.runtime.user // \"root\"' {VM_ROOT}/agents/{name}/agent.yaml"),
        ])
        .await?;
    let user = String::from_utf8_lossy(&user_out.stdout).trim().to_string();
    let user = if user.is_empty() || !user_out.status.success() {
        "root"
    } else {
        &user
    };

    let status = std::process::Command::new("multipass")
        .args([
            "exec",
            VM_NAME,
            "--",
            "docker",
            "exec",
            "-it",
            "-u",
            user,
            CONTAINER_NAME,
            "bash",
        ])
        .status()
        .context("failed to spawn multipass")?;
    std::process::exit(status.code().unwrap_or(1));
}

async fn exec_cmd(mp: &impl Multipass, args: &ExecArgs) -> Result<()> {
    require_running(mp).await?;
    let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--", "docker", "exec", CONTAINER_NAME];
    let refs: Vec<&str> = args.command.iter().map(String::as_str).collect();
    cmd_args.extend(&refs);
    let status = std::process::Command::new("multipass")
        .args(&cmd_args)
        .status()
        .context("failed to spawn multipass")?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

async fn agent_cmd(mp: &impl Multipass, args: &CmdArgs) -> Result<()> {
    require_running(mp).await?;
    let name = require_active_agent()?;
    let commands_sh = format!("{VM_ROOT}/agents/{name}/commands.sh");

    // Verify commands.sh exists
    let check = mp.exec(&["test", "-f", &commands_sh]).await?;
    anyhow::ensure!(check.status.success(), "Agent '{name}' has no commands.sh");

    let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--", "bash", &commands_sh, CONTAINER_NAME];
    let refs: Vec<&str> = args.args.iter().map(String::as_str).collect();
    cmd_args.extend(&refs);
    let status = std::process::Command::new("multipass")
        .args(&cmd_args)
        .status()
        .context("failed to spawn multipass")?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_manifest_parses_name() {
        let yaml = "metadata:\n  name: my-agent\n";
        let m: AgentManifest = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(m.metadata.name, "my-agent");
    }

    #[test]
    fn test_agent_manifest_missing_name_returns_error() {
        let yaml = "metadata:\n  version: v1.0\n";
        assert!(serde_yaml::from_str::<AgentManifest>(yaml).is_err());
    }
}
