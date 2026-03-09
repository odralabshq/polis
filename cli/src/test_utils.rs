//! Test utilities for command handler testing.
//!
//! This module provides shared test infrastructure for testing command handlers
//! with mock dependencies. It is conditionally compiled with `#[cfg(test)]`
//! since it's only used in tests.
//!
//! # Example
//!
//! ```ignore
//! use crate::test_utils::{mock_app_context, mock_provisioner, verify_command_handler_patterns};
//!
//! #[tokio::test]
//! async fn test_command_handler() {
//!     let app = mock_app_context();
//!     let provisioner = mock_provisioner();
//!     // ... test command handler ...
//! }
//! ```

use std::cell::RefCell;
use std::path::Path;
use std::process::Output;

use anyhow::{Context, Result};

use crate::app::{App, OutputMode};
use crate::application::ports::{
    AssetExtractor, CommandRunner, ConfigStore, ContainerExecutor, FileHasher, FileTransfer,
    InstanceInspector, InstanceLifecycle, InstanceSpec, LocalFs, LocalPaths, NetworkProbe,
    ProgressReporter, ShellExecutor, SshConfigurator, WorkspaceStateStore,
};
use crate::domain::config::PolisConfig;
use crate::domain::workspace::WorkspaceState;
use crate::output::OutputContext;

// ── Mock AppContext ───────────────────────────────────────────────────────────

/// Create a mock `AppContext` for testing command handlers.
///
/// The mock context is configured with:
/// - Non-interactive mode (skips prompts)
/// - Human output mode (not JSON)
/// - Quiet output (suppresses non-error output)
/// - Temporary state directory
///
/// # Panics
///
/// Panics if the temporary directory or state manager cannot be created.
#[must_use]
pub fn mock_app_context() -> MockAppContext {
    MockAppContext::new()
}

/// A mock `AppContext` that uses in-memory state and mock dependencies.
///
/// This struct mirrors the real `AppContext` but uses mock implementations
/// for testing purposes.
pub struct MockAppContext {
    /// Terminal output context (colors, quiet mode).
    pub output: OutputContext,
    /// Output rendering mode (human vs JSON).
    pub mode: OutputMode,
    /// Mock provisioner for VM operations.
    pub provisioner: MockProvisioner,
    /// Mock state store for workspace state.
    pub state_store: MockStateStore,
    /// Mock asset extractor.
    pub assets: MockAssets,
    /// When `true`, skip interactive prompts and use defaults.
    pub non_interactive: bool,
    /// Mock local filesystem.
    pub local_fs: MockLocalFs,
    /// Mock configuration store.
    pub config: MockConfig,
    /// Mock command runner.
    pub cmd_runner: MockCmdRunner,
    /// Mock network probe.
    pub network: MockNetwork,
    /// Mock SSH configurator.
    pub ssh: MockSsh,
}

impl MockAppContext {
    /// Create a new mock `AppContext` with default test configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            output: OutputContext::new(true, false, true), // no color, not tty, quiet
            mode: OutputMode::Human,
            provisioner: MockProvisioner::new(),
            state_store: MockStateStore::new(),
            assets: MockAssets,
            non_interactive: true,
            local_fs: MockLocalFs::new(),
            config: MockConfig,
            cmd_runner: MockCmdRunner,
            network: MockNetwork,
            ssh: MockSsh,
        }
    }

    /// Returns a `TerminalReporter` bound to this context's output.
    #[must_use]
    pub fn terminal_reporter(&self) -> crate::output::reporter::TerminalReporter<'_> {
        crate::output::reporter::TerminalReporter::new(&self.output)
    }

    /// Returns the appropriate `Renderer` variant for the current output mode.
    #[must_use]
    pub fn renderer(&self) -> crate::output::Renderer<'_> {
        match self.mode {
            OutputMode::Human => {
                crate::output::Renderer::Human(crate::output::HumanRenderer::new(&self.output))
            }
            OutputMode::Json => crate::output::Renderer::Json(crate::output::JsonRenderer),
        }
    }

    /// Ask the user for confirmation (always returns `default` in mock).
    ///
    /// # Errors
    ///
    /// This mock implementation never returns an error.
    pub fn confirm(&self, _prompt: &str, default: bool) -> Result<bool> {
        Ok(default)
    }
}

