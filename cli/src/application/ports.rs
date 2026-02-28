//! Port trait definitions for the Application layer.
//!
//! Ports are the interfaces (contracts) that infrastructure must fulfill.
//! This file imports only from `crate::domain` — never from `crate::infra`,
//! `crate::commands`, or `crate::output`.

use std::collections::HashMap;
use std::process::Output;

use anyhow::Result;

use crate::domain::{DoctorChecks, WorkspaceState};

// ── Constants ─────────────────────────────────────────────────────────────────

/// The canonical VM instance name used by all trait implementations.
pub const POLIS_INSTANCE: &str = "polis";

// ── Value Types ───────────────────────────────────────────────────────────────

/// Launch parameters for creating a new VM instance.
/// Preserved exactly from `provisioner.rs` (move-only).
pub struct InstanceSpec<'a> {
    /// Ubuntu image to launch, e.g. `"24.04"`.
    pub image: &'a str,
    /// Number of vCPUs, e.g. `"2"`.
    pub cpus: &'a str,
    /// Memory size, e.g. `"8G"`.
    pub memory: &'a str,
    /// Disk size, e.g. `"40G"`.
    pub disk: &'a str,
    /// Optional path to a cloud-init YAML file.
    pub cloud_init: Option<&'a str>,
    /// Launch timeout in seconds. Defaults to `"600"` when `None`.
    pub timeout: Option<&'a str>,
}

/// VM instance state — preserved exactly from `provisioner.rs` (move-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Reserved for future use — VmState in lifecycle.rs is the active enum
pub enum InstanceState {
    Running,
    Stopped,
    Starting,
    Stopping,
    NotFound,
    Error,
}

// ── VM Port Traits ────────────────────────────────────────────────────────────

