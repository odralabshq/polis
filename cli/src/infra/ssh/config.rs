//! SSH config file management — `SshConfigManager` and trait implementations.
//!
//! Manages the polis SSH config files (`~/.ssh/config.d/polis`) and the
//! `Include` directive in `~/.ssh/config`. Also implements the
//! [`SshConfigurator`](crate::application::ports::SshConfigurator) and
//! [`HostKeyExtractor`](crate::application::ports::HostKeyExtractor) port traits.

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::identity::{IdentityKeyProvider, OsIdentityKeyProvider};
use super::known_hosts::{KnownHostsManager, KnownHostsOps};
use super::sockets::{OsSocketsDir, SocketsDir};
use crate::infra::blocking::spawn_blocking_io;
use crate::infra::polis_dir::PolisDir;
use crate::infra::secure_fs::SecureFs;
// ---------------------------------------------------------------------------
// SSH config templates
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
const POLIS_SSH_CONFIG: &str = "\
# ~/.ssh/config.d/polis (managed by polis — DO NOT EDIT)
Host workspace
    HostName workspace
    User polis
    ProxyCommand polis _ssh-proxy
    StrictHostKeyChecking yes
    UserKnownHostsFile ~/.polis/known_hosts
    IdentityFile ~/.polis/id_ed25519
    ControlMaster auto
    ControlPath ~/.ssh/config.d/polis-sockets/%r@%h:%p
    ControlPersist 30s
    ForwardAgent no
    IdentitiesOnly yes
";

// ---------------------------------------------------------------------------
// SshConfigManager
// ---------------------------------------------------------------------------

/// Manages the polis SSH config files and `~/.ssh/config` Include directive.
pub struct SshConfigManager {
    polis_config_path: std::path::PathBuf,
    user_config_path: std::path::PathBuf,
    polis_root: std::path::PathBuf,
    sockets_dir: Box<dyn SocketsDir>,
    known_hosts: Box<dyn KnownHostsOps>,
    identity_provider: Box<dyn IdentityKeyProvider>,
}