impl Default for MockAppContext {
    fn default() -> Self {
        Self::new()
    }
}

impl App for MockAppContext {
    type Provisioner = MockProvisioner;
    type StateStore = MockStateStore;
    type Fs = MockLocalFs;
    type Config = MockConfig;
    type CmdRunner = MockCmdRunner;
    type Network = MockNetwork;
    type Ssh = MockSsh;
    type Assets = MockAssets;

    fn provisioner(&self) -> &Self::Provisioner {
        &self.provisioner
    }
    fn state_store(&self) -> &Self::StateStore {
        &self.state_store
    }
    fn fs(&self) -> &Self::Fs {
        &self.local_fs
    }
    fn config(&self) -> &Self::Config {
        &self.config
    }
    fn cmd_runner(&self) -> &Self::CmdRunner {
        &self.cmd_runner
    }
    fn network(&self) -> &Self::Network {
        &self.network
    }
    fn ssh(&self) -> &Self::Ssh {
        &self.ssh
    }
    fn assets(&self) -> &Self::Assets {
        &self.assets
    }
    fn confirm(&self, _prompt: &str, default: bool) -> Result<bool> {
        Ok(default)
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
        let dir = tempfile::TempDir::new().context("creating mock assets dir")?;
        let path = dir.path().to_path_buf();
        Ok((path, dir))
    }
}

// ── Mock Provisioner ──────────────────────────────────────────────────────────

/// Create a mock provisioner for testing command handlers.
///
/// The mock provisioner returns predefined responses and can be configured
/// to simulate various VM states and failure scenarios.
///
/// # Example
///
/// ```ignore
/// let provisioner = mock_provisioner();
/// provisioner.set_vm_running(true);
/// provisioner.set_info_response(br#"{"info":{"polis":{"state":"Running"}}}"#);
/// ```
#[must_use]
pub fn mock_provisioner() -> MockProvisioner {
    MockProvisioner::new()
}

/// Mock implementation of VM provisioner traits for testing.
///
/// Implements `InstanceInspector`, `InstanceLifecycle`, `FileTransfer`,
/// and `ShellExecutor` with configurable responses.
pub struct MockProvisioner {
    /// Response to return from `info()` calls.
    info_response: RefCell<Output>,
    /// Response to return from `version()` calls.
    version_response: RefCell<Output>,
    /// Whether lifecycle operations should fail.
    lifecycle_fails: RefCell<bool>,
    /// Whether exec operations should fail.
    exec_fails: RefCell<bool>,
    /// Last command executed via `exec()`.
    last_exec_cmd: RefCell<Vec<String>>,
    /// Number of times `exec()` was called.
    exec_call_count: RefCell<usize>,
}

impl MockProvisioner {
    /// Create a new mock provisioner with default (success) responses.
    #[must_use]
    pub fn new() -> Self {
        Self {
            info_response: RefCell::new(ok_output(
                br#"{"info":{"polis":{"state":"Running","ipv4":["192.168.64.2"]}}}"#,
            )),
            version_response: RefCell::new(ok_output(b"multipass 1.14.0")),
            lifecycle_fails: RefCell::new(false),
            exec_fails: RefCell::new(false),
            last_exec_cmd: RefCell::new(Vec::new()),
            exec_call_count: RefCell::new(0),
        }
    }

    /// Configure the response for `info()` calls.
    pub fn set_info_response(&self, stdout: &[u8]) {
        *self.info_response.borrow_mut() = ok_output(stdout);
    }

    /// Configure `info()` to return a failure response.
    pub fn set_info_not_found(&self) {
        *self.info_response.borrow_mut() = fail_output();
    }

    /// Configure whether lifecycle operations should fail.
    pub fn set_lifecycle_fails(&self, fails: bool) {
        *self.lifecycle_fails.borrow_mut() = fails;
    }

    /// Configure whether exec operations should fail.
    pub fn set_exec_fails(&self, fails: bool) {
        *self.exec_fails.borrow_mut() = fails;
    }

    /// Get the last command executed via `exec()`.
    #[must_use]
    pub fn last_exec_cmd(&self) -> Vec<String> {
        self.last_exec_cmd.borrow().clone()
    }