/// VM lifecycle operations: create, start, stop, destroy.
#[allow(async_fn_in_trait)]
pub trait InstanceLifecycle {
    /// Launch a new VM instance with the given spec.
    async fn launch(&self, spec: &InstanceSpec<'_>) -> Result<Output>;
    /// Start a stopped VM instance.
    async fn start(&self) -> Result<Output>;
    /// Stop a running VM instance.
    async fn stop(&self) -> Result<Output>;
    /// Delete the VM instance (can be recovered with `recover`).
    async fn delete(&self) -> Result<Output>;
    /// Permanently remove all deleted instances.
    async fn purge(&self) -> Result<Output>;
}

/// VM state inspection: query info and version.
#[allow(async_fn_in_trait)]
pub trait InstanceInspector {
    /// Get VM instance info as JSON.
    async fn info(&self) -> Result<Output>;
    /// Get the provisioner backend version.
    async fn version(&self) -> Result<Output>;
}

/// Host-to-VM file transfer operations.
#[allow(async_fn_in_trait)]
pub trait FileTransfer {
    /// Transfer a single file from host to VM.
    async fn transfer(&self, local: &str, remote: &str) -> Result<Output>;
    /// Recursively transfer a directory from host to VM.
    async fn transfer_recursive(&self, local: &str, remote: &str) -> Result<Output>;
}

/// Command execution inside the VM.
#[allow(async_fn_in_trait)]
pub trait ShellExecutor {
    /// Execute a command inside the VM and capture output.
    async fn exec(&self, args: &[&str]) -> Result<Output>;
    /// Execute a command inside the VM with stdin piped from `input`.
    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<Output>;
    /// Spawn a command inside the VM with piped stdin/stdout for STDIO bridging.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be spawned.
    #[allow(dead_code)] // Defined for future interactive use cases
    fn exec_spawn(&self, args: &[&str]) -> Result<tokio::process::Child>;
    /// Execute a command inside the VM with inherited stdio (interactive).
    async fn exec_status(&self, args: &[&str]) -> Result<std::process::ExitStatus>;
}

/// Composite trait — any type implementing all four sub-traits is a `VmProvisioner`.
pub trait VmProvisioner:
    InstanceLifecycle + InstanceInspector + FileTransfer + ShellExecutor
{
}

/// Blanket implementation: any type implementing all four sub-traits is a `VmProvisioner`.
impl<T> VmProvisioner for T where
    T: InstanceLifecycle + InstanceInspector + FileTransfer + ShellExecutor
{
}

// ── Command Runner Port ───────────────────────────────────────────────────────

/// Abstracts process execution so infrastructure can be swapped or mocked.
#[allow(async_fn_in_trait)]
pub trait CommandRunner {
    /// Run a program and capture its output.
    ///
    /// Implementations should delegate to `run_with_timeout` using the
    /// instance's configured default timeout.
    async fn run(&self, program: &str, args: &[&str]) -> Result<Output>;
    /// Run a program with a custom timeout override.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be spawned or exceeds `timeout`.
    /// On timeout, the child process must be killed (not left orphaned).
    async fn run_with_timeout(
        &self,
        program: &str,
        args: &[&str],
        timeout: std::time::Duration,
    ) -> Result<Output>;
    /// Run a program with stdin piped from `stdin`.
    async fn run_with_stdin(&self, program: &str, args: &[&str], stdin: &[u8]) -> Result<Output>;
    /// Spawn a program without waiting for it to finish.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be spawned.
    #[allow(dead_code)] // Reserved for future background process use cases
    fn spawn(&self, program: &str, args: &[&str]) -> Result<tokio::process::Child>;
    /// Run a program and return only its exit status.
    async fn run_status(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus>;
}

// ── Progress Reporting Port ───────────────────────────────────────────────────

/// Abstracts progress reporting so services can emit events without
/// depending on the Presentation layer. Sync trait — no async needed.
pub trait ProgressReporter {
    /// Emit an in-progress step message.
    fn step(&self, message: &str);
    /// Emit a success message.
    fn success(&self, message: &str);
    /// Emit a warning message.
    #[allow(dead_code)] // Not yet called from all service implementations
    fn warn(&self, message: &str);
}

// ── State and Filesystem Ports ────────────────────────────────────────────────

/// Abstracts workspace state persistence (load/save).
#[allow(async_fn_in_trait)]
pub trait WorkspaceStateStore {
    /// Load the current workspace state, returning `None` if no state exists.
    async fn load_async(&self) -> Result<Option<WorkspaceState>>;
    /// Persist the given workspace state.
    async fn save_async(&self, state: &WorkspaceState) -> Result<()>;
}

/// Abstracts writing agent artifact files to the local filesystem.
#[allow(async_fn_in_trait)]
#[allow(dead_code)] // Not yet wired from command handlers
pub trait LocalArtifactWriter {
    /// Write agent artifact files and return the directory path.
    async fn write_agent_artifacts(
        &self,
        agent_name: &str,
        files: HashMap<String, String>,
    ) -> Result<std::path::PathBuf>;
}

// ── Network Probe Port ────────────────────────────────────────────────────────

/// Abstracts network connectivity checks so application services can be tested
/// without real network access.
#[allow(async_fn_in_trait)]
pub trait NetworkProbe {
    /// Check TCP connectivity to the given host and port.
    async fn check_tcp_connectivity(&self, host: &str, port: u16) -> Result<bool>;
    /// Check DNS resolution for the given hostname.
    async fn check_dns_resolution(&self, hostname: &str) -> Result<bool>;
}

// ── Health Port ───────────────────────────────────────────────────────────────

/// Abstracts health probing so the doctor service can be tested with mocks.
#[allow(async_fn_in_trait)]
#[allow(dead_code)] // Not yet used from application services
pub trait HealthProbe {
    /// Run all health probes and return the aggregated results.
    async fn probe_all(&self) -> Result<DoctorChecks>;
}

// ── Asset Management Port ─────────────────────────────────────────────────────

/// Abstracts extraction of embedded assets.
#[allow(async_fn_in_trait)]
pub trait AssetExtractor {
    /// Extract all embedded assets to a temporary directory.
    ///
    /// The returned directory must be accessible to the VM provisioner (e.g.
    /// under `$HOME` on Linux if using snap-confined Multipass).
    ///
    /// Returns `(path, guard)` where `path` is the directory containing the
    /// extracted files and `guard` is an object that should delete the
    /// directory when dropped.
    async fn extract_assets(&self) -> Result<(std::path::PathBuf, Box<dyn std::any::Any>)>;

    /// Get the raw bytes of a single embedded asset.
    async fn get_asset(&self, name: &str) -> Result<&'static [u8]>;
}

// ── Filesystem and Path Ports ─────────────────────────────────────────────────

/// Abstracts file hashing operations.
pub trait FileHasher {
    /// Compute the SHA-256 hash of a file.
    fn sha256_file(&self, path: &std::path::Path) -> Result<String>;
}

/// Abstracts local filesystem paths.
pub trait LocalPaths {
    /// Get the directory where VM images are stored.
    fn images_dir(&self) -> std::path::PathBuf;
}

// ── SSH Configuration Port ────────────────────────────────────────────────────

/// Abstracts SSH environment management for VM access.
#[allow(async_fn_in_trait)]
pub trait SshConfigurator {
    /// Ensure the user's SSH identity (private key) exists.
    /// Returns the public key material.
    async fn ensure_identity(&self) -> Result<String>;

    /// Update the known_hosts file with the given host key.
    async fn update_host_key(&self, host_key: &str) -> Result<()>;

    /// Check if the local SSH config is correctly included in the user's config.
    async fn is_configured(&self) -> Result<bool>;

    /// Set up the local SSH config.
    async fn setup_config(&self) -> Result<()>;
}
