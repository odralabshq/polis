//! Shared test helpers for application layer tests.
//!
//! Provides cross-platform `exit_status()`, output helpers, a macro to generate
//! `ShellExecutor` stub methods, and configurable stubs for all port traits.

/// Build an `ExitStatus` from a logical exit code (cross-platform).
#[cfg(unix)]
pub fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
pub fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    #[allow(clippy::cast_sign_loss)]
    std::process::ExitStatus::from_raw(code as u32)
}

pub fn ok_output(stdout: &[u8]) -> std::process::Output {
    std::process::Output {
        status: exit_status(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

pub fn fail_output() -> std::process::Output {
    std::process::Output {
        status: exit_status(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

/// Generate `ShellExecutor` stub methods that bail with "not expected".
///
/// Usage: `impl_shell_executor_stubs!(exec, exec_with_stdin, exec_spawn, exec_status);`
/// Omit any method you implement yourself.
macro_rules! impl_shell_executor_stubs {
    ($($method:ident),* $(,)?) => {
        $(impl_shell_executor_stubs!(@one $method);)*
    };
    (@one exec) => {
        /// # Errors
        /// Stub — always bails.
        async fn exec(&self, _: &[&str]) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("not expected")
        }
    };
    (@one exec_with_stdin) => {
        /// # Errors
        /// Stub — always bails.
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("not expected")
        }
    };
    (@one exec_spawn) => {
        /// # Errors
        /// Stub — always bails.
        fn exec_spawn(&self, _: &[&str]) -> anyhow::Result<tokio::process::Child> {
            anyhow::bail!("not expected")
        }
    };
    (@one exec_status) => {
        /// # Errors
        /// Stub — always bails.
        async fn exec_status(&self, _: &[&str]) -> anyhow::Result<std::process::ExitStatus> {
            anyhow::bail!("not expected")
        }
    };
}

pub(crate) use impl_shell_executor_stubs;

// ── InstanceInspector stub ────────────────────────────────────────────────────

/// Stub for `InstanceInspector`. `info_output` is returned from `info()`.
pub struct InfoStub(pub std::process::Output);

impl crate::application::ports::InstanceInspector for InfoStub {
    async fn info(&self) -> anyhow::Result<std::process::Output> {
        Ok(std::process::Output {
            status: self.0.status,
            stdout: self.0.stdout.clone(),
            stderr: self.0.stderr.clone(),
        })
    }
    async fn version(&self) -> anyhow::Result<std::process::Output> {
        anyhow::bail!("not expected")
    }
}

// ── InstanceLifecycle stub ────────────────────────────────────────────────────

/// Stub for `InstanceLifecycle`. All methods return `ok_output(b"")` by default.
/// Set `stop_fails` to make `stop()` return a failure output.
pub struct LifecycleStub {
    pub stop_fails: bool,
}

impl Default for LifecycleStub {
    fn default() -> Self {
        Self { stop_fails: false }
    }
}

impl crate::application::ports::InstanceLifecycle for LifecycleStub {
    async fn launch(&self, _: &crate::application::ports::InstanceSpec<'_>) -> anyhow::Result<std::process::Output> {
        anyhow::bail!("not expected")
    }
    async fn start(&self) -> anyhow::Result<std::process::Output> {
        Ok(ok_output(b""))
    }
    async fn stop(&self) -> anyhow::Result<std::process::Output> {
        if self.stop_fails { Ok(fail_output()) } else { Ok(ok_output(b"")) }
    }
    async fn delete(&self) -> anyhow::Result<std::process::Output> {
        anyhow::bail!("not expected")
    }
    async fn purge(&self) -> anyhow::Result<std::process::Output> {
        anyhow::bail!("not expected")
    }
}

// ── FileTransfer stub ─────────────────────────────────────────────────────────

/// Stub for `FileTransfer`. Returns `ok_output(b"")` unless `fail` is true.
pub struct TransferStub {
    pub fail: bool,
}

impl crate::application::ports::FileTransfer for TransferStub {
    async fn transfer(&self, _: &str, _: &str) -> anyhow::Result<std::process::Output> {
        if self.fail { Ok(fail_output()) } else { Ok(ok_output(b"")) }
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> anyhow::Result<std::process::Output> {
        if self.fail { Ok(fail_output()) } else { Ok(ok_output(b"")) }
    }
}

// ── WorkspaceStateStore stub ──────────────────────────────────────────────────

use std::sync::Mutex;

/// Stub for `WorkspaceStateStore`. Holds state in memory.
pub struct StateStoreStub(pub Mutex<Option<crate::domain::workspace::WorkspaceState>>);

impl StateStoreStub {
    pub fn empty() -> Self { Self(Mutex::new(None)) }
    pub fn with(state: crate::domain::workspace::WorkspaceState) -> Self {
        Self(Mutex::new(Some(state)))
    }
}

impl crate::application::ports::WorkspaceStateStore for StateStoreStub {
    async fn load_async(&self) -> anyhow::Result<Option<crate::domain::workspace::WorkspaceState>> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn save_async(&self, state: &crate::domain::workspace::WorkspaceState) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = Some(state.clone());
        Ok(())
    }
    async fn clear_async(&self) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

// ── ProgressReporter stub ─────────────────────────────────────────────────────

/// No-op progress reporter for tests.
pub struct NoopReporter;

impl crate::application::ports::ProgressReporter for NoopReporter {
    fn step(&self, _: &str) {}
    fn success(&self, _: &str) {}
    fn warn(&self, _: &str) {}
}

// ── SshConfigurator stub ──────────────────────────────────────────────────────

/// Stub for `SshConfigurator`.
pub struct SshConfiguratorStub {
    pub is_configured: bool,
    pub pubkey: String,
}

impl SshConfiguratorStub {
    pub fn configured() -> Self {
        Self { is_configured: true, pubkey: "ssh-ed25519 AAAA test@host".to_string() }
    }
    pub fn unconfigured() -> Self {
        Self { is_configured: false, pubkey: "ssh-ed25519 AAAA test@host".to_string() }
    }
}

impl crate::application::ports::SshConfigurator for SshConfiguratorStub {
    async fn ensure_identity(&self) -> anyhow::Result<String> {
        Ok(self.pubkey.clone())
    }
    async fn update_host_key(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
    async fn is_configured(&self) -> anyhow::Result<bool> { Ok(self.is_configured) }
    async fn setup_config(&self) -> anyhow::Result<()> { Ok(()) }
    async fn validate_permissions(&self) -> anyhow::Result<()> { Ok(()) }
    async fn remove_config(&self) -> anyhow::Result<()> { Ok(()) }
    async fn remove_include_directive(&self) -> anyhow::Result<()> { Ok(()) }
}

// ── ProcessLauncher stub ──────────────────────────────────────────────────────

/// Stub for `ProcessLauncher`. Returns success or failure exit status.
pub struct ProcessLauncherStub(pub bool);

impl crate::application::ports::ProcessLauncher for ProcessLauncherStub {
    async fn launch(&self, _: &str, _: &[&str]) -> anyhow::Result<std::process::ExitStatus> {
        Ok(exit_status(if self.0 { 0 } else { 1 }))
    }
}

// ── FileHasher stub ───────────────────────────────────────────────────────────

/// Stub for `FileHasher`. Always returns the configured hash string.
pub struct FileHasherStub(pub String);

impl crate::application::ports::FileHasher for FileHasherStub {
    fn sha256_file(&self, _: &std::path::Path) -> anyhow::Result<String> {
        Ok(self.0.clone())
    }
}

// ── LocalFs stub ──────────────────────────────────────────────────────────────

use std::collections::HashMap;

/// In-memory stub for `LocalFs`. Tracks written files and existing paths.
pub struct LocalFsStub {
    pub existing: Vec<std::path::PathBuf>,
    pub written: Mutex<HashMap<std::path::PathBuf, String>>,
    pub write_fails: bool,
}

impl LocalFsStub {
    pub fn new(existing: Vec<std::path::PathBuf>) -> Self {
        Self { existing, written: Mutex::new(HashMap::new()), write_fails: false }
    }
}

impl crate::application::ports::LocalFs for LocalFsStub {
    fn exists(&self, path: &std::path::Path) -> bool {
        self.existing.iter().any(|p| p == path)
    }
    fn create_dir_all(&self, _: &std::path::Path) -> anyhow::Result<()> { Ok(()) }
    fn remove_dir_all(&self, _: &std::path::Path) -> anyhow::Result<()> { Ok(()) }
    fn remove_file(&self, _: &std::path::Path) -> anyhow::Result<()> { Ok(()) }
    fn write(&self, path: &std::path::Path, content: String) -> anyhow::Result<()> {
        if self.write_fails {
            anyhow::bail!("write failed")
        }
        self.written.lock().unwrap().insert(path.to_path_buf(), content);
        Ok(())
    }
    fn read_to_string(&self, path: &std::path::Path) -> anyhow::Result<String> {
        self.written.lock().unwrap().get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("file not found: {}", path.display()))
    }
    fn set_permissions(&self, _: &std::path::Path, _: u32) -> anyhow::Result<()> { Ok(()) }
    fn is_dir(&self, _: &std::path::Path) -> bool { false }
}