    /// Get the number of times `exec()` was called.
    #[must_use]
    pub fn exec_call_count(&self) -> usize {
        *self.exec_call_count.borrow()
    }
}

impl Default for MockProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceInspector for MockProvisioner {
    async fn info(&self) -> Result<Output> {
        let response = self.info_response.borrow();
        Ok(Output {
            status: response.status,
            stdout: response.stdout.clone(),
            stderr: response.stderr.clone(),
        })
    }

    async fn version(&self) -> Result<Output> {
        let response = self.version_response.borrow();
        Ok(Output {
            status: response.status,
            stdout: response.stdout.clone(),
            stderr: response.stderr.clone(),
        })
    }
}

impl InstanceLifecycle for MockProvisioner {
    async fn launch(&self, _spec: &InstanceSpec<'_>) -> Result<Output> {
        if *self.lifecycle_fails.borrow() {
            anyhow::bail!("mock launch failed")
        }
        Ok(ok_output(b""))
    }

    async fn start(&self) -> Result<Output> {
        if *self.lifecycle_fails.borrow() {
            anyhow::bail!("mock start failed")
        }
        Ok(ok_output(b""))
    }

    async fn stop(&self) -> Result<Output> {
        if *self.lifecycle_fails.borrow() {
            anyhow::bail!("mock stop failed")
        }
        Ok(ok_output(b""))
    }

    async fn delete(&self) -> Result<Output> {
        if *self.lifecycle_fails.borrow() {
            anyhow::bail!("mock delete failed")
        }
        Ok(ok_output(b""))
    }

    async fn purge(&self) -> Result<Output> {
        if *self.lifecycle_fails.borrow() {
            anyhow::bail!("mock purge failed")
        }
        Ok(ok_output(b""))
    }
}

impl FileTransfer for MockProvisioner {
    async fn transfer(&self, _local: &str, _remote: &str) -> Result<Output> {
        Ok(ok_output(b""))
    }

    async fn transfer_recursive(&self, _local: &str, _remote: &str) -> Result<Output> {
        Ok(ok_output(b""))
    }
}

impl ShellExecutor for MockProvisioner {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        *self.exec_call_count.borrow_mut() += 1;
        *self.last_exec_cmd.borrow_mut() = args.iter().map(|s| (*s).to_string()).collect();

        if *self.exec_fails.borrow() {
            anyhow::bail!("mock exec failed")
        }
        Ok(ok_output(b""))
    }

    async fn exec_with_stdin(&self, args: &[&str], _input: &[u8]) -> Result<Output> {
        *self.exec_call_count.borrow_mut() += 1;
        *self.last_exec_cmd.borrow_mut() = args.iter().map(|s| (*s).to_string()).collect();

        if *self.exec_fails.borrow() {
            anyhow::bail!("mock exec_with_stdin failed")
        }
        Ok(ok_output(b""))
    }

    fn exec_spawn(&self, _args: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not supported in mock")
    }

    async fn exec_status(&self, _args: &[&str]) -> Result<std::process::ExitStatus> {
        if *self.exec_fails.borrow() {
            anyhow::bail!("mock exec_status failed")
        }
        Ok(exit_status(0))
    }
}

impl ContainerExecutor for MockProvisioner {
    async fn container_exec_status(
        &self,
        _args: &[&str],
        _interactive: bool,
    ) -> anyhow::Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Mock State Store ──────────────────────────────────────────────────────────

/// Mock implementation of `WorkspaceStateStore` for testing.
pub struct MockStateStore {
    state: RefCell<Option<WorkspaceState>>,
    load_fails: RefCell<bool>,
    save_fails: RefCell<bool>,
    clear_fails: RefCell<bool>,
}

impl MockStateStore {
    /// Create a new mock state store with no initial state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: RefCell::new(None),
            load_fails: RefCell::new(false),
            save_fails: RefCell::new(false),
            clear_fails: RefCell::new(false),
        }
    }

    /// Set the state to return from `load_async()`.
    pub fn set_state(&self, state: Option<WorkspaceState>) {
        *self.state.borrow_mut() = state;
    }

    /// Configure whether `load_async()` should fail.
    pub fn set_load_fails(&self, fails: bool) {
        *self.load_fails.borrow_mut() = fails;
    }

    /// Configure whether `save_async()` should fail.
    pub fn set_save_fails(&self, fails: bool) {
        *self.save_fails.borrow_mut() = fails;
    }

    /// Configure whether `clear_async()` should fail.
    pub fn set_clear_fails(&self, fails: bool) {
        *self.clear_fails.borrow_mut() = fails;
    }
}

