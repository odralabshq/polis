//! Application context — unified state passed to every command handler.
//!
//! `AppContext` replaces the per-command pattern of constructing loose
//! `OutputContext`, `MultipassProvisioner`, and `StateManager` instances.
//! Adding a new cross-cutting concern (e.g. `--verbose`, telemetry) requires
//! only one field change here — zero command signatures change.

use anyhow::Result;

use crate::infra::assets::EmbeddedAssets;
use crate::infra::command_runner::{DEFAULT_CMD_TIMEOUT, TokioCommandRunner};
use crate::infra::config::YamlConfigStore;
use crate::infra::fs::LocalFs;
use crate::infra::network::TokioNetworkProbe;
use crate::infra::provisioner::MultipassProvisioner;
use crate::infra::ssh::SshConfigManager;
use crate::infra::state::StateManager;
use crate::output::{HumanRenderer, JsonRenderer, OutputContext, Renderer};

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
    /// Embedded assets extractor.
    pub assets: EmbeddedAssets,
    /// SSH configuration manager.
    pub ssh: SshConfigManager,
    /// When `true`, skip interactive prompts and use defaults.
    ///
    /// Set when `--yes` / `-y` is passed, or when the `CI` or `POLIS_YES`
    /// environment variables are present.
    pub non_interactive: bool,

    /// Command runner for local process execution.
    pub cmd_runner: TokioCommandRunner,
    /// Network probe for connectivity checks.
    pub network_probe: TokioNetworkProbe,
    /// Local filesystem operations.
    pub local_fs: LocalFs,
    /// Configuration store.
    pub config_store: YamlConfigStore,
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
            assets: EmbeddedAssets,
            ssh: SshConfigManager::new()?,
            non_interactive,
            cmd_runner: TokioCommandRunner::new(DEFAULT_CMD_TIMEOUT),
            network_probe: TokioNetworkProbe,
            local_fs: LocalFs,
            config_store: YamlConfigStore,
        })
    }

    /// Returns `true` when JSON output mode is active.
    #[must_use]
    #[allow(dead_code)] // Used in tests and future command handlers
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

    /// Returns a `TerminalReporter` bound to this context's output.
    #[must_use]
    #[allow(dead_code)] // Not yet called from command handlers
    pub fn terminal_reporter(&self) -> crate::output::reporter::TerminalReporter<'_> {
        crate::output::reporter::TerminalReporter::new(&self.output)
    }

    /// Extract bundled assets to a temp directory and return the path.
    ///
    /// The returned `TempDir` guard must be kept alive until all operations
    /// using the assets are complete.
    ///
    /// # Errors
    ///
    /// Returns an error if asset extraction fails.
    #[allow(dead_code)] // Not yet called from command handlers
    pub fn assets_dir(&self) -> Result<(std::path::PathBuf, tempfile::TempDir)> {
        let (path, guard) = crate::infra::assets::extract_assets()?;
        Ok((path, guard))
    }
}
