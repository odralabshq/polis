//! SSH utilities — host key pinning (`KnownHostsManager`).

use anyhow::{Context, Result};
use std::path::PathBuf;


/// Generates a passphrase-free ED25519 keypair at `~/.polis/id_ed25519` if it
/// does not already exist.
/// Returns the public key string (`ssh-ed25519 <material>`).
/// # Errors
/// Returns an error if key generation or file I/O fails.
pub fn ensure_identity_key() -> Result<String> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let key_path = home.join(".polis").join("id_ed25519");
    let pub_path = home.join(".polis").join("id_ed25519.pub");

    if !key_path.exists() {
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            set_permissions(parent, 0o700)?;
        }
        let status = std::process::Command::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-N",
                "", // no passphrase
                "-f",
                key_path
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("non-UTF8 path"))?,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("ssh-keygen not found")?;
        anyhow::ensure!(status.success(), "ssh-keygen failed");
        set_permissions(&key_path, 0o600)?;
    }

    let pubkey = std::fs::read_to_string(&pub_path)
        .with_context(|| format!("read {}", pub_path.display()))?;
    Ok(pubkey.trim().to_string())
}

/// Manages `~/.polis/known_hosts` for SSH host key pinning.
pub struct KnownHostsManager {
    path: PathBuf,
}

impl KnownHostsManager {
    /// Creates a manager pointing at `~/.polis/known_hosts`.
    /// # Errors
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        Ok(Self::with_path(home.join(".polis").join("known_hosts")))
    }

    /// Creates a manager pointing at an arbitrary path (for testing).
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Writes `host_key_line` to the `known_hosts` file, creating parent dirs as needed.
    /// Sets file permissions to 600 and parent directory to 700 on Unix.
    /// # Errors
    /// Returns an error if the file cannot be written or permissions cannot be set.
    pub fn update(&self, host_key_line: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            set_permissions(parent, 0o700)?;
        }
        std::fs::write(&self.path, host_key_line)
            .with_context(|| format!("write {}", self.path.display()))?;
        set_permissions(&self.path, 0o600)?;
        Ok(())
    }

    /// Removes the `known_hosts` file if it exists.
    /// # Errors
    /// Returns an error if the file exists but cannot be removed.
    pub fn remove(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .with_context(|| format!("remove {}", self.path.display()))?;
        }
        Ok(())
    }
}

/// # Errors
/// This function will return an error if the underlying operations fail.
#[cfg(unix)]
fn set_permissions(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("set permissions on {}", path.display()))
}

/// # Errors
/// This function will return an error if the underlying operations fail.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn set_permissions(_path: &std::path::Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    // ⚠️  Testability requirement: `KnownHostsManager` must expose a
    // `with_path(path: PathBuf) -> Self` constructor so tests can inject a
    // temp directory instead of relying on `$HOME`.  The production `new()`
    // delegates to `with_path(home.join(".polis").join("known_hosts"))`.
    //
    // ⚠️  Testability requirement: extract a `pub fn validate_host_key(key: &str)
    // -> Result<()>` from the inline check in `extract_from_multipass` /
    // `extract_from_docker` so the validation logic can be unit-tested directly.
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn manager_in(dir: &tempfile::TempDir) -> KnownHostsManager {
        KnownHostsManager::with_path(dir.path().join("known_hosts"))
    }

    const VALID_KEY: &str = "workspace ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKeyMaterialHere";

    // -----------------------------------------------------------------------
    // KnownHostsManager::update
    // -----------------------------------------------------------------------

    #[test]
    fn test_known_hosts_manager_update_creates_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("known_hosts");
        assert!(!path.exists());
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        assert!(path.exists());
    }

    #[test]
    fn test_known_hosts_manager_update_creates_parent_directory() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let nested = dir.path().join("a").join("b");
        let mgr = KnownHostsManager::with_path(nested.join("known_hosts"));
        mgr.update(VALID_KEY)
            .expect("update should create parent dirs");
        assert!(nested.exists());
    }

    #[test]
    fn test_known_hosts_manager_update_overwrites_existing_content() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.update("workspace ssh-ed25519 OldKey")
            .expect("first update");
        mgr.update(VALID_KEY).expect("second update");
        let content =
            std::fs::read_to_string(dir.path().join("known_hosts")).expect("file should exist");
        assert_eq!(content, VALID_KEY);
    }

    #[cfg(unix)]
    #[test]
    fn test_known_hosts_manager_update_sets_file_permissions_600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        let mode = std::fs::metadata(dir.path().join("known_hosts"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "file must be 600");
    }

    #[cfg(unix)]
    #[test]
    fn test_known_hosts_manager_update_sets_parent_dir_permissions_700() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let parent = dir.path().join("polis_dir");
        let mgr = KnownHostsManager::with_path(parent.join("known_hosts"));
        mgr.update(VALID_KEY).expect("update should succeed");
        let mode = std::fs::metadata(&parent)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "directory must be 700");
    }

    // -----------------------------------------------------------------------
    // KnownHostsManager::remove
    // -----------------------------------------------------------------------

    #[test]
    fn test_known_hosts_manager_remove_deletes_existing_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("known_hosts");
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        mgr.remove().expect("remove should succeed");
        assert!(!path.exists());
    }

    #[test]
    fn test_known_hosts_manager_remove_is_idempotent_when_file_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        // File never created — remove must not error.
        let result = mgr.remove();
        assert!(result.is_ok());
    }


}