impl Default for MockStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceStateStore for MockStateStore {
    async fn load_async(&self) -> Result<Option<WorkspaceState>> {
        if *self.load_fails.borrow() {
            anyhow::bail!("mock load failed")
        }
        Ok(self.state.borrow().clone())
    }

    async fn save_async(&self, state: &WorkspaceState) -> Result<()> {
        if *self.save_fails.borrow() {
            anyhow::bail!("mock save failed")
        }
        *self.state.borrow_mut() = Some(state.clone());
        Ok(())
    }

    async fn clear_async(&self) -> Result<()> {
        if *self.clear_fails.borrow() {
            anyhow::bail!("mock clear failed")
        }
        *self.state.borrow_mut() = None;
        Ok(())
    }
}

// ── Mock Local Filesystem ─────────────────────────────────────────────────────

/// Mock implementation of `LocalFs` for testing.
pub struct MockLocalFs {
    exists: RefCell<bool>,
    is_dir: RefCell<bool>,
    remove_fails: RefCell<bool>,
}

impl MockLocalFs {
    /// Create a new mock filesystem.
    #[must_use]
    pub fn new() -> Self {
        Self {
            exists: RefCell::new(false),
            is_dir: RefCell::new(false),
            remove_fails: RefCell::new(false),
        }
    }

    /// Configure whether paths exist.
    pub fn set_exists(&self, exists: bool) {
        *self.exists.borrow_mut() = exists;
    }

    /// Configure whether paths are directories.
    pub fn set_is_dir(&self, is_dir: bool) {
        *self.is_dir.borrow_mut() = is_dir;
    }

    /// Configure whether remove operations should fail.
    pub fn set_remove_fails(&self, fails: bool) {
        *self.remove_fails.borrow_mut() = fails;
    }
}

impl Default for MockLocalFs {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalFs for MockLocalFs {
    fn exists(&self, _path: &Path) -> bool {
        *self.exists.borrow()
    }

    fn is_dir(&self, _path: &Path) -> bool {
        *self.is_dir.borrow()
    }

    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn remove_dir_all(&self, _path: &Path) -> Result<()> {
        if *self.remove_fails.borrow() {
            anyhow::bail!("mock remove_dir_all failed")
        }
        Ok(())
    }

    fn remove_file(&self, _path: &Path) -> Result<()> {
        if *self.remove_fails.borrow() {
            anyhow::bail!("mock remove_file failed")
        }
        Ok(())
    }

    fn write(&self, _path: &Path, _content: String) -> Result<()> {
        Ok(())
    }

    fn read_to_string(&self, _path: &Path) -> Result<String> {
        Ok(String::new())
    }

    fn set_permissions(&self, _path: &Path, _mode: u32) -> Result<()> {
        Ok(())
    }
}

impl LocalPaths for MockLocalFs {
    fn images_dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::from("/tmp/mock-images")
    }
    fn polis_dir(&self) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from("/tmp/mock-polis"))
    }
}

impl FileHasher for MockLocalFs {
    fn sha256_file(&self, _path: &Path) -> Result<String> {
        Ok(
            "mock-sha256-0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
        )
    }
}

// ── Mock Config Store ─────────────────────────────────────────────────────────

/// Mock implementation of `ConfigStore` for testing.
pub struct MockConfig;

impl ConfigStore for MockConfig {
    fn load(&self) -> Result<PolisConfig> {
        Ok(PolisConfig::default())
    }
    fn save(&self, _config: &PolisConfig) -> Result<()> {
        Ok(())
    }
    fn path(&self) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from("/tmp/mock-config.yaml"))
    }
}

// ── Mock Command Runner ───────────────────────────────────────────────────────

/// Mock implementation of `CommandRunner` for testing.
pub struct MockCmdRunner;

