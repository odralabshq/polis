//! `polis delete [--all]` — remove workspace (and optionally cached images).

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::commands::DeleteArgs;
use crate::ssh::KnownHostsManager;
use crate::state::StateManager;
use crate::workspace::WorkspaceDriver;

/// Run `polis delete [--all]`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be removed or state cannot be cleared.
pub fn run(
    args: &DeleteArgs,
    state_mgr: &StateManager,
    driver: &dyn WorkspaceDriver,
) -> Result<()> {
    if args.all {
        delete_all(state_mgr, driver)
    } else {
        delete_workspace(state_mgr, driver)
    }
}

/// Prompt for confirmation, reading from stdin (works with both TTY and piped input).
///
/// # Errors
///
/// Returns an error if stdin cannot be read or is closed (EOF).
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    print!("{prompt} [y/N]: ");
    std::io::stdout().flush().context("flushing stdout")?;
    let mut line = String::new();
    let n = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading confirmation")?;
    anyhow::ensure!(n > 0, "no input provided (stdin closed)");
    Ok(line.trim().eq_ignore_ascii_case("y"))
}

fn delete_workspace(state_mgr: &StateManager, driver: &dyn WorkspaceDriver) -> Result<()> {
    println!();
    println!("  This will remove the workspace and all agent data.");
    println!("  Configuration and cached images are preserved.");
    println!();

    if !confirm("Continue?")? {
        println!("Cancelled.");
        return Ok(());
    }

    if let Some(state) = state_mgr.load().context("reading workspace state")? {
        if driver.is_running(&state.workspace_id)? {
            driver.stop(&state.workspace_id)?;
        }
        driver.remove(&state.workspace_id)?;
    }

    state_mgr.clear().context("clearing state file")?;
    remove_certificates()?;

    println!("Workspace removed");
    println!();
    println!("Run: polis run <agent>  to create a new workspace");

    Ok(())
}

fn delete_all(state_mgr: &StateManager, driver: &dyn WorkspaceDriver) -> Result<()> {
    println!();
    println!("  This will remove everything including cached images (~3.5 GB).");
    println!("  Only configuration is preserved.");
    println!();

    if !confirm("Continue?")? {
        println!("Cancelled.");
        return Ok(());
    }

    if let Some(state) = state_mgr.load().context("reading workspace state")? {
        if driver.is_running(&state.workspace_id)? {
            driver.stop(&state.workspace_id)?;
        }
        driver.remove(&state.workspace_id)?;
    }

    driver.remove_cached_images()?;
    state_mgr.clear().context("clearing state file")?;
    remove_certificates()?;
    remove_ssh_config()?;
    remove_known_hosts()?;

    println!("All data removed");

    Ok(())
}

/// Remove certificates from `~/.polis/certs/`.
///
/// Non-fatal: logs a warning if removal fails.
fn remove_certificates() -> Result<()> {
    let certs_dir = get_polis_path("certs")?;
    remove_dir_if_exists(&certs_dir)
}

/// Remove SSH config from `~/.polis/ssh_config`.
///
/// Non-fatal: logs a warning if removal fails.
fn remove_ssh_config() -> Result<()> {
    let ssh_config = get_polis_path("ssh_config")?;
    remove_file_if_exists(&ssh_config)?;
    // Also remove sockets directory
    let sockets_dir = get_polis_path("sockets")?;
    let _ = remove_dir_if_exists(&sockets_dir);
    Ok(())
}

/// Remove `known_hosts` from `~/.polis/known_hosts`.
fn remove_known_hosts() -> Result<()> {
    KnownHostsManager::new()?.remove()
}

/// Get a path under `~/.polis/`.
fn get_polis_path(name: &str) -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    Ok(home.join(".polis").join(name))
}