// ---------------------------------------------------------------------------
// SocketsDir
// ---------------------------------------------------------------------------

/// Abstracts creation of the SSH control-socket directory.
/// The production implementation ([`OsSocketsDir`]) creates the directory on
/// the filesystem.  In tests a `MockSocketsDir` (generated by `mockall`) can
/// be injected to verify call behaviour or simulate failures without touching
/// the real filesystem.
#[cfg_attr(test, mockall::automock)]
pub trait SocketsDir {
    /// Ensures the sockets directory exists with the correct permissions.
    /// # Errors
    /// Returns an error if the directory cannot be created or permissions set.
    fn ensure_exists(&self) -> Result<()>;
}

/// OS-backed implementation of [`SocketsDir`].
/// On Unix it creates `path` with permissions 700.  On Windows it is a no-op
/// because `ControlMaster` is not supported by Windows OpenSSH.
pub struct OsSocketsDir {
    #[cfg_attr(windows, allow(dead_code))]
    path: std::path::PathBuf,
}

impl OsSocketsDir {
    /// Creates an [`OsSocketsDir`] pointing at `path`.
    #[must_use]
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl SocketsDir for OsSocketsDir {
    /// # Errors
    /// This function will return an error if the underlying operations fail.
    fn ensure_exists(&self) -> Result<()> {
        #[cfg(not(windows))]
        {
            std::fs::create_dir_all(&self.path)
                .with_context(|| format!("create dir {}", self.path.display()))?;
            set_permissions(&self.path, 0o700)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SshConfigManager
// ---------------------------------------------------------------------------

/// Manages the polis SSH config files and `~/.ssh/config` Include directive.
pub struct SshConfigManager {
    polis_config_path: std::path::PathBuf,
    user_config_path: std::path::PathBuf,
    sockets_dir: Box<dyn SocketsDir>,
}

impl SshConfigManager {
    /// Creates a manager using the real `$HOME`-based paths.
    /// # Errors
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        Ok(Self::with_paths(
            home.join(".polis").join("ssh_config"),
            home.join(".ssh").join("config"),
            Box::new(OsSocketsDir::new(home.join(".polis").join("sockets"))),
        ))
    }

    /// Creates a manager with explicit paths (for testing).
    #[must_use]
    pub fn with_paths(
        polis_config_path: std::path::PathBuf,
        user_config_path: std::path::PathBuf,
        sockets_dir: Box<dyn SocketsDir>,
    ) -> Self {
        Self {
            polis_config_path,
            user_config_path,
            sockets_dir,
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
            if content.contains("Include ~/.polis/ssh_config") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Writes the hardened polis SSH config to `~/.polis/ssh_config`.
    /// Sets file permissions to 600 and parent directory to 700 on Unix.
    /// # Errors
    /// Returns an error if the file cannot be written or permissions cannot be set.
    pub fn create_polis_config(&self) -> Result<()> {
        // ControlMaster/ControlPath/ControlPersist use Unix domain sockets and
        // are not supported by Windows OpenSSH — omit them on Windows.
        // Windows OpenSSH ProxyCommand requires absolute path to executable.
        #[cfg(not(windows))]
        let config = "\
# ~/.polis/ssh_config (managed by polis — DO NOT EDIT)
Host workspace
    HostName workspace
    User polis
    ProxyCommand polis _ssh-proxy
    StrictHostKeyChecking yes
    UserKnownHostsFile ~/.polis/known_hosts
    IdentityFile ~/.polis/id_ed25519
    ControlMaster auto
    ControlPath ~/.polis/sockets/%r@%h:%p
    ControlPersist 30s
    ForwardAgent no
    IdentitiesOnly yes
";
        #[cfg(windows)]
        let config = format!(
            "\
# ~/.polis/ssh_config (managed by polis — DO NOT EDIT)
Host workspace
    HostName workspace
    User polis
    ProxyCommand \"{}\" _ssh-proxy
    StrictHostKeyChecking yes
    UserKnownHostsFile ~/.polis/known_hosts
    IdentityFile ~/.polis/id_ed25519
    ForwardAgent no
    IdentitiesOnly yes
",
            std::env::current_exe()
                .unwrap_or_else(|_| std::path::PathBuf::from("polis.exe"))
                .display()
        );
        if let Some(parent) = self.polis_config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            set_permissions(parent, 0o700)?;
        }
        std::fs::write(&self.polis_config_path, config)
            .with_context(|| format!("write {}", self.polis_config_path.display()))?;
        set_permissions(&self.polis_config_path, 0o600)?;
        Ok(())
    }

    /// Prepends `Include ~/.polis/ssh_config` to `~/.ssh/config`, creating
    /// the file if absent. Idempotent.
    /// # Errors
    /// Returns an error if the file cannot be read or written.
    pub fn add_include_directive(&self) -> Result<()> {
        const INCLUDE: &str = "Include ~/.polis/ssh_config\n";
        if let Some(parent) = self.user_config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            set_permissions(parent, 0o700)?;
        }
        if self.user_config_path.exists() {
            let content = std::fs::read_to_string(&self.user_config_path)
                .with_context(|| format!("read {}", self.user_config_path.display()))?;
            if content.contains("Include ~/.polis/ssh_config") {
                return Ok(());
            }
            std::fs::write(&self.user_config_path, format!("{INCLUDE}{content}"))
                .with_context(|| format!("write {}", self.user_config_path.display()))?;
        } else {
            std::fs::write(&self.user_config_path, INCLUDE)
                .with_context(|| format!("write {}", self.user_config_path.display()))?;
        }
        set_permissions(&self.user_config_path, 0o600)?;
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

    fn manager_in(dir: &tempfile::TempDir) -> SshConfigManager {
        SshConfigManager::with_paths(
            dir.path().join("polis").join("ssh_config"),
            dir.path().join("ssh").join("config"),
            Box::new(OsSocketsDir::new(dir.path().join("polis").join("sockets"))),
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
        let content = std::fs::read_to_string(dir.path().join("polis").join("ssh_config"))
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
        let content = std::fs::read_to_string(dir.path().join("polis").join("ssh_config"))
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
        let content = std::fs::read_to_string(dir.path().join("polis").join("ssh_config"))
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
        let content = std::fs::read_to_string(dir.path().join("polis").join("ssh_config"))
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
        let mode = std::fs::metadata(dir.path().join("polis").join("ssh_config"))
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
        let mode = std::fs::metadata(dir.path().join("polis"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "V-004: .polis dir must be 700");
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
            content.starts_with("Include ~/.polis/ssh_config\n"),
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
        assert!(content.contains("Include ~/.polis/ssh_config"));
    }

    #[test]
    fn test_ssh_config_manager_add_include_directive_is_idempotent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.add_include_directive().expect("first call");
        mgr.add_include_directive().expect("second call");
        let content = std::fs::read_to_string(dir.path().join("ssh").join("config"))
            .expect("config should exist");
        let count = content.matches("Include ~/.polis/ssh_config").count();
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
        assert!(dir.path().join("polis").join("sockets").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn test_ssh_config_manager_create_sockets_dir_sets_permissions_700() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.create_sockets_dir().expect("create_sockets_dir");
        let mode = std::fs::metadata(dir.path().join("polis").join("sockets"))
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
            dir.path().join("polis").join("ssh_config"),
            std::fs::Permissions::from_mode(0o644),
        )
        .expect("set permissions");
        assert!(
            mgr.validate_permissions().is_err(),
            "V-004: must reject config with permissions != 600"
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
                dir.path().join("polis").join("ssh_config"),
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
                content.contains("ControlPath ~/.polis/sockets/%r@%h:%p"),
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

impl crate::application::ports::SshConfigurator for SshConfigManager {
    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn ensure_identity(&self) -> Result<String> {
        ensure_identity_key()
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn update_host_key(&self, host_key: &str) -> Result<()> {
        KnownHostsManager::new()?.update(host_key)
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn is_configured(&self) -> Result<bool> {
        self.is_configured()
    }

    /// # Errors
    /// This function will return an error if the underlying operations fail.
    async fn setup_config(&self) -> Result<()> {
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
}