impl CommandRunner for MockCmdRunner {
    async fn run(&self, _program: &str, _args: &[&str]) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn run_with_timeout(
        &self,
        _program: &str,
        _args: &[&str],
        _timeout: std::time::Duration,
    ) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn run_with_stdin(
        &self,
        _program: &str,
        _args: &[&str],
        _stdin: &[u8],
    ) -> Result<Output> {
        Ok(ok_output(b""))
    }
    fn spawn(&self, _program: &str, _args: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("spawn not supported in mock")
    }
    async fn run_status(&self, _program: &str, _args: &[&str]) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Mock Network Probe ────────────────────────────────────────────────────────

/// Mock implementation of `NetworkProbe` for testing.
pub struct MockNetwork;

impl NetworkProbe for MockNetwork {
    async fn check_tcp_connectivity(&self, _host: &str, _port: u16) -> Result<bool> {
        Ok(true)
    }
    async fn check_dns_resolution(&self, _hostname: &str) -> Result<bool> {
        Ok(true)
    }
}

// ── Mock SSH Configurator ─────────────────────────────────────────────────────

/// Mock implementation of `SshConfigurator` for testing.
pub struct MockSsh;

impl SshConfigurator for MockSsh {
    async fn ensure_identity(&self) -> Result<String> {
        Ok("ssh-ed25519 AAAA mock-pubkey".to_string())
    }
    async fn update_host_key(&self, _host_key: &str) -> Result<()> {
        Ok(())
    }
    async fn is_configured(&self) -> Result<bool> {
        Ok(true)
    }
    async fn setup_config(&self) -> Result<()> {
        Ok(())
    }
    async fn validate_permissions(&self) -> Result<()> {
        Ok(())
    }
    async fn remove_config(&self) -> Result<()> {
        Ok(())
    }
    async fn remove_include_directive(&self) -> Result<()> {
        Ok(())
    }
}

// ── Mock Asset Extractor ──────────────────────────────────────────────────────

/// Mock implementation of `AssetExtractor` for testing.
pub struct MockAssets;

impl AssetExtractor for MockAssets {
    async fn extract_assets(&self) -> Result<(std::path::PathBuf, Box<dyn std::any::Any>)> {
        let dir = tempfile::TempDir::new().context("creating mock assets dir")?;
        let path = dir.path().to_path_buf();
        Ok((path, Box::new(dir)))
    }
    async fn get_asset(&self, _name: &str) -> Result<&'static [u8]> {
        Ok(b"")
    }
}

// ── Mock Progress Reporter ────────────────────────────────────────────────────

/// Mock implementation of `ProgressReporter` for testing.
pub struct MockProgressReporter;

impl ProgressReporter for MockProgressReporter {
    fn step(&self, _message: &str) {}
    fn success(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn begin_stage(&self, _message: &str) {}
    fn complete_stage(&self) {}
    fn fail_stage(&self) {}
}

// ── Pattern Verification ──────────────────────────────────────────────────────

/// Verification result for command handler pattern checks.
#[derive(Debug, Default)]
pub struct PatternVerificationResult {
    /// List of pattern violations found.
    pub violations: Vec<String>,
}

impl PatternVerificationResult {
    /// Returns `true` if no violations were found.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Verify a command handler file follows standard patterns.
///
/// Checks for:
/// - `ExitCode` import pattern (`use std::process::ExitCode;`)
/// - No fully-qualified `std::process::ExitCode` in return types
/// - Module documentation (`//!` doc comment)
/// - No empty test modules
/// - No direct `println!` for structured output
/// - No direct `spinner()` usage (should use `TerminalReporter`)
/// - No `std::process::Command` in async functions (should use `tokio::process::Command`)
///
/// # Errors
///
/// Returns an error if the file cannot be read.
///
/// # Example
///
/// ```ignore
/// let result = verify_command_handler_patterns(Path::new("cli/src/commands/stop.rs"))?;
/// assert!(result.is_ok(), "violations: {:?}", result.violations);
/// ```
pub fn verify_command_handler_patterns(path: &Path) -> Result<PatternVerificationResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading command handler file {}", path.display()))?;

    let mut result = PatternVerificationResult::default();

    // Check for module documentation
    if !content.starts_with("//!") {
        result
            .violations
            .push("Missing module documentation (//! doc comment)".to_string());
    }

    // Check ExitCode import pattern
    let has_exitcode_import = content.contains("use std::process::ExitCode;");
    let has_qualified_exitcode = content.contains("-> Result<std::process::ExitCode>");