impl SshConfigManager {
    /// Creates a manager using the real `$HOME`-based paths.
    /// # Errors
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let polis_dir = PolisDir::new()?;
        let home = polis_dir
            .root()
            .parent()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
            .to_path_buf();
        Ok(Self::with_deps(
            home.join(".ssh").join("config.d").join("polis"),
            home.join(".ssh").join("config"),
            polis_dir.root().to_path_buf(),
            Box::new(OsSocketsDir::new(
                home.join(".ssh").join("config.d").join("polis-sockets"),
            )),
            Box::new(KnownHostsManager::new()?),
            Box::new(OsIdentityKeyProvider::new(&polis_dir)),
        ))
    }

    /// Creates a manager with explicit paths and injected dependencies (for testing).
    #[must_use]
    pub fn with_deps(
        polis_config_path: std::path::PathBuf,
        user_config_path: std::path::PathBuf,
        polis_root: std::path::PathBuf,
        sockets_dir: Box<dyn SocketsDir>,
        known_hosts: Box<dyn KnownHostsOps>,
        identity_provider: Box<dyn IdentityKeyProvider>,
    ) -> Self {
        Self {
            polis_config_path,
            user_config_path,
            polis_root,
            sockets_dir,
            known_hosts,
            identity_provider,
        }
    }

    /// Returns `true` if the polis SSH config exists **and** the Include
    /// directive is present in `~/.ssh/config`.
    /// # Errors
    /// Returns an error if the user SSH config cannot be read.
    pub fn is_configured(&self) -> Result<bool> {
        if !self.polis_config_path.exists() {
            return Ok(false);
        }
        if self.user_config_path.exists() {
            let content = std::fs::read_to_string(&self.user_config_path)
                .with_context(|| format!("read {}", self.user_config_path.display()))?;
            if content.contains("Include config.d/polis") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Writes the hardened polis SSH config to `~/.ssh/config.d/polis`.
    /// Creates `~/.ssh/config.d/` with 0700 permissions if it does not exist.
    /// Sets file permissions to 0600 on Unix.
    /// # Errors
    /// Returns an error if the file cannot be written or permissions cannot be set.
    pub fn create_polis_config(&self) -> Result<()> {
        // ControlMaster/ControlPath/ControlPersist use Unix domain sockets and
        // are not supported by Windows OpenSSH — omit them on Windows.
        // Windows OpenSSH ProxyCommand requires absolute path to executable.
        #[cfg(not(windows))]
        let config = POLIS_SSH_CONFIG.to_owned();
        #[cfg(windows)]
        let config = format!(
            "# ~/.ssh/config.d/polis (managed by polis — DO NOT EDIT)\nHost workspace\n    HostName workspace\n    User polis\n    ProxyCommand \"{}\" _ssh-proxy\n    StrictHostKeyChecking yes\n    UserKnownHostsFile ~/.polis/known_hosts\n    IdentityFile ~/.polis/id_ed25519\n    ForwardAgent no\n    IdentitiesOnly yes\n",
            std::env::current_exe()
                .unwrap_or_else(|_| std::path::PathBuf::from("polis.exe"))
                .display()
        );
        if let Some(parent) = self.polis_config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            SecureFs::set_permissions(parent, 0o700)?;
        }
        std::fs::write(&self.polis_config_path, config)
            .with_context(|| format!("write {}", self.polis_config_path.display()))?;
        SecureFs::set_permissions(&self.polis_config_path, 0o600)?;
        Ok(())
    }

    /// Prepends `Include config.d/polis` to `~/.ssh/config`, creating
    /// the file if absent. Idempotent.
    /// # Errors
    /// Returns an error if the file cannot be read or written.
    pub fn add_include_directive(&self) -> Result<()> {
        const INCLUDE: &str = "Include config.d/polis\n";
        if let Some(parent) = self.user_config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            SecureFs::set_permissions(parent, 0o700)?;
        }
        if self.user_config_path.exists() {
            let content = std::fs::read_to_string(&self.user_config_path)
                .with_context(|| format!("read {}", self.user_config_path.display()))?;
            if content.contains("Include config.d/polis") {
                return Ok(());
            }
            std::fs::write(&self.user_config_path, format!("{INCLUDE}{content}"))
                .with_context(|| format!("write {}", self.user_config_path.display()))?;
        } else {
            std::fs::write(&self.user_config_path, INCLUDE)
                .with_context(|| format!("write {}", self.user_config_path.display()))?;
        }
        SecureFs::set_permissions(&self.user_config_path, 0o600)?;
        Ok(())
    }

    /// Removes the legacy `~/.polis/ssh_config` file and the old
    /// `Include ~/.polis/ssh_config` directive from `~/.ssh/config`.
    /// Idempotent — no error if the legacy file does not exist.
    /// # Errors
    /// Returns an error if the file cannot be removed or the user SSH config
    /// cannot be read or written.
    pub fn remove_legacy_config(&self) -> Result<()> {
        let legacy_path = self.polis_root.join("ssh_config");

        if legacy_path.exists() {
            std::fs::remove_file(&legacy_path)
                .with_context(|| format!("remove {}", legacy_path.display()))?;

            // Remove the old Include directive from ~/.ssh/config
            if self.user_config_path.exists() {
                let content = std::fs::read_to_string(&self.user_config_path)
                    .with_context(|| format!("read {}", self.user_config_path.display()))?;
                let filtered: String = content
                    .lines()
                    .filter(|line| !line.contains("Include ~/.polis/ssh_config"))
                    .fold(String::new(), |mut acc, line| {
                        acc.push_str(line);
                        acc.push('\n');
                        acc
                    });
                std::fs::write(&self.user_config_path, filtered)
                    .with_context(|| format!("write {}", self.user_config_path.display()))?;
            }
        }
        Ok(())
    }

    /// Creates `~/.polis/sockets/` with permissions 700.
    /// No-op on Windows (`ControlMaster` not supported).
    /// # Errors
    /// Returns an error if the directory cannot be created or permissions set.
    pub fn create_sockets_dir(&self) -> Result<()> {
        self.sockets_dir.ensure_exists()
    }

    /// Validates that `~/.polis/ssh_config` has permissions 600 (V-004).
    /// No-op if the file does not exist yet.
    /// # Errors
    /// Returns an error if the file has unsafe permissions.
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    pub fn validate_permissions(&self) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if self.polis_config_path.exists() {
                let mode = std::fs::metadata(&self.polis_config_path)
                    .with_context(|| format!("stat {}", self.polis_config_path.display()))?
                    .permissions()
                    .mode()
                    & 0o777;
                anyhow::ensure!(
                    mode == 0o600,
                    "~/.polis/ssh_config has unsafe permissions {mode:o}. Run: chmod 600 ~/.polis/ssh_config"
                );
            }
        }
        Ok(())
    }
}


