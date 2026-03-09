//! `polis agent activate` — activate an agent on the running workspace.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::App;
use crate::application::services::agent::{
    self, ActivateOutcome, AgentActivateOptions, AgentSwapOptions,
};
use crate::application::vm::lifecycle as vm;

/// Run the `agent activate` subcommand.
///
/// # Errors
///
/// Returns an error if agent activation or swap fails.
pub async fn run(app: &impl App, name: &str, envs: Vec<String>) -> Result<ExitCode> {
    let reporter = app.terminal_reporter();
    let opts = AgentActivateOptions {
        reporter: &reporter,
        agent_name: name,
        envs: envs.clone(),
    };
    let outcome =
        agent::activate_agent(app.provisioner(), app.state_store(), app.fs(), opts).await?;

    if let ActivateOutcome::SwapRequired { active, requested } = outcome {
        let prompt = format!("Agent '{active}' is active. Swap to '{requested}'?");
        if !app.confirm(&prompt, true)? {
            app.output().info("Swap cancelled.");
            return Ok(ExitCode::SUCCESS);
        }
        let swap_opts = AgentSwapOptions {
            reporter: &reporter,
            active_name: &active,
            new_name: &requested,
            envs,
        };
        let swap_outcome =
            agent::swap_agent(app.provisioner(), app.state_store(), app.fs(), swap_opts).await?;
        app.renderer().render_activate_outcome(&swap_outcome);
        show_dashboard_url(app).await;
    } else {
        app.renderer().render_activate_outcome(&outcome);
        show_dashboard_url(app).await;
    }
    Ok(ExitCode::SUCCESS)
}

/// Show the agent dashboard URL (best-effort, no error on failure).
async fn show_dashboard_url(app: &impl App) {
    if let Ok(ip) = vm::resolve_vm_ip(app.provisioner()).await {
        app.output().blank();
        app.output()
            .kv("Control UI", &format!("http://{ip}:18789/overview"));
    }
}