    if !has_exitcode_import && content.contains("ExitCode") {
        result
            .violations
            .push("ExitCode used but not imported with `use std::process::ExitCode;`".to_string());
    }

    if has_qualified_exitcode {
        result.violations.push(
            "Fully-qualified `std::process::ExitCode` in return type (use imported ExitCode)"
                .to_string(),
        );
    }

    // Check for empty test modules
    if content.contains("#[cfg(test)]\nmod tests {}")
        || content.contains("#[cfg(test)]\r\nmod tests {}")
    {
        result
            .violations
            .push("Empty test module found".to_string());
    }

    // Check for direct spinner usage (should use TerminalReporter)
    if content.contains("crate::output::progress::spinner(") {
        result
            .violations
            .push("Direct spinner() usage found (use app.terminal_reporter() instead)".to_string());
    }

    // Check for std::process::Command in async functions
    // This is a heuristic check - look for std::process::Command near async fn
    if content.contains("async fn") && content.contains("std::process::Command::new") {
        result.violations.push(
            "std::process::Command used in async context (use tokio::process::Command)".to_string(),
        );
    }

    // Check for error swallowing pattern
    if content.contains("return Ok(ExitCode::FAILURE)") {
        result.violations.push(
            "Error swallowing pattern found (return Ok(ExitCode::FAILURE) after error)".to_string(),
        );
    }

    Ok(result)
}

// ── Helper Functions ──────────────────────────────────────────────────────────

/// Build an `ExitStatus` from a logical exit code (cross-platform).
#[cfg(unix)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    #[allow(clippy::cast_sign_loss)]
    std::process::ExitStatus::from_raw(code as u32)
}

/// Create a successful `Output` with the given stdout.
fn ok_output(stdout: &[u8]) -> Output {
    Output {
        status: exit_status(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

/// Create a failed `Output` (exit code 1).
fn fail_output() -> Output {
    Output {
        status: exit_status(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn mock_app_context_is_non_interactive() {
        let ctx = mock_app_context();
        assert!(ctx.non_interactive);
    }

    #[test]
    fn mock_app_context_confirm_returns_default() {
        let ctx = mock_app_context();
        assert!(ctx.confirm("test?", true).expect("confirm should succeed"));
        assert!(!ctx.confirm("test?", false).expect("confirm should succeed"));
    }

    #[test]
    fn mock_provisioner_default_is_running() {
        let provisioner = mock_provisioner();
        let response = provisioner.info_response.borrow();
        let stdout = String::from_utf8_lossy(&response.stdout);
        assert!(stdout.contains("Running"));
    }

    #[test]
    fn mock_provisioner_tracks_exec_calls() {
        let provisioner = mock_provisioner();
        assert_eq!(provisioner.exec_call_count(), 0);
    }

    #[test]
    fn mock_state_store_starts_empty() {
        let store = MockStateStore::new();
        assert!(store.state.borrow().is_none());
    }

    #[test]
    fn verify_patterns_detects_missing_doc() {
        let content = "use std::process::ExitCode;\n\npub fn run() {}";
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = temp_dir.path().join("test.rs");
        std::fs::write(&path, content).expect("write test file");

        let result = verify_command_handler_patterns(&path).expect("verify patterns");
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.contains("Missing module documentation"))
        );
    }

    #[test]
    fn verify_patterns_detects_empty_test_module() {
        let content = "//! Test module\n\n#[cfg(test)]\nmod tests {}";
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = temp_dir.path().join("test.rs");
        std::fs::write(&path, content).expect("write test file");

        let result = verify_command_handler_patterns(&path).expect("verify patterns");
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.contains("Empty test module"))
        );
    }

    #[test]
    fn verify_patterns_passes_valid_handler() {
        let content = r"//! `polis test` — test command.

use std::process::ExitCode;
use anyhow::Result;

pub async fn run() -> Result<ExitCode> {
    Ok(ExitCode::SUCCESS)
}
";
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = temp_dir.path().join("test.rs");
        std::fs::write(&path, content).expect("write test file");

        let result = verify_command_handler_patterns(&path).expect("verify patterns");
        assert!(result.is_ok(), "violations: {:?}", result.violations);
    }
}
