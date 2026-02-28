//! Application context — unified state passed to every command handler.
//!
//! `AppContext` replaces the per-command pattern of constructing loose
//! `OutputContext`, `MultipassProvisioner`, and `StateManager` instances.
//! Adding a new cross-cutting concern (e.g. `--verbose`, telemetry) requires
//! only one field change here — zero command signatures change.

#![allow(dead_code)] // Refactor in progress — some fields/methods not yet adopted by all commands

use anyhow::Result;

use crate::command_runner::TokioCommandRunner;
use crate::output::{HumanRenderer, JsonRenderer, OutputContext, Renderer};
use crate::provisioner::MultipassProvisioner;
use crate::state::StateManager;

/// Output rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable terminal output (default).
    Human,
    /// Machine-readable JSON output.
    Json,
}

/// Output rendering flags.
pub struct OutputFlags {
    /// Disable ANSI color output.
    pub no_color: bool,
    /// Suppress non-error output.
    pub quiet: bool,
    /// Enable JSON output mode.
    pub json: bool,
}

/// Behaviour flags.
pub struct BehaviourFlags {
    /// Skip interactive prompts (also set by `CI` / `POLIS_YES` env vars).
    pub yes: bool,
}

/// Flags passed from the top-level CLI to `AppContext::new`.
pub struct AppFlags {
    /// Output rendering options.
    pub output: OutputFlags,
    /// Behaviour options.
    pub behaviour: BehaviourFlags,
}

/// Unified application context passed to every command handler.
///
/// Constructed once in `Cli::run()` and passed as `&AppContext` to all
/// command handlers, replacing the previous pattern of loose parameters.
pub struct AppContext {
    /// Terminal output context (colors, quiet mode).
    pub output: OutputContext,
    /// Output rendering mode (human vs JSON).
    pub mode: OutputMode,
    /// Multipass VM provisioner.
    pub provisioner: MultipassProvisioner<TokioCommandRunner>,
    /// Workspace state manager.
    pub state_mgr: StateManager,
    /// When `true`, skip interactive prompts and use defaults.
    ///
    /// Set when `--yes` / `-y` is passed, or when the `CI` or `POLIS_YES`
    /// environment variables are present.
    pub non_interactive: bool,
}

impl AppContext {
    /// Construct an `AppContext` from top-level CLI flags.
    ///
    /// # Errors
    ///
    /// Returns an error if `StateManager::new()` fails (home directory not found).
    pub fn new(flags: &AppFlags) -> Result<Self> {
        let ci_env = std::env::var("CI").is_ok() || std::env::var("POLIS_YES").is_ok();
        let non_interactive = flags.behaviour.yes || ci_env;

        let mode = if flags.output.json {
            OutputMode::Json
        } else {
            OutputMode::Human
        };

        Ok(Self {
            output: OutputContext::new(flags.output.no_color, flags.output.quiet),
            mode,
            provisioner: MultipassProvisioner::default_runner(),
            state_mgr: StateManager::new()?,
            non_interactive,
        })
    }

    /// Returns `true` when JSON output mode is active.
    #[must_use]
    pub fn is_json(&self) -> bool {
        self.mode == OutputMode::Json
    }

    /// Returns the appropriate `Renderer` variant for the current output mode.
    #[must_use]
    pub fn renderer(&self) -> Renderer<'_> {
        match self.mode {
            OutputMode::Human => Renderer::Human(HumanRenderer::new(&self.output)),
            OutputMode::Json => Renderer::Json(JsonRenderer),
        }
    }

    /// Ask the user for confirmation.
    ///
    /// When `non_interactive` is `true` (CI, `--yes` flag, or `POLIS_YES` env),
    /// returns `default` immediately without prompting.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal prompt fails (e.g. no TTY available).
    pub fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        if self.non_interactive {
            return Ok(default);
        }
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(prompt)
            .default(default)
            .interact()?;
        Ok(confirmed)
    }
}
