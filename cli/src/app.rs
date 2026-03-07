//! Application context — unified state passed to every command handler.
//!
//! `AppContext` replaces the per-command pattern of constructing loose
//! `OutputContext`, `MultipassProvisioner`, and `StateManager` instances.
//! Adding a new cross-cutting concern (e.g. `--verbose`, telemetry) requires
//! only one field change here — zero command signatures change.

use anyhow::Result;

use crate::application::ports::{
    AssetExtractor, CommandRunner, ConfigStore, ContainerExecutor, FileHasher, FileTransfer,
    InstanceInspector, InstanceLifecycle, LocalFs, LocalPaths, NetworkProbe, ShellExecutor,
    SshConfigurator, WorkspaceStateStore,
};
use crate::infra::assets::EmbeddedAssets;
use crate::infra::command_runner::{DEFAULT_CMD_TIMEOUT, TokioCommandRunner};
use crate::infra::config::YamlConfigStore;
use crate::infra::fs::OsFs;
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
    pub local_fs: OsFs,
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
            local_fs: OsFs,
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
    pub fn assets_dir(&self) -> Result<(std::path::PathBuf, tempfile::TempDir)> {
        let (path, guard) = crate::infra::assets::extract_assets()?;
        Ok((path, guard))
    }

    /// Returns a reference to the VM provisioner (opaque type).
    ///
    /// This accessor provides source-level decoupling — the commands layer
    /// cannot name or depend on the concrete type.
    #[must_use]
    pub fn provisioner(
        &self,
    ) -> &(impl ShellExecutor + FileTransfer + InstanceInspector + InstanceLifecycle + ContainerExecutor)
    {
        &self.provisioner
    }

    /// Returns a reference to the workspace state store.
    #[must_use]
    pub fn state_store(&self) -> &impl WorkspaceStateStore {
        &self.state_mgr
    }

    /// Returns a reference to the local filesystem.
    #[must_use]
    pub fn local_fs(&self) -> &impl LocalFs {
        &self.local_fs
    }
}

// ── App trait ─────────────────────────────────────────────────────────────────

/// Trait abstracting the application context for dependency injection.
///
/// Implement this trait to provide mock dependencies in tests.
/// `AppContext` is the production implementation; `test_utils::MockAppContext`
/// is the test implementation.
pub trait App {
    /// VM provisioner type.
    type Provisioner: ShellExecutor
        + FileTransfer
        + InstanceInspector
        + InstanceLifecycle
        + ContainerExecutor;
    /// Workspace state store type.
    type StateStore: WorkspaceStateStore;
    /// Local filesystem type (implements `LocalFs` + `LocalPaths` + `FileHasher`).
    type Fs: LocalFs + LocalPaths + FileHasher;
    /// Configuration store type.
    type Config: ConfigStore;
    /// Command runner type.
    type CmdRunner: CommandRunner;
    /// Network probe type.
    type Network: NetworkProbe;
    /// SSH configurator type.
    type Ssh: SshConfigurator;
    /// Asset extractor type.
    type Assets: AssetExtractor;

    /// Returns a reference to the VM provisioner.
    fn provisioner(&self) -> &Self::Provisioner;
    /// Returns a reference to the workspace state store.
    fn state_store(&self) -> &Self::StateStore;
    /// Returns a reference to the local filesystem.
    fn fs(&self) -> &Self::Fs;
    /// Returns a reference to the configuration store.
    fn config(&self) -> &Self::Config;
    /// Returns a reference to the command runner.
    fn cmd_runner(&self) -> &Self::CmdRunner;
    /// Returns a reference to the network probe.
    fn network(&self) -> &Self::Network;
    /// Returns a reference to the SSH configurator.
    fn ssh(&self) -> &Self::Ssh;
    /// Returns a reference to the asset extractor.
    fn assets(&self) -> &Self::Assets;
    /// Ask the user for confirmation.
    ///
    /// # Errors
    /// Returns an error if the terminal prompt fails.
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;
    /// Returns the appropriate renderer for the current output mode.
    fn renderer(&self) -> crate::output::Renderer<'_>;
    /// Returns a terminal reporter bound to this context's output.
    fn terminal_reporter(&self) -> crate::output::reporter::TerminalReporter<'_>;
    /// Returns a reference to the output context.
    fn output(&self) -> &OutputContext;
    /// Returns `true` when interactive prompts should be skipped.
    fn non_interactive(&self) -> bool;
    /// Extract bundled assets to a temp directory.
    ///
    /// # Errors
    /// Returns an error if asset extraction fails.
    fn assets_dir(&self) -> Result<(std::path::PathBuf, tempfile::TempDir)>;
}

impl App for AppContext {
    type Provisioner = MultipassProvisioner<TokioCommandRunner>;
    type StateStore = StateManager;
    type Fs = OsFs;
    type Config = YamlConfigStore;
    type CmdRunner = TokioCommandRunner;
    type Network = TokioNetworkProbe;
    type Ssh = SshConfigManager;
    type Assets = EmbeddedAssets;

    fn provisioner(&self) -> &Self::Provisioner {
        &self.provisioner
    }
    fn state_store(&self) -> &Self::StateStore {
        &self.state_mgr
    }
    fn fs(&self) -> &Self::Fs {
        &self.local_fs
    }
    fn config(&self) -> &Self::Config {
        &self.config_store
    }
    fn cmd_runner(&self) -> &Self::CmdRunner {
        &self.cmd_runner
    }
    fn network(&self) -> &Self::Network {
        &self.network_probe
    }
    fn ssh(&self) -> &Self::Ssh {
        &self.ssh
    }
    fn assets(&self) -> &Self::Assets {
        &self.assets
    }
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        self.confirm(prompt, default)
    }
    fn renderer(&self) -> crate::output::Renderer<'_> {
        self.renderer()
    }
    fn terminal_reporter(&self) -> crate::output::reporter::TerminalReporter<'_> {
        self.terminal_reporter()
    }
    fn output(&self) -> &OutputContext {
        &self.output
    }
    fn non_interactive(&self) -> bool {
        self.non_interactive
    }
    fn assets_dir(&self) -> Result<(std::path::PathBuf, tempfile::TempDir)> {
        self.assets_dir()
    }
}
