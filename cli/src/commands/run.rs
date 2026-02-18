//! Run command — state machine for checkpoint/resume and agent switching.

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;
use polis_common::types::{RunStage, RunState};

use crate::state::StateManager;

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
/// # Errors
///
/// Returns an error if no agents are installed or the requested agent is not found.
fn resolve_agent(requested: Option<&str>) -> Result<String> {
    if let Some(agent) = requested {
        let available = list_available_agents()?;
        if !available.is_empty() && !available.iter().any(|a| a == agent) {
            anyhow::bail!(
                "Agent '{}' not found. Available: {}",
                agent,
                available.join(", ")
            );
        }
        return Ok(agent.to_string());
    }

    let agents = list_available_agents()?;
    match agents.len() {
        0 => anyhow::bail!("No agents installed. Run: polis agents add <path>"),
        1 => Ok(agents.into_iter().next().unwrap_or_default()),
        _ => prompt_agent_selection(&agents),
    }
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
        execute_stage(&mut run_state, next_stage);
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
    execute_stage(&mut new_state, RunStage::AgentReady);
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
        execute_stage(&mut run_state, next_stage);
        state_mgr.advance(&mut run_state, next_stage)?;
    }

    println!("{agent} is ready");
    Ok(())
}

/// Execute a single pipeline stage (stub — real provisioning is out of scope for issue 07).
fn execute_stage(run_state: &mut RunState, next_stage: RunStage) {
    println!("{}...", next_stage.description());
    if next_stage == RunStage::ImageReady {
        run_state.image_sha256 = Some(String::from("stub"));
    }
}

fn generate_workspace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("polis-{ts:08x}")
}