impl crate::application::ports::SshConfigurator for SshConfigManager {
    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn ensure_identity(&self) -> Result<String> {
        self.identity_provider.ensure_identity_key()
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn update_host_key(&self, host_key: &str) -> Result<()> {
        self.known_hosts.update(host_key)
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn is_configured(&self) -> Result<bool> {
        self.is_configured()
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn setup_config(&self) -> Result<()> {
        self.remove_legacy_config()?;
        self.create_polis_config()?;
        self.add_include_directive()?;
        self.create_sockets_dir()?;
        Ok(())
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn validate_permissions(&self) -> Result<()> {
        self.validate_permissions()
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn remove_config(&self) -> Result<()> {
        let path = self.polis_config_path.clone();
        spawn_blocking_io("ssh config remove", move || -> Result<()> {
            if path.exists() {
                std::fs::remove_file(&path)
                    .with_context(|| format!("remove {}", path.display()))?;
            }
            Ok(())
        })
        .await
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn remove_include_directive(&self) -> Result<()> {
        let user_config_path = self.user_config_path.clone();
        spawn_blocking_io("ssh include directive remove", move || -> Result<()> {
            if user_config_path.exists() {
                let content = std::fs::read_to_string(&user_config_path)
                    .with_context(|| format!("read {}", user_config_path.display()))?;
                let filtered: String = content
                    .lines()
                    .filter(|line| !line.contains("Include config.d/polis"))
                    .fold(String::new(), |mut acc, line| {
                        acc.push_str(line);
                        acc.push('\n');
                        acc
                    });
                if filtered != content {
                    std::fs::write(&user_config_path, filtered)
                        .with_context(|| format!("write {}", user_config_path.display()))?;
                }
            }
            Ok(())
        })
        .await
    }
}

impl crate::application::ports::HostKeyExtractor for SshConfigManager {
    async fn extract_host_key(&self) -> Option<String> {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("polis"));
        let output = tokio::process::Command::new(exe)
            .args(["_extract-host-key"])
            .output()
            .await
            .ok()?;
        if output.status.success() {
            String::from_utf8(output.stdout)
                .ok()
                .map(|s| s.trim().to_owned())
        } else {
            None
        }
    }
}


// ---------------------------------------------------------------------------
// SshConfigManager — RED tests (issue 13)
// ---------------------------------------------------------------------------
// ⚠️  Testability requirement: `SshConfigManager` must expose a
// `with_paths(polis_config: PathBuf, user_config: PathBuf, sockets_dir: PathBuf) -> Self`
// constructor so tests can inject temp directories instead of `$HOME`.
// The production `new()` delegates to `with_paths(...)`.
#[cfg(test)]
mod ssh_config_manager_tests {
    use super::{OsSocketsDir, SshConfigManager};
    use crate::application::ports::SshConfigurator;
    use crate::infra::ssh::identity::IdentityKeyProvider;
    use crate::infra::ssh::known_hosts::KnownHostsOps;

    // -----------------------------------------------------------------------
    // Stub implementations for DI
    // -----------------------------------------------------------------------

    struct StubIdentityProvider;
    impl IdentityKeyProvider for StubIdentityProvider {
        fn ensure_identity_key(&self) -> anyhow::Result<String> {
            Ok("ssh-ed25519 AAAA stub@test".to_string())
        }
    }

    struct StubKnownHosts;
    impl KnownHostsOps for StubKnownHosts {
        fn update(&self, _host_key_line: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn remove(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn manager_in(dir: &tempfile::TempDir) -> SshConfigManager {
        SshConfigManager::with_deps(
            dir.path().join("ssh").join("config.d").join("polis"),
            dir.path().join("ssh").join("config"),
            dir.path().join(".polis"),
            Box::new(OsSocketsDir::new(
                dir.path()
                    .join("ssh")
                    .join("config.d")
                    .join("polis-sockets"),
            )),
            Box::new(StubKnownHosts),
            Box::new(StubIdentityProvider),
        )
    }

    // -----------------------------------------------------------------------
    // is_configured
    // -----------------------------------------------------------------------

    #[test]
    fn test_ssh_config_manager_is_configured_returns_false_when_polis_config_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        assert!(!mgr.is_configured().expect("is_configured should not error"));
    }

    #[test]
    fn test_ssh_config_manager_is_configured_returns_false_when_include_directive_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        // Create polis config but no Include in user config
        mgr.create_polis_config().expect("create_polis_config");
        std::fs::create_dir_all(dir.path().join("ssh")).expect("mkdir");
        std::fs::write(
            dir.path().join("ssh").join("config"),
            "Host *\n    ServerAliveInterval 60\n",
        )
        .expect("write user config");
        assert!(!mgr.is_configured().expect("is_configured should not error"));
    }

    #[test]
    fn test_ssh_config_manager_is_configured_returns_true_when_both_present() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        mgr.add_include_directive().expect("add_include_directive");
        assert!(mgr.is_configured().expect("is_configured should not error"));
    }

    // -----------------------------------------------------------------------
    // create_polis_config — security properties (V-001, V-002, V-011)
    // -----------------------------------------------------------------------

    #[test]
    fn test_ssh_config_manager_create_polis_config_contains_forward_agent_no() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        let content =
            std::fs::read_to_string(dir.path().join("ssh").join("config.d").join("polis"))
                .expect("config file should exist");
        assert!(
            content.contains("ForwardAgent no"),
            "V-001: ForwardAgent must be no"
        );
    }

    #[test]
    fn test_ssh_config_manager_create_polis_config_does_not_contain_forward_agent_yes() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        let content =
            std::fs::read_to_string(dir.path().join("ssh").join("config.d").join("polis"))
                .expect("config file should exist");
        assert!(
            !content.contains("ForwardAgent yes"),
            "V-001: ForwardAgent yes must never appear"
        );
    }

    #[test]
    fn test_ssh_config_manager_create_polis_config_contains_strict_host_key_checking_yes() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        let content =
            std::fs::read_to_string(dir.path().join("ssh").join("config.d").join("polis"))
                .expect("config file should exist");
        assert!(
            content.contains("StrictHostKeyChecking yes"),
            "V-002: StrictHostKeyChecking must be yes"
        );
    }

    #[test]
    fn test_ssh_config_manager_create_polis_config_contains_user_polis() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        let content =
            std::fs::read_to_string(dir.path().join("ssh").join("config.d").join("polis"))
                .expect("config file should exist");
        assert!(content.contains("User polis"), "V-011: User must be polis");
        assert!(
            !content.contains("User vscode"),
            "V-011: User vscode must not appear"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_ssh_config_manager_create_polis_config_sets_file_permissions_600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        let mode = std::fs::metadata(dir.path().join("ssh").join("config.d").join("polis"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "V-004: ssh_config must be 600");
    }

    #[cfg(unix)]
    #[test]
    fn test_ssh_config_manager_create_polis_config_sets_parent_dir_permissions_700() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        let mode = std::fs::metadata(dir.path().join("ssh").join("config.d"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "V-004: config.d dir must be 700");
    }

    // -----------------------------------------------------------------------
    // add_include_directive
    // -----------------------------------------------------------------------

    #[test]
    fn test_ssh_config_manager_add_include_directive_prepends_to_existing_config() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        let ssh_dir = dir.path().join("ssh");
        std::fs::create_dir_all(&ssh_dir).expect("mkdir");
        std::fs::write(
            ssh_dir.join("config"),
            "Host *\n    ServerAliveInterval 60\n",
        )
        .expect("write");
        mgr.add_include_directive().expect("add_include_directive");
        let content = std::fs::read_to_string(ssh_dir.join("config")).expect("config should exist");
        assert!(
            content.starts_with("Include config.d/polis\n"),
            "Include must be at the top of ~/.ssh/config"
        );
    }

    #[test]
    fn test_ssh_config_manager_add_include_directive_creates_config_when_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.add_include_directive().expect("add_include_directive");
        let content = std::fs::read_to_string(dir.path().join("ssh").join("config"))
            .expect("config should be created");
        assert!(content.contains("Include config.d/polis"));
    }

    #[test]
    fn test_ssh_config_manager_add_include_directive_is_idempotent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.add_include_directive().expect("first call");
        mgr.add_include_directive().expect("second call");
        let content = std::fs::read_to_string(dir.path().join("ssh").join("config"))
            .expect("config should exist");
        let count = content.matches("Include config.d/polis").count();
        assert_eq!(count, 1, "Include directive must appear exactly once");
    }