/// Remove a directory if it exists.
///
/// # Errors
///
/// Returns an error if the directory exists but cannot be removed.
pub fn remove_dir_if_exists(path: &std::path::Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

/// Remove a file if it exists.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be removed.
pub fn remove_file_if_exists(path: &std::path::Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── remove_dir_if_exists ─────────────────────────────────────────────────

    #[test]
    fn test_remove_dir_if_exists_removes_existing_dir() {
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("certs");
        std::fs::create_dir_all(&target).expect("create dir");
        std::fs::write(target.join("ca.pem"), b"cert").expect("write file");

        let result = remove_dir_if_exists(&target);
        assert!(result.is_ok());
        assert!(!target.exists());
    }

    #[test]
    fn test_remove_dir_if_exists_noop_when_absent() {
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("nonexistent");

        let result = remove_dir_if_exists(&target);
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_dir_if_exists_removes_nested_contents() {
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("certs");
        std::fs::create_dir_all(target.join("ca")).expect("create nested dir");
        std::fs::write(target.join("ca").join("ca.pem"), b"cert").expect("write file");

        let result = remove_dir_if_exists(&target);
        assert!(result.is_ok());
        assert!(!target.exists());
    }

    // ── remove_file_if_exists ────────────────────────────────────────────────

    #[test]
    fn test_remove_file_if_exists_removes_existing_file() {
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("ssh_config");
        std::fs::write(&target, b"config").expect("write file");

        let result = remove_file_if_exists(&target);
        assert!(result.is_ok());
        assert!(!target.exists());
    }

    #[test]
    fn test_remove_file_if_exists_noop_when_absent() {
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("nonexistent");

        let result = remove_file_if_exists(&target);
        assert!(result.is_ok());
    }

    // ── delete flow (post-confirmation logic) ────────────────────────────────

    use crate::workspace::MockDriver;

    fn setup_state_and_config(dir: &TempDir) -> (StateManager, std::path::PathBuf) {
        let polis_dir = dir.path().join(".polis");
        std::fs::create_dir_all(&polis_dir).expect("create .polis dir");
        let state_path = polis_dir.join("state.json");
        std::fs::write(
            &state_path,
            r#"{"stage":"agent_ready","agent":"claude-dev","workspace_id":"ws-test01","started_at":"2026-02-17T14:30:00Z"}"#,
        ).expect("write state");
        let config_path = polis_dir.join("config.yaml");
        std::fs::write(&config_path, b"security_level: balanced\n").expect("write config");
        (StateManager::with_path(state_path), config_path)
    }

    #[test]
    fn test_delete_flow_removes_state_file() {
        let dir = TempDir::new().expect("tempdir");
        let (state_mgr, _config) = setup_state_and_config(&dir);
        let driver = MockDriver { running: false };

        // Simulate post-confirmation delete logic
        if let Some(state) = state_mgr.load().unwrap() {
            driver.remove(&state.workspace_id).unwrap();
        }
        state_mgr.clear().unwrap();

        let state_path = dir.path().join(".polis").join("state.json");
        assert!(
            !state_path.exists(),
            "state.json must be removed after delete"
        );
    }

    #[test]
    fn test_delete_flow_preserves_config() {
        let dir = TempDir::new().expect("tempdir");
        let (state_mgr, config_path) = setup_state_and_config(&dir);
        let driver = MockDriver { running: false };

        if let Some(state) = state_mgr.load().unwrap() {
            driver.remove(&state.workspace_id).unwrap();
        }
        state_mgr.clear().unwrap();

        assert!(
            config_path.exists(),
            "config.yaml must be preserved after delete"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use tempfile::TempDir;

    proptest! {
        /// remove_dir_if_exists is idempotent
        #[test]
        fn prop_remove_dir_if_exists_idempotent(create in any::<bool>()) {
            let dir = TempDir::new().expect("tempdir");
            let target = dir.path().join("test_dir");

            if create {
                std::fs::create_dir_all(&target).expect("create dir");
            }

            // First call
            let r1 = remove_dir_if_exists(&target);
            prop_assert!(r1.is_ok());

            // Second call (should also succeed)
            let r2 = remove_dir_if_exists(&target);
            prop_assert!(r2.is_ok());

            // Directory should not exist after either call
            prop_assert!(!target.exists());
        }

        /// remove_file_if_exists is idempotent
        #[test]
        fn prop_remove_file_if_exists_idempotent(create in any::<bool>()) {
            let dir = TempDir::new().expect("tempdir");
            let target = dir.path().join("test_file");

            if create {
                std::fs::write(&target, b"content").expect("write file");
            }

            // First call
            let r1 = remove_file_if_exists(&target);
            prop_assert!(r1.is_ok());

            // Second call (should also succeed)
            let r2 = remove_file_if_exists(&target);
            prop_assert!(r2.is_ok());

            // File should not exist after either call
            prop_assert!(!target.exists());
        }

        /// remove_dir_if_exists removes any content
        #[test]
        fn prop_remove_dir_removes_all_content(file_count in 0usize..5) {
            let dir = TempDir::new().expect("tempdir");
            let target = dir.path().join("test_dir");
            std::fs::create_dir_all(&target).expect("create dir");

            for i in 0..file_count {
                std::fs::write(target.join(format!("file{i}")), b"x").expect("write");
            }

            let result = remove_dir_if_exists(&target);
            prop_assert!(result.is_ok());
            prop_assert!(!target.exists());
        }
    }
}