    // -----------------------------------------------------------------------
    // create_sockets_dir
    // -----------------------------------------------------------------------

    #[cfg(not(windows))]
    #[test]
    fn test_ssh_config_manager_create_sockets_dir_creates_directory() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_sockets_dir().expect("create_sockets_dir");
        assert!(
            dir.path()
                .join("ssh")
                .join("config.d")
                .join("polis-sockets")
                .is_dir()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_ssh_config_manager_create_sockets_dir_sets_permissions_700() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_sockets_dir().expect("create_sockets_dir");
        let mode = std::fs::metadata(
            dir.path()
                .join("ssh")
                .join("config.d")
                .join("polis-sockets"),
        )
        .expect("metadata")
        .permissions()
        .mode();
        assert_eq!(mode & 0o777, 0o700, "V-007: sockets dir must be 700");
    }

    // -----------------------------------------------------------------------
    // validate_permissions
    // -----------------------------------------------------------------------

    #[test]
    fn test_ssh_config_manager_validate_permissions_returns_ok_when_config_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        // No config file — must not error
        assert!(mgr.validate_permissions().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_ssh_config_manager_validate_permissions_returns_err_when_permissions_wrong() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_polis_config().expect("create_polis_config");
        // Deliberately set wrong permissions
        std::fs::set_permissions(
            dir.path().join("ssh").join("config.d").join("polis"),
            std::fs::Permissions::from_mode(0o644),
        )
        .expect("set permissions");
        assert!(
            mgr.validate_permissions().is_err(),
            "V-004: must reject config with permissions != 600"
        );
    }

    // -----------------------------------------------------------------------
    // DI delegation — ensure_identity & update_host_key
    // -----------------------------------------------------------------------

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct TrackingIdentityProvider {
        called: Arc<AtomicBool>,
    }
    impl IdentityKeyProvider for TrackingIdentityProvider {
        fn ensure_identity_key(&self) -> anyhow::Result<String> {
            self.called.store(true, Ordering::SeqCst);
            Ok("ssh-ed25519 AAAA tracking@test".to_string())
        }
    }

    struct TrackingKnownHosts {
        update_called: Arc<AtomicBool>,
    }
    impl KnownHostsOps for TrackingKnownHosts {
        fn update(&self, _host_key_line: &str) -> anyhow::Result<()> {
            self.update_called.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn remove(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_ensure_identity_delegates_to_injected_provider() {
        let called = Arc::new(AtomicBool::new(false));
        let provider = TrackingIdentityProvider {
            called: called.clone(),
        };
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = SshConfigManager::with_deps(
            dir.path().join("ssh").join("config.d").join("polis"),
            dir.path().join("ssh").join("config"),
            dir.path().join(".polis"),
            Box::new(OsSocketsDir::new(
                dir.path()
                    .join("ssh")
                    .join("config.d")
                    .join("polis-sockets"),
            )),
            Box::new(StubKnownHosts),
            Box::new(provider),
        );
        let result = mgr.ensure_identity().await;
        assert!(result.is_ok(), "ensure_identity should succeed");
        assert_eq!(
            result.expect("ensure_identity result"),
            "ssh-ed25519 AAAA tracking@test",
            "ensure_identity must return the value from the injected provider"
        );
        assert!(
            called.load(Ordering::SeqCst),
            "ensure_identity must delegate to the injected IdentityKeyProvider"
        );
    }

    #[tokio::test]
    async fn test_update_host_key_delegates_to_injected_known_hosts() {
        let update_called = Arc::new(AtomicBool::new(false));
        let known_hosts = TrackingKnownHosts {
            update_called: update_called.clone(),
        };
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = SshConfigManager::with_deps(
            dir.path().join("ssh").join("config.d").join("polis"),
            dir.path().join("ssh").join("config"),
            dir.path().join(".polis"),
            Box::new(OsSocketsDir::new(
                dir.path()
                    .join("ssh")
                    .join("config.d")
                    .join("polis-sockets"),
            )),
            Box::new(known_hosts),
            Box::new(StubIdentityProvider),
        );
        let result = mgr
            .update_host_key("workspace ssh-ed25519 AAAA test-key")
            .await;
        assert!(result.is_ok(), "update_host_key should succeed");
        assert!(
            update_called.load(Ordering::SeqCst),
            "update_host_key must delegate to the injected KnownHostsOps"
        );
    }

    // -------------------------------------------------------------------
    // Property 2 (Part A): Preservation — Unix SSH Config Unchanged
    //
    // **Validates: Requirements 3.1, 3.2, 3.3**
    //
    // On Unix, `create_polis_config()` MUST produce a config containing:
    //   - `ProxyCommand polis _ssh-proxy` (bare command, not absolute path)
    //   - `ControlMaster auto`
    //   - `ControlPath ~/.polis/sockets/%r@%h:%p`
    //   - `ControlPersist 30s`
    //   - `ForwardAgent no`
    //   - `StrictHostKeyChecking yes`
    //   - `User polis`
    //
    // This is a deterministic property (the config template is fixed), so
    // we use proptest with a dummy input to express it in the PBT framework.
    // The property holds for every invocation — the config never varies.
    // -------------------------------------------------------------------
    proptest::proptest! {
        #[test]
        #[cfg(not(windows))]
        fn prop_unix_ssh_config_preservation(_dummy in 0u8..1) {
            let dir = tempfile::TempDir::new().expect("tempdir");
            let mgr = manager_in(&dir);
            mgr.create_polis_config().expect("create_polis_config");
            let content = std::fs::read_to_string(
                dir.path().join("ssh").join("config.d").join("polis"),
            )
            .expect("config file should exist");

            // Bare command (not absolute path)
            proptest::prop_assert!(
                content.contains("ProxyCommand polis _ssh-proxy"),
                "Unix config must use bare `polis` command in ProxyCommand"
            );
            // ControlMaster directives
            proptest::prop_assert!(
                content.contains("ControlMaster auto"),
                "Unix config must include ControlMaster auto"
            );
            proptest::prop_assert!(
                content.contains("ControlPath ~/.ssh/config.d/polis-sockets/%r@%h:%p"),
                "Unix config must include ControlPath"
            );
            proptest::prop_assert!(
                content.contains("ControlPersist 30s"),
                "Unix config must include ControlPersist 30s"
            );
            // Security directives
            proptest::prop_assert!(
                content.contains("ForwardAgent no"),
                "Unix config must include ForwardAgent no"
            );
            proptest::prop_assert!(
                content.contains("StrictHostKeyChecking yes"),
                "Unix config must include StrictHostKeyChecking yes"
            );
            proptest::prop_assert!(
                content.contains("User polis"),
                "Unix config must include User polis"
            );
        }
    }
}
